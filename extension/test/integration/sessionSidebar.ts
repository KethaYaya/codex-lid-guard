import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createConnection } from "node:net";
import * as path from "node:path";
import { promisify } from "node:util";
import * as vscode from "vscode";
import { handleSessionUri, openSessionSidebar } from "../../src/sessionSidebar";
import { createSessionBridge } from "../../src/sessionBridge";
import { createSidebarFixture } from "./sidebarFixture";

const first = "11111111-1111-1111-1111-111111111111";
const second = "22222222-2222-2222-2222-222222222222";
const editor = "chatgpt.conversationEditor";

export async function run(): Promise<void> {
  await vscode.extensions.getExtension("openai.chatgpt")!.activate();
  const sidebar = createSidebarFixture();
  const provider = vscode.window.registerCustomEditorProvider(editor, {
    openCustomDocument: (uri: vscode.Uri) => ({ uri, dispose() {} }),
    resolveCustomEditor: (_document: vscode.CustomDocument, panel: vscode.WebviewPanel) => {
      panel.webview.html = "<!doctype html><html><body>Existing centre chat editor</body></html>";
    }
  });
  const tabs = () => vscode.window.tabGroups.all.flatMap((group) => group.tabs);
  try {
    // Reproduce the screenshot: a matching chat is already in the editor area.
    const existing = vscode.Uri.from({ scheme: "openai-codex", authority: "route", path: `/local/${first}` });
    await vscode.commands.executeCommand("vscode.openWith", existing, editor, { preview: false });
    const before = tabs();
    const active = vscode.window.tabGroups.activeTabGroup.activeTab;
    await vscode.commands.executeCommand("workbench.action.focusWindow");
    for (const id of [first, second, first]) {
      assert.ok(await openSessionSidebar(id, sidebar.confirmed));
      assert.ok(await sidebar.confirmed(id), "the requested chat must load in the visible right sidebar");
      assert.deepEqual(tabs(), before, "opening a sidebar chat must not add or replace editor tabs");
      assert.equal(vscode.window.tabGroups.activeTabGroup.activeTab, active);
    }
    assert.equal(sidebar.resolved, 1, "switching sessions reuses one sidebar webview");
    const viewed: string[] = [];
    const link = (route: string) => vscode.Uri.from({ scheme: vscode.env.uriScheme, authority: "kethayaya.codex-lid-guard", path: route });
    await handleSessionUri(link(`/local/${second}`), (id) => viewed.push(id), sidebar.confirmed);
    assert.deepEqual(viewed, [second]);
    const count = sidebar.routes.length;
    for (const invalid of ["/local/../other", "/local/unknown", `/local/${first}/extra`]) {
      await handleSessionUri(link(invalid), (id) => viewed.push(id), sidebar.confirmed);
    }
    assert.equal(await openSessionSidebar("https://example.com", sidebar.confirmed), false);
    assert.equal(sidebar.routes.length, count, "invalid requests do not navigate");
    {
      const helper = process.env.LID_GUARD_TEST_HELPER;
      const exec = promisify(execFile);
      let roots = vscode.workspace.workspaceFolders!.map((folder) => folder.uri.fsPath);
      const cwd = roots[0];
      const errors: unknown[] = [];
      const bridge = await createSessionBridge({
        directory: path.join(process.env.LOCALAPPDATA!, "CodexLidGuard", "windows"),
        roots: () => roots, focused: () => vscode.window.state.focused,
        open: async (id) => { assert.ok(await openSessionSidebar(id, sidebar.confirmed)); },
        reportError: (error) => errors.push(error)
      });
      const send = async (id: string): Promise<boolean> => {
        if (helper) {
          return JSON.parse((await exec(helper, ["open-editor-session", id, cwd], {
            windowsHide: true, timeout: 5000
          })).stdout).accepted;
        }
        return new Promise((resolve, reject) => {
          const socket = createConnection(bridge.pipe);
          let data = "";
          socket.setTimeout(2000, () => socket.destroy(new Error("Bridge request timed out")));
          socket.on("error", reject);
          socket.on("connect", () => socket.write(`${JSON.stringify({ sessionId: id, cwd })}\n`));
          socket.on("data", (chunk) => { data += chunk.toString(); });
          socket.on("end", () => resolve(JSON.parse(data).accepted));
        });
      };
      try {
        if (helper) {
          await vscode.commands.executeCommand("workbench.action.focusWindow");
          await new Promise((resolve) => setTimeout(resolve, 250));
          await bridge.bind(helper);
        } else {
          // Hermetic bridge test: no native foreground input is needed.
          await bridge.bindWindow(42);
        }
        assert.equal(await send(first), true);
        const deadline = Date.now() + 5000;
        while (!await sidebar.confirmed(first) && Date.now() < deadline) {
          await new Promise((resolve) => setTimeout(resolve, 25));
        }
        assert.ok(await sidebar.confirmed(first));
        roots = [path.join(cwd, "replacement")];
        assert.equal(await send(second), false);
        if (helper) { await bridge.bind(helper); } else { await bridge.bindWindow(42); }
        assert.equal(await send(second), false);
        assert.deepEqual(errors, []);
        assert.deepEqual(tabs(), before);
        console.log("PASS: navigation pipe to existing sidebar, session content, stale workspace rejection, no editor tabs created");
      } finally { bridge.dispose(); }
    }
    console.log("PASS: sidebar chat navigation, existing centre tab preserved, same sidebar reused, invalid links rejected");
  } finally {
    await vscode.commands.executeCommand("workbench.action.closeAllEditors");
    sidebar.dispose();
    provider.dispose();
  }
}
