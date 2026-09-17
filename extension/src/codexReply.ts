import { randomUUID } from "node:crypto";
import { createConnection, type Socket } from "node:net";
import { codexSessionRoute } from "./sessionNavigation";

const MAX_FRAME = 64 * 1024 * 1024;
type Message = Record<string, any>;

// Codex 26.908's versioned local peer protocol. Send through the conversation's
// existing owner so its model, permissions, approvals and history stay intact.
// Never launch another app-server against a thread the editor already owns.
export async function sendCodexReply(
  sessionId: string, text: string, cwd: string, _busy: boolean,
  pipe = "\\\\.\\pipe\\codex-ipc", timeoutMs = 30000
): Promise<void> {
  if (!codexSessionRoute(sessionId) || !text.trim() || text.length > 8192) {
    throw new Error("Enter a message of at most 8,192 characters.");
  }
  const socket = createConnection(pipe);
  const peer = new Peer(socket, timeoutMs);
  try {
    await peer.connected;
    const initialized = await peer.request("initialize", 0, { clientType: "codex-lid-guard" });
    if (typeof initialized.result?.clientId !== "string") { throw new Error("Codex reply connection is unavailable."); }
    peer.clientId = initialized.result.clientId;
    const owner = await peer.request("thread-owner-discovery", 1, { hostId: "local", conversationId: sessionId });
    if (typeof owner.handledByClientId !== "string") { throw new Error("Open this chat in Codex once, then try again."); }
    const input = [{ type: "text", text, text_elements: [] }];
    // Saved history and the overlay's busy indicator can lag the live owner.
    // Ask the owner to steer first; only a definitive inactive-turn rejection
    // permits starting a new turn. Never retry an uncertain delivery.
    const clientUserMessageId = randomUUID();
    try {
      const response = await peer.request("thread-follower-steer-turn", 1, {
        conversationId: sessionId, input, attachments: [], clientUserMessageId,
        restoreMessage: { id: clientUserMessageId, text, cwd, createdAt: Date.now(),
          context: { prompt: text, addedFiles: [], fileAttachments: [], imageAttachments: [],
            ideContext: null, workspaceRoots: [cwd] } }
      }, owner.handledByClientId);
      if (typeof response.result?.result?.turnId !== "string") { throw unconfirmed(); }
    } catch (error) {
      if (!(error instanceof PeerRejection) || !error.inactiveTurn(sessionId)) { throw error; }
      const response = await peer.request("thread-follower-start-turn", 2, {
        conversationId: sessionId,
        turnStart: { request: { threadId: sessionId, input, clientUserMessageId },
          context: { inheritThreadSettings: true } }
      }, owner.handledByClientId);
      if (typeof response.result?.result?.turn?.id !== "string") { throw unconfirmed(); }
    }
  } finally { peer.dispose(); }
}

function unconfirmed(): Error { return new Error("Send not confirmed. Check the chat before retrying."); }

class PeerRejection extends Error {
  constructor(private detail: unknown) {
    super(detail === "no-client-found" ? "Open this chat in Codex once, then try again."
      : "Codex could not accept the message. Check the chat before retrying.");
  }
  inactiveTurn(sessionId: string): boolean {
    return this.detail === `Cannot steer conversation ${sessionId} because its active turn already ended`
      || this.detail === "no active turn to steer";
  }
}

class Peer {
  clientId = "initializing-client";
  readonly connected: Promise<void>;
  private data: Buffer = Buffer.alloc(0);
  private pending = new Map<string, { resolve: (message: Message) => void; reject: (error: Error) => void; timer: NodeJS.Timeout }>();
  private failure: Error | undefined;

  constructor(private socket: Socket, private timeoutMs: number) {
    this.connected = new Promise((resolve, reject) => {
      socket.once("connect", resolve);
      socket.once("error", () => reject(new Error("Open the selected chat in Codex, then try again.")));
      socket.setTimeout(timeoutMs, () => socket.destroy(new Error("timeout")));
    });
    socket.on("error", () => this.fail(new Error("Send not confirmed. Check the chat before retrying.")));
    socket.on("close", () => this.fail(new Error("Send not confirmed. Check the chat before retrying.")));
    socket.on("data", (chunk: Buffer) => {
      this.data = Buffer.concat([this.data, chunk]);
      while (this.data.length >= 4) {
        const length = this.data.readUInt32LE(0);
        if (length === 0 || length > MAX_FRAME) { socket.destroy(); return; }
        if (this.data.length < length + 4) { return; }
        let message: Message;
        try { message = JSON.parse(this.data.subarray(4, length + 4).toString("utf8")); }
        catch { socket.destroy(); return; }
        this.data = this.data.subarray(length + 4);
        if (message?.type === "client-discovery-request") {
          this.write({ type: "client-discovery-response", requestId: message.requestId, response: { canHandle: false } });
        } else if (message?.type === "response") {
          const pending = this.pending.get(message.requestId);
          if (!pending) { continue; }
          this.pending.delete(message.requestId);
          clearTimeout(pending.timer);
          if (message.resultType === "success") { pending.resolve(message); }
          else { pending.reject(new PeerRejection(message.error)); }
        }
      }
    });
  }

  request(method: string, version: number, params: unknown, targetClientId?: string): Promise<Message> {
    if (this.failure) { return Promise.reject(this.failure); }
    const requestId = randomUUID();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(requestId);
        reject(new Error("Send not confirmed. Check the chat before retrying."));
      }, this.timeoutMs);
      this.pending.set(requestId, { resolve, reject, timer });
      this.write({ type: "request", requestId, sourceClientId: this.clientId, method, version,
        params, targetClientId, timeoutMs: this.timeoutMs });
    });
  }

  private write(message: Message): void {
    const bytes = Buffer.from(JSON.stringify(message), "utf8");
    const frame = Buffer.alloc(4 + bytes.length);
    frame.writeUInt32LE(bytes.length, 0);
    bytes.copy(frame, 4);
    this.socket.write(frame);
  }
  private fail(error: Error): void {
    this.failure = error;
    for (const pending of this.pending.values()) { clearTimeout(pending.timer); pending.reject(error); }
    this.pending.clear();
  }
  dispose(): void { this.socket.destroy(); this.fail(new Error("Connection closed.")); }
}
