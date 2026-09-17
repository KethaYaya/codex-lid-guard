import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { createServer } from "node:net";
import test from "node:test";
import { sendCodexReply } from "../src/codexReply";

const session = "11111111-1111-1111-1111-111111111111";
const steer = "thread-follower-steer-turn";
const start = "thread-follower-start-turn";
const inactive = `Cannot steer conversation ${session} because its active turn already ended`;

async function withOwner(
  reply: (request: any) => Record<string, unknown> | null,
  run: (pipe: string, requests: any[]) => Promise<void>
): Promise<void> {
  const pipe = `\\\\.\\pipe\\LidGuard.ReplyTest.${randomUUID()}`;
  const requests: any[] = [];
  const server = createServer((socket) => {
    let data: Buffer = Buffer.alloc(0);
    socket.on("error", () => undefined);
    socket.on("data", (chunk: Buffer) => {
      data = Buffer.concat([data, chunk]);
      while (data.length >= 4 && data.length >= 4 + data.readUInt32LE(0)) {
        const length = data.readUInt32LE(0);
        const request = JSON.parse(data.subarray(4, length + 4).toString("utf8"));
        data = data.subarray(length + 4);
        requests.push(request);
        const response = request.method === "initialize" ? { result: { clientId: "test-client" } }
          : request.method === "thread-owner-discovery" ? { result: { supportsUntrustedAppInput: true } }
          : reply(request);
        if (response === null) { socket.destroy(); return; }
        const body = Buffer.from(JSON.stringify({ type: "response", requestId: request.requestId,
          method: request.method, handledByClientId: "exact-owner", resultType: "success", ...response }));
        const bytes = Buffer.alloc(4 + body.length);
        bytes.writeUInt32LE(body.length); body.copy(bytes, 4);
        socket.write(bytes.subarray(0, 3));
        setImmediate(() => socket.write(bytes.subarray(3)));
      }
    });
  });
  await new Promise<void>((resolve) => server.listen(pipe, resolve));
  try { await run(pipe, requests); }
  finally { await new Promise<void>((resolve) => server.close(() => resolve())); }
}

for (const busy of [false, true]) {
  test(`replies use the live owner when the overlay busy flag is ${busy}`, { skip: process.platform !== "win32" }, async () => {
    await withOwner(() => ({ result: { result: { turnId: "active-turn" } } }), async (pipe, requests) => {
      await sendCodexReply(session, "A reply 🌍", "C:\\One", busy, pipe, 1000);
      assert.deepEqual(requests.map((request) => request.method), ["initialize", "thread-owner-discovery", steer]);
      assert.equal(requests[1].params.conversationId, session);
      const sent = requests[2];
      assert.equal(sent.targetClientId, "exact-owner");
      assert.equal(sent.version, 1);
      assert.equal(sent.params.conversationId, session);
      assert.deepEqual(sent.params.input, [{ type: "text", text: "A reply 🌍", text_elements: [] }]);
      assert.equal(sent.params.restoreMessage.context.prompt, "A reply 🌍");
      assert.equal(sent.params.restoreMessage.id, sent.params.clientUserMessageId);
    });
  });

  for (const reason of [inactive, "no active turn to steer"]) {
    test(`a definitively ended turn starts once with inherited settings (${busy}, ${reason})`, {
      skip: process.platform !== "win32"
    }, async () => {
      await withOwner((request) => request.method === steer ? { resultType: "error", error: reason }
        : { result: { result: { turn: { id: "new-turn", status: "inProgress" } } } }, async (pipe, requests) => {
        await sendCodexReply(session, "Next message", "C:\\One", busy, pipe, 1000);
        assert.deepEqual(requests.map((request) => request.method), ["initialize", "thread-owner-discovery", steer, start]);
        assert.equal(requests[3].targetClientId, "exact-owner");
        assert.equal(requests[3].version, 2);
        assert.deepEqual(requests[3].params.turnStart, {
          request: { threadId: session, input: requests[2].params.input,
            clientUserMessageId: requests[2].params.clientUserMessageId },
          context: { inheritThreadSettings: true }
        });
      });
    });
  }
}

for (const failure of ["refused", "request-timeout", "client-disconnected", "no-client-found",
  "Cannot steer conversation different-session because its active turn already ended",
  "Cannot steer conversation " + session + " without an active turn id", "unknown"]) {
  test(`no automatic retry for ${failure}`, { skip: process.platform !== "win32" }, async () => {
    await withOwner(() => failure === "unknown" ? null : { resultType: "error", error: failure }, async (pipe, requests) => {
      await assert.rejects(sendCodexReply(session, "Unconfirmed", "C:\\One", true, pipe, 1000));
      assert.equal(requests.filter((request) => request.method.startsWith("thread-follower-")).length, 1);
    });
  });
}

test("missing confirmation and failed fallback never report sent or retry", { skip: process.platform !== "win32" }, async () => {
  for (const afterInactive of [false, true]) {
    for (const result of [{ result: { result: {} } }, { resultType: "error", error: "refused" }, null]) {
      await withOwner((request) => afterInactive && request.method === steer
        ? { resultType: "error", error: inactive } : result, async (pipe, requests) => {
        await assert.rejects(sendCodexReply(session, "Check delivery", "C:\\One", false, pipe, 1000));
        assert.equal(requests.filter((request) => request.method === steer).length, 1);
        assert.equal(requests.filter((request) => request.method === start).length, afterInactive ? 1 : 0);
      });
    }
  }
  await assert.rejects(sendCodexReply("bad", "Hello", "C:\\One", false), /Enter a message/);
});
