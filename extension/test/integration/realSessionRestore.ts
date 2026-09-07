// Optional smoke test against the installed Codex provider in an isolated profile.
// It only opens existing chats; it never sends a prompt or changes the transcript.
import assert from "node:assert/strict";
import * as vscode from "vscode";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { openSessionSidebar } from "../../src/sessionSidebar";
import { readFocusedCodexSession } from "../../src/codexTurnWatcher";

export async function run(): Promise<void> {
  const session = process.env.LID_GUARD_REAL_SESSION!;
  const codex = vscode.extensions.getExtension("openai.chatgpt");
  assert.ok(codex);
  await codex.activate();
  await vscode.commands.executeCommand("workbench.action.focusWindow");
  const tabs = () => vscode.window.tabGroups.all.flatMap((group) => group.tabs);
  const before = tabs();
  const logs = path.join(process.env.LID_GUARD_RESTORE_RUN!, "profile", "logs");
  const readLog = async () => {
    const runs = (await fs.readdir(logs)).sort().reverse();
    for (const run of runs) {
      for (const window of await fs.readdir(path.join(logs, run))) {
        const log = path.join(logs, run, window, "exthost", "openai.chatgpt", "Codex.log");
        if (await readFocusedCodexSession(log) === session) { return log; }
      }
    }
    return undefined;
  };
  const deadline = Date.now() + 40000;
  assert.ok(await openSessionSidebar(session, async () => (await readLog()) !== undefined));
  assert.deepEqual(tabs(), before, "real Codex must load the chat in its sidebar without creating an editor");
  let selected = false;
  let log: string | undefined;
  while (Date.now() < deadline) {
    log = await readLog();
    if (log) { selected = true; break; }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  assert.ok(selected, "the actual Codex renderer must select the saved session");
  await new Promise((resolve) => setTimeout(resolve, 32000));
  assert.equal(await readFocusedCodexSession(log!), session, "the saved session must stay selected after startup");
  const contents = await fs.readFile(log!, "utf8");
  assert.ok(!contents.includes("renderer_ready_timeout"), "no chat renderer may time out");
  assert.deepEqual(tabs(), before, "no editor tab may appear later during startup");
}
