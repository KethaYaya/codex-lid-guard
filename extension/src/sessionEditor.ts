import * as vscode from "vscode";
import { codexSessionRoute } from "./sessionNavigation";

const conversationEditor = "chatgpt.conversationEditor";

function sessionOf(uri: vscode.Uri): string | undefined {
  if (uri.scheme !== "openai-codex" || uri.authority !== "route") {
    return undefined;
  }
  return sessionFromPath(uri.path);
}

function sessionFromPath(path: string): string | undefined {
  const id = path.startsWith("/local/") ? path.slice(7) : "";
  return codexSessionRoute(id) === path ? id.toLowerCase() : undefined;
}

function tabResource(tab: vscode.Tab | undefined): vscode.Uri | undefined {
  const input = tab?.input;
  return input instanceof vscode.TabInputCustom && input.viewType === conversationEditor
    ? input.uri : undefined;
}

function isSessionActive(sessionId: string): boolean {
  const resource = tabResource(vscode.window.tabGroups.activeTabGroup.activeTab);
  return vscode.window.state.focused && resource !== undefined && sessionOf(resource) === sessionId;
}

function waitForActiveSession(sessionId: string): Promise<boolean> {
  return new Promise((resolve) => {
    const subscriptions: vscode.Disposable[] = [];
    const finish = (opened: boolean) => {
      clearTimeout(timeout);
      subscriptions.forEach((subscription) => subscription.dispose());
      resolve(opened);
    };
    const check = () => { if (isSessionActive(sessionId)) { finish(true); } };
    const timeout = setTimeout(() => finish(false), 1500);
    subscriptions.push(
      vscode.window.tabGroups.onDidChangeTabs(check),
      vscode.window.tabGroups.onDidChangeTabGroups(check),
      vscode.window.onDidChangeWindowState(check)
    );
    check();
  });
}

export async function openSessionEditor(sessionId: string): Promise<boolean> {
  const route = codexSessionRoute(sessionId.toLowerCase());
  if (!route) { return false; }
  const normalized = route.slice(7);
  // Reuse the actual tab URI, including its query, and its editor group.
  // Opening Codex's sidebar URI leaves the previously selected editor unchanged.
  for (const group of vscode.window.tabGroups.all) {
    for (const tab of group.tabs) {
      const resource = tabResource(tab);
      if (resource && sessionOf(resource) === normalized) {
        await vscode.commands.executeCommand("vscode.openWith", resource, conversationEditor, {
          viewColumn: group.viewColumn, preserveFocus: false, preview: false
        });
        return waitForActiveSession(normalized);
      }
    }
  }
  await vscode.commands.executeCommand("vscode.openWith", vscode.Uri.from({
    scheme: "openai-codex", authority: "route", path: route
  }), conversationEditor, { viewColumn: vscode.ViewColumn.Active, preserveFocus: false, preview: false });
  return waitForActiveSession(normalized);
}

export async function handleSessionUri(
  uri: vscode.Uri,
  viewed: (sessionId: string) => void
): Promise<void> {
  if (uri.scheme !== vscode.env.uriScheme || uri.authority.toLowerCase() !== "kethayaya.codex-lid-guard"
      || uri.fragment) { return; }
  const sessionId = sessionFromPath(uri.path);
  if (sessionId && await openSessionEditor(sessionId)) { viewed(sessionId); }
}
