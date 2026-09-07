import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createConnection } from "node:net";
import { tmpdir } from "node:os";
import * as path from "node:path";
import test from "node:test";
import { createSessionBridge, sessionBelongsToWorkspace } from "../src/sessionBridge";

const sessionId = "11111111-1111-1111-1111-111111111111";

test("workspace matching handles case, subfolders, multi-root and sibling boundaries", () => {
  const roots = ["C:\\Projects\\One", "D:\\Two\\"];
  for (const cwd of ["\\\\?\\c:\\projects\\ONE", "C:/Projects/One/src", "D:\\Two"]) {
    assert.ok(sessionBelongsToWorkspace(cwd, roots), cwd);
  }
  for (const cwd of ["C:\\Projects\\One-old", "C:\\Projects\\One\\..\\Other", "", "C:\\Other"]) {
    assert.equal(sessionBelongsToWorkspace(cwd, roots), false, cwd);
  }
  assert.equal(sessionBelongsToWorkspace("C:\\Projects\\One", []), false);
});

function request(pipe: string, payload: string): Promise<boolean> {
  return new Promise((resolve, reject) => {
    const socket = createConnection(pipe);
    socket.setTimeout(2000, () => socket.destroy(new Error("Navigation timed out")));
    socket.on("error", reject);
    let response = "";
    socket.on("data", (chunk) => { response += chunk.toString(); });
    socket.on("end", () => resolve(response ? JSON.parse(response).accepted : false));
    socket.once("connect", () => {
      // Fragment the request across reads to exercise framing.
      socket.write(payload.slice(0, 12));
      setImmediate(() => socket.end(payload.slice(12)));
    });
  });
}

test("navigation opens only a valid session in the current focused workspace", {
  skip: process.platform !== "win32"
}, async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "lid-guard-bridge-test-"));
  let roots = ["C:\\One"];
  let focused = true;
  const opened: string[] = [];
  const errors: unknown[] = [];
  const bridge = await createSessionBridge({
    directory, roots: () => roots, focused: () => focused,
    workspaceFile: () => "C:\\Saved workspace.code-workspace",
    open: async (id) => { opened.push(id); }, reportError: (error) => errors.push(error)
  });
  const send = (id = sessionId, cwd = "C:\\One") =>
    request(bridge.pipe, `${JSON.stringify({ sessionId: id, cwd })}\n`);
  try {
    assert.equal(await send(), false, "unbound bridge cannot open chats");
    await bridge.bindWindow(42);
    assert.equal(await send(sessionId.toUpperCase()), true);
    assert.deepEqual(opened, [sessionId]);
    assert.equal(await send("../bad"), false);
    assert.equal(await request(bridge.pipe, "not JSON\n"), false);
    assert.equal(await send(sessionId, "C:\\One-other"), false);
    focused = false;
    assert.equal(await send(), false);
    focused = true;
    roots = [];
    await bridge.bindWindow(42);
    assert.equal(await send(), false, "an empty window must not open the previous folder's chat directly");
    const empty = JSON.parse(await readFile(path.join(directory, "42.json"), "utf8"));
    assert.deepEqual(empty.workspaceFolders, []);
    assert.equal(empty.executable, process.execPath);
    assert.equal(empty.workspaceFile, "C:\\Saved workspace.code-workspace");
    roots = ["C:\\Two"];
    assert.equal(await send(), false, "Open Folder invalidates a queued click before republishing");
    await bridge.bindWindow(42);
    const context = JSON.parse(await readFile(path.join(directory, "42.json"), "utf8"));
    assert.deepEqual(context.workspaceFolders, ["C:\\Two"]);
    assert.equal(context.pid, process.pid);
    assert.equal(await send(sessionId, "C:\\Two"), true);
    assert.deepEqual(opened, [sessionId, sessionId]);
    assert.deepEqual(errors, []);
  } finally {
    bridge.dispose();
    assert.ok(await readFile(path.join(directory, "42.json")), "exit preserves the last registration");
    await rm(directory, { recursive: true, force: true });
  }
});
