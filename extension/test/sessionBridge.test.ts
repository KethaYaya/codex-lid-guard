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

test("overlay replies validate the project and acknowledge delivery without focusing or opening a chat", {
  skip: process.platform !== "win32"
}, async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "lid-guard-reply-"));
  let roots = ["C:\\One"];
  let fail = false;
  const delivered: unknown[] = [];
  const errors: unknown[] = [];
  const opened: string[] = [];
  const bridge = await createSessionBridge({
    directory, roots: () => roots, focused: () => false,
    open: async (id) => { opened.push(id); },
    send: async (...args) => {
      await new Promise((resolve) => setTimeout(resolve, 30));
      if (fail) { throw new Error("Codex unavailable"); }
      delivered.push(args);
    },
    reportError: (error) => errors.push(error)
  });
  const send = (text = "Hello 🌍", cwd = "C:\\One", action = "send", id = sessionId) =>
    request(bridge.pipe, `${JSON.stringify({ action, cwd, text, sessionId: id, busy: false })}\n`);
  try {
    assert.equal(await send(), false);
    await bridge.bindWindow(42);
    assert.equal(await send(), true);
    assert.deepEqual(delivered, [[sessionId, "Hello 🌍", "C:\\One", false]]);
    assert.equal(await send("  "), false);
    assert.equal(await send("a".repeat(8193)), false);
    assert.equal(await send("hello", "C:\\One", "send", "../bad"), false);
    assert.equal(await send("hello", "C:\\One-other"), false);
    assert.equal(await send("hello", "C:\\One", "new-chat"), false);
    roots = ["C:\\Two"];
    assert.equal(await send(), false, "a changed project invalidates a queued reply");
    roots = [];
    assert.equal(await send(), false);
    roots = ["C:\\One"];
    fail = true;
    assert.equal(await send(), false, "failure is reported without losing the draft");
    assert.equal(errors.length, 1);
    assert.equal(delivered.length, 1);
    assert.deepEqual(opened, []);
  } finally {
    bridge.dispose();
    await rm(directory, { recursive: true, force: true });
  }
});

test("new overlay chats prepare the current workspace runtime without navigating or sending a prompt", {
  skip: process.platform !== "win32"
}, async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "lid-guard-new-chat-"));
  let roots = ["C:\\One"];
  let trusted = true;
  let prepared = 0;
  const bridge = await createSessionBridge({ directory, roots: () => roots, focused: () => false,
    open: async () => assert.fail("preparing a chat must not open an editor"),
    send: async () => assert.fail("preparing a chat must not submit a prompt"),
    prepareNewChat: async () => {
      if (!trusted) { throw new Error("Trust this project first"); }
      prepared++; return "C:\\Codex\\codex.exe";
    }, reportError: () => undefined });
  const prepare = (cwd = "C:\\One") => request(bridge.pipe, `${JSON.stringify({ action: "prepare-new-chat", cwd })}\n`);
  try {
    assert.equal(await prepare(), false);
    await bridge.bindWindow(42);
    assert.equal(await prepare(), true);
    assert.equal(prepared, 1);
    assert.equal(await prepare("C:\\One-other"), false);
    roots = ["C:\\Two"];
    assert.equal(await prepare(), false);
    roots = ["C:\\One"]; trusted = false;
    assert.equal(await prepare(), false);
    assert.equal(prepared, 1);
  } finally { bridge.dispose(); await rm(directory, { recursive: true, force: true }); }
});
