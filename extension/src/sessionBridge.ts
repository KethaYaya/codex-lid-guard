import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import * as fs from "node:fs/promises";
import { createServer, type Socket } from "node:net";
import * as path from "node:path";
import { promisify } from "node:util";
import { codexSessionRoute } from "./sessionNavigation";

const execFileAsync = promisify(execFile);

function normalizedPath(value: string): string {
  return path.win32.normalize(value.replace(/^\\\\\?\\/u, ""))
    .replace(/[\\/]+$/u, "").toLowerCase();
}

export function sessionBelongsToWorkspace(cwd: string, roots: readonly string[]): boolean {
  const directory = normalizedPath(cwd);
  return directory.length > 0 && roots.some((root) => {
    const folder = normalizedPath(root);
    return folder.length > 0 && (directory === folder || directory.startsWith(`${folder}\\`));
  });
}

// One connection belongs to one loaded workspace. A random pipe name prevents
// queued clicks from reaching a replacement extension host after Open Folder.
export async function createSessionBridge(options: {
  directory: string;
  roots: () => readonly string[];
  workspaceFile?: () => string | undefined;
  focused: () => boolean;
  open: (sessionId: string) => Promise<void>;
  send?: (sessionId: string, text: string, cwd: string, busy: boolean) => Promise<void>;
  prepareNewChat?: () => Promise<string>;
  reportError: (error: unknown) => void;
}) {
  const pipe = `\\\\.\\pipe\\CodexLidGuard.Navigation.${randomUUID()}`;
  const sockets = new Set<Socket>();
  let disposed = false;
  let window: number | undefined;
  let registration = Promise.resolve();
  const server = createServer({ allowHalfOpen: true }, (socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
    socket.on("error", () => socket.destroy());
    socket.setTimeout(1000, () => socket.destroy());
    let data = "";
    socket.setEncoding("utf8");
    socket.on("data", (chunk: string) => {
      data += chunk;
      if (Buffer.byteLength(data) > 64 * 1024) { socket.destroy(); return; }
      if (!data.includes("\n")) { return; }
      socket.removeAllListeners("data");
      let sessionId: string | undefined;
      let reply: { sessionId: string; text: string; cwd: string; busy: boolean } | undefined;
      let newChat = false;
      try {
        const request = JSON.parse(data.slice(0, data.indexOf("\n")));
        if (!disposed && window !== undefined
            && typeof request.cwd === "string"
            && sessionBelongsToWorkspace(request.cwd, options.roots())) {
          if (request.action === "send" && options.send
              && typeof request.sessionId === "string" && codexSessionRoute(request.sessionId)
              && typeof request.text === "string" && request.text.trim() && request.text.length <= 8192
              && typeof request.busy === "boolean") {
            reply = { sessionId: request.sessionId.toLowerCase(), text: request.text, cwd: request.cwd, busy: request.busy };
          } else if (request.action === "prepare-new-chat" && options.prepareNewChat) {
            newChat = true;
          } else if (request.action === undefined && options.focused()
              && typeof request.sessionId === "string" && codexSessionRoute(request.sessionId)) {
            sessionId = request.sessionId.toLowerCase();
          }
        }
      } catch { /* Invalid local requests never navigate. */ }
      if (newChat) {
        socket.setTimeout(10000, () => socket.destroy());
        void options.prepareNewChat!().then(
          (codexPath) => socket.end(`${JSON.stringify({ accepted: true, codexPath })}\n`),
          (error) => {
            options.reportError(error);
            socket.end(`${JSON.stringify({ accepted: false, error: error instanceof Error ? error.message : "Could not start a new chat." })}\n`);
          }
        );
        return;
      }
      if (reply) {
        socket.setTimeout(100000, () => socket.destroy());
        void options.send!(reply.sessionId, reply.text, reply.cwd, reply.busy).then(
          () => socket.end(`${JSON.stringify({ accepted: true })}\n`),
          (error) => {
            options.reportError(error);
            socket.end(`${JSON.stringify({ accepted: false, error: error instanceof Error ? error.message : "Could not send the message." })}\n`);
          }
        );
        return;
      }
      // This acknowledges delivery only. The guardian still waits for Codex's
      // own view event before clearing the overlay's completion indicator.
      socket.end(`${JSON.stringify({ accepted: sessionId !== undefined })}\n`);
      if (sessionId) { void options.open(sessionId).catch(options.reportError); }
    });
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(pipe, () => { server.removeListener("error", reject); resolve(); });
  });
  server.on("error", options.reportError);

  const publish = (handle: number) => {
    registration = registration.catch(() => undefined).then(async () => {
      if (disposed) { return; }
      await fs.mkdir(options.directory, { recursive: true });
      const destination = path.join(options.directory, `${handle}.json`);
      const temporary = `${destination}.${randomUUID()}.tmp`;
      try {
        await fs.writeFile(temporary, JSON.stringify({
          version: 1, window: handle, pid: process.pid, pipe, workspaceFolders: options.roots(),
          executable: process.execPath, workspaceFile: options.workspaceFile?.()
        }));
        await fs.rename(temporary, destination);
        window = handle;
      } finally { await fs.unlink(temporary).catch(() => undefined); }
    });
    return registration;
  };

  return {
    pipe,
    async bind(helper: string): Promise<void> {
      // Reuse a known handle on multi-root changes, including while unfocused.
      if (window !== undefined) { await publish(window); return; }
      if (!options.focused() || disposed) { return; }
      const { stdout } = await execFileAsync(helper, ["editor-window"], {
        windowsHide: true, timeout: 2000, encoding: "utf8"
      });
      const handle: unknown = JSON.parse(stdout).window;
      if (options.focused() && typeof handle === "number" && Number.isSafeInteger(handle) && handle > 0) {
        await publish(handle);
      }
    },
    // Also used by the isolated VS Code integration test after it observes its
    // own HWND. No user window or guardian needs to be restarted for that test.
    bindWindow: publish,
    dispose() {
      disposed = true;
      server.close();
      sockets.forEach((socket) => socket.destroy());
      // Preserve reopening details after exit. A replacement host atomically
      // publishes its folder here; an empty window keeps existing tabs available.
    }
  };
}
