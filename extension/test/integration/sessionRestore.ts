import assert from "node:assert/strict";
import * as vscode from "vscode";
import { openSessionSidebar } from "../../src/sessionSidebar";
import { createSidebarFixture } from "./sidebarFixture";

const first = "11111111-1111-1111-1111-111111111111";
const second = "22222222-2222-2222-2222-222222222222";
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export async function run(): Promise<void> {
  const restored = process.env.LID_GUARD_RESTORE_PHASE === "restore";
  const sidebar = createSidebarFixture(restored ? 6500 : 0);
  const tabs = () => vscode.window.tabGroups.all.flatMap((group) => group.tabs);
  await vscode.commands.executeCommand("workbench.action.focusWindow");
  if (!restored) {
    const file = vscode.Uri.joinPath(vscode.workspace.workspaceFolders![0].uri, "keep-open.txt");
    await vscode.workspace.fs.writeFile(file, Buffer.from("Keep this editor open during chat navigation."));
    await vscode.window.showTextDocument(file, { preview: false });
    assert.ok(await openSessionSidebar(first, sidebar.confirmed));
    assert.ok(await openSessionSidebar(second, sidebar.confirmed));
    if (process.env.LID_GUARD_RESTORE_MAXIMIZED) {
      await vscode.commands.executeCommand("workbench.action.maximizeAuxiliaryBar");
    }
    return;
  }
  assert.equal(tabs().length, 1, "restore the file editor saved by the previous process");
  const before = tabs();
  const restore = setTimeout(() => sidebar.navigate(second), 500);
  assert.ok(await openSessionSidebar(first, sidebar.confirmed));
  clearTimeout(restore);
  assert.ok(await sidebar.confirmed(first), "delayed startup must end on the clicked sidebar chat");
  assert.equal(sidebar.resolved, 1, "reuse the sidebar after reopening");
  assert.deepEqual(tabs(), before, "reopening a chat must create no editor tabs");
  const superseded = openSessionSidebar(first, async () => false);
  await pause(200);
  assert.ok(await openSessionSidebar(second, sidebar.confirmed));
  assert.equal(await superseded, false);
  const routeCount = sidebar.routes.length;
  await pause(5200);
  assert.equal(sidebar.routes.length, routeCount, "a superseded request must not navigate again");
  assert.ok(await sidebar.confirmed(second));
  assert.deepEqual(tabs(), before);
}
