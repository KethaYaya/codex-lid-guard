import * as vscode from "vscode";
import { codexSessionRoute } from "../../src/sessionNavigation";

// Runs only as the disposable openai.chatgpt test extension. Real-provider smoke
// tests use the installed Codex extension instead of this fixture.
export function createSidebarFixture(delay = 0) {
  // Tests compiled outside the fixture get VS Code's null extension API.
  // URI handlers must use the API owned by the actual development extension.
  const owner: typeof vscode = vscode.extensions.getExtension("openai.chatgpt")!.exports.vscode;
  let view: vscode.WebviewView | undefined;
  let ready = false;
  let selected: string | undefined;
  let pending: string | undefined;
  let resolved = 0;
  const routes: string[] = [];
  const deliver = () => {
    if (ready && view && pending) { void view.webview.postMessage({ session: pending }); }
  };
  const navigate = (id: string) => {
    selected = undefined;
    pending = id;
    routes.push(id);
    deliver();
  };
  const disposables = [
    vscode.window.registerWebviewViewProvider("chatgpt.sidebarSecondaryView", {
      resolveWebviewView: (created) => {
        view = created;
        resolved++;
        ready = false;
        created.webview.options = { enableScripts: true };
        created.webview.onDidReceiveMessage((message) => {
          if (message.ready) { ready = true; deliver(); }
          if (message.session === pending) { selected = message.session; }
        });
        created.webview.html = `<!doctype html><html><body>Chats
          <script>
            const api = acquireVsCodeApi(); let revision = 0;
            window.addEventListener('message', event => {
              const session = event.data.session, current = ++revision;
              setTimeout(() => {
                if (current !== revision) return;
                document.body.textContent = 'Session ' + session;
                api.postMessage({session});
              }, ${delay});
            });
            api.postMessage({ready:true});
          </script></body></html>`;
      }
    }, { webviewOptions: { retainContextWhenHidden: true } }),
    vscode.commands.registerCommand("chatgpt.openSidebar", async () => {
      await vscode.commands.executeCommand("workbench.view.extension.codexSecondaryViewContainer");
      await vscode.commands.executeCommand("chatgpt.sidebarSecondaryView.focus");
    }),
    owner.window.registerUriHandler({ handleUri: async (uri) => {
      const id = uri.path.slice(7);
      if (uri.authority !== "openai.chatgpt" || codexSessionRoute(id) !== uri.path) { return; }
      await vscode.commands.executeCommand("chatgpt.openSidebar");
      navigate(id);
    } })
  ];
  return {
    confirmed: async (id: string) => view?.visible === true && selected === id,
    routes,
    navigate,
    get resolved() { return resolved; },
    dispose: () => disposables.forEach((item) => item.dispose())
  };
}
