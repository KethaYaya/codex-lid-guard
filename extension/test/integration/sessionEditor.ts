// Run only in an isolated VS Code Extension Development Host with the fixture
// in test/fixtures/session-editor. These tests never touch a user's editor tabs.
import assert from "node:assert/strict";
import * as vscode from "vscode";
import { handleSessionUri, openSessionEditor } from "../../src/sessionEditor";

const editor = "chatgpt.conversationEditor";
const first = "11111111-1111-1111-1111-111111111111";
const second = "22222222-2222-2222-2222-222222222222";

export async function run(): Promise<void> {
  const provider = vscode.window.registerCustomEditorProvider(editor, {
    openCustomDocument: (uri: vscode.Uri) => ({ uri, dispose() {} }),
    resolveCustomEditor: (_document: vscode.CustomDocument, panel: vscode.WebviewPanel) => {
      panel.webview.html = "<!doctype html><html><body>Session navigation test</body></html>";
    }
  });
  const activeUri = () => {
    const input = vscode.window.tabGroups.activeTabGroup.activeTab?.input;
    assert.ok(input instanceof vscode.TabInputCustom);
    return input.uri.toString();
  };
  const uri = (id: string, query = "") => vscode.Uri.from({
    scheme: "openai-codex", authority: "route", path: `/local/${id}`, query
  });
  try {
    const a = uri(first, "panel=existing");
    const b = uri(second);
    await vscode.commands.executeCommand("vscode.openWith", a, editor,
      { viewColumn: vscode.ViewColumn.One, preview: false });
    await vscode.commands.executeCommand("vscode.openWith", b, editor,
      { viewColumn: vscode.ViewColumn.One, preview: false });
    assert.equal(activeUri(), b.toString());
    assert.ok(await openSessionEditor(first));
    assert.equal(activeUri(), a.toString(), "select the first session, preserving its actual URI");
    assert.ok(await openSessionEditor(second));
    assert.equal(activeUri(), b.toString(), "switch back to the second session in the same window");
    assert.equal(vscode.window.tabGroups.all.flatMap((group) => group.tabs).length, 2,
      "reuse existing tabs without duplicates");

    await vscode.commands.executeCommand("workbench.action.moveEditorToNextGroup");
    assert.equal(vscode.window.tabGroups.activeTabGroup.viewColumn, vscode.ViewColumn.Two);
    assert.ok(await openSessionEditor(first));
    assert.equal(vscode.window.tabGroups.activeTabGroup.viewColumn, vscode.ViewColumn.One);
    assert.ok(await openSessionEditor(second));
    assert.equal(vscode.window.tabGroups.activeTabGroup.viewColumn, vscode.ViewColumn.Two);

    const viewed: string[] = [];
    const link = (path: string) => vscode.Uri.from({
      scheme: vscode.env.uriScheme, authority: "kethayaya.codex-lid-guard", path
    });
    await handleSessionUri(link(`/local/${first}`), (id) => viewed.push(id));
    assert.equal(activeUri(), a.toString());
    assert.deepEqual(viewed, [first], "only the selected session is acknowledged");
    for (const invalid of ["/local/../other", "/local/unknown", `/local/${second}/extra`]) {
      await handleSessionUri(link(invalid), (id) => viewed.push(id));
    }
    assert.equal(await openSessionEditor("https://example.com"), false);
    assert.equal(activeUri(), a.toString());
    assert.deepEqual(viewed, [first], "invalid links do not navigate or acknowledge");

    const third = "33333333-3333-3333-3333-333333333333";
    assert.ok(await openSessionEditor(third));
    assert.equal(activeUri(), uri(third).toString(), "create the exact editor when no tab exists");
    console.log("PASS: exact chat navigation, same-window switching, group/URI reuse, acknowledgement, invalid links, absent tab");
  } finally {
    await vscode.commands.executeCommand("workbench.action.closeAllEditors");
    provider.dispose();
  }
}
