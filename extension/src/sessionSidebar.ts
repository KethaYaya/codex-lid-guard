import * as vscode from "vscode";
import { codexSessionRoute } from "./sessionNavigation";

let openRequest = 0;

function sessionFromPath(path: string): string | undefined {
  const id = path.startsWith("/local/") ? path.slice(7) : "";
  return codexSessionRoute(id) === path ? id.toLowerCase() : undefined;
}

export async function openSessionSidebar(
  sessionId: string,
  confirmed: (sessionId: string) => Promise<boolean>
): Promise<boolean> {
  const normalized = sessionId.toLowerCase();
  const route = codexSessionRoute(normalized);
  if (!route) { return false; }
  const request = ++openRequest;
  const roots = () => JSON.stringify(vscode.workspace.workspaceFolders?.map((folder) => folder.uri.toString()) ?? []);
  const originalRoots = roots();
  const current = () => request === openRequest && vscode.window.state.focused && roots() === originalRoots;
  return vscode.window.withProgress({
    location: vscode.ProgressLocation.Window, title: "Opening saved Codex chat…"
  }, async () => {
    if (!current()) { return false; }
    // Codex's URI handler navigates its sidebar and queues the route until its
    // webview is ready. asExternalUri adds VS Code's own windowId, so another
    // project window cannot receive this request if foreground focus changes.
    const uri = await vscode.env.asExternalUri(vscode.Uri.from({
      scheme: vscode.env.uriScheme, authority: "openai.chatgpt", path: route
    }));
    if (!current()) { return false; }
    const started = Date.now();
    let retried = false;
    await vscode.commands.executeCommand("chatgpt.openSidebar");
    if (!current()) { return false; }
    // Without editor options, vscode.open dispatches the URI to its handler.
    // openWith would create a separate conversation editor in the centre.
    await vscode.commands.executeCommand("vscode.open", uri);
    while (Date.now() - started < 35000 && current()) {
      const selected = await confirmed(normalized);
      if (!current()) { return false; }
      if (selected) { return true; }
      if (!retried && Date.now() - started >= 5000) {
        retried = true;
        await vscode.commands.executeCommand("vscode.open", uri);
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    return false;
  });
}

export async function handleSessionUri(
  uri: vscode.Uri,
  viewed: (sessionId: string) => void,
  confirmed: (sessionId: string) => Promise<boolean>
): Promise<void> {
  if (uri.scheme !== vscode.env.uriScheme || uri.authority.toLowerCase() !== "kethayaya.codex-lid-guard"
      || uri.fragment) { return; }
  const sessionId = sessionFromPath(uri.path);
  if (sessionId && await openSessionSidebar(sessionId, confirmed)) { viewed(sessionId); }
}
