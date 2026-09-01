import * as os from "node:os";
import * as path from "node:path";
import { randomUUID } from "node:crypto";
import { watch, type FSWatcher } from "node:fs";
import * as fs from "node:fs/promises";
import * as vscode from "vscode";
import {
  quotePowerShellLiteral,
  readHooksDocument,
  withGuardHooks,
  withoutGuardHooks,
  writeHooksDocument
} from "./hookInstaller";
import {
  focusHelperSession,
  GuardStatus,
  preAcquireHelper,
  readHelperStatus,
  runHelper,
  showHelperMenu,
  type GuardActiveItem,
  writeHelperSettings
} from "./helper";
import {
  allGuardTrustHashesChanged,
  readGuardTrustHashes,
  setupStateMatchesRevision,
  type GuardTrustHashes
} from "./trustVerifier";
import {
  awakeSessionDisplay,
  awakeSessionMenuLabel,
  codexSessionRoute
} from "./sessionNavigation";
import {
  codexSessionTitle,
  readCodexSessionTitles
} from "./sessionIndex";
import {
  codexLogPathForExtensionLog,
  type CodexTurnStartWatcher,
  watchCodexTurnStarts
} from "./codexTurnWatcher";

let refreshTimer: NodeJS.Timeout | undefined;
let daemonLeaseTimer: NodeJS.Timeout | undefined;
let statusWatcher: FSWatcher | undefined;
let statusRefreshDebounce: NodeJS.Timeout | undefined;
let codexTurnStartWatcher: CodexTurnStartWatcher | undefined;
let updatingEnabledSetting = false;
let setupPromptOpen = false;
const setupVersionStateKey = "hookSetupVersion";
const setupTrustBaselineStateKey = "hookTrustBaseline";
const hookSetupRevision = "herdr-alert-sounds-5";

type StoredTrustBaseline = {
  revision: string;
  hashes: GuardTrustHashes;
};

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 90);
  statusBar.command = "codexLidGuard.showStatus";
  statusBar.name = "Codex Lid Guard";
  context.subscriptions.push(statusBar);

  context.subscriptions.push(
    vscode.commands.registerCommand("codexLidGuard.enable", () => enable(context, statusBar, true)),
    vscode.commands.registerCommand("codexLidGuard.disable", () => disable(context, statusBar)),
    vscode.commands.registerCommand("codexLidGuard.showStatus", () => showStatus(context, statusBar)),
    vscode.commands.registerCommand("codexLidGuard.restorePowerSettings", () => restorePowerSettings(context, statusBar)),
    vscode.commands.registerCommand("codexLidGuard.finishSetup", () => finishSetup(context)),
    vscode.commands.registerCommand("codexLidGuard.testAlertSounds", () => testAlertSounds(context)),
    vscode.workspace.onDidChangeConfiguration(async (event) => {
      if (!event.affectsConfiguration("codexLidGuard") || updatingEnabledSetting) {
        return;
      }
      await syncSettings();
      if (configuration().get<boolean>("enabled", true)) {
        await enable(context, statusBar, false);
      } else {
        await disable(context, statusBar, false);
      }
    })
  );

  if (process.platform !== "win32") {
    setUnavailable(statusBar, "Codex Lid Guard supports Windows laptops only.");
    return;
  }

  await syncSettings();
  startStatusWatcher(context, statusBar);
  context.subscriptions.push({
    dispose: () => {
      stopStatusWatcher();
      stopCodexTurnStartWatcher();
      stopFallbackRefresh();
      stopDaemonLease();
    }
  });
  if (configuration().get<boolean>("enabled", true)) {
    await enable(context, statusBar, false);
  } else {
    setDisabled(statusBar);
  }
  await startCodexTurnStartWatcher(context, statusBar);
  startDaemonLease(context, statusBar);

}

export function deactivate(): void {
  stopDaemonLease();
  stopFallbackRefresh();
  stopStatusWatcher();
  stopCodexTurnStartWatcher();
  // Do not stop the native guardian here: VS Code may close while Codex is still
  // finishing a local turn, and that is exactly when protection matters most.
}

async function enable(
  context: vscode.ExtensionContext,
  statusBar: vscode.StatusBarItem,
  notify: boolean
): Promise<void> {
  try {
    ensureWindows();
    const helper = helperPath(context);
    await fs.access(helper);
    await syncSettings();
    const hooksPath = codexHooksPath();
    const document = await readHooksDocument(hooksPath);
    const installed = withGuardHooks(document, helper);
    const hooksChanged = JSON.stringify(installed) !== JSON.stringify(document);
    if (hooksChanged) {
      await writeHooksDocument(hooksPath, installed);
    }
    await setEnabledSetting(true);
    await refreshStatus(context, statusBar);

    const storedSetupVersion = context.globalState.get<string>(setupVersionStateKey);
    const setupRequired = hooksChanged || !setupStateMatchesRevision(storedSetupVersion, hookSetupRevision);
    if (!setupRequired && storedSetupVersion !== setupStateVersion()) {
      await context.globalState.update(setupVersionStateKey, setupStateVersion());
    }
    if (setupRequired) {
      void promptToFinishSetup(context);
    } else if (notify) {
      const codex = vscode.extensions.getExtension("openai.chatgpt");
      const suffix = codex
        ? "New Codex turns are now protected."
        : "Hooks are ready; install or enable the OpenAI Codex extension to use them.";
      void vscode.window.showInformationMessage(`Codex Lid Guard enabled. ${suffix}`);
    }
  } catch (error) {
    setError(statusBar, messageOf(error));
    if (notify) {
      void vscode.window.showErrorMessage(`Could not enable Codex Lid Guard: ${messageOf(error)}`);
    }
  }
}

async function promptToFinishSetup(context: vscode.ExtensionContext): Promise<void> {
  if (setupPromptOpen) {
    return;
  }

  setupPromptOpen = true;
  try {
    const choice = await vscode.window.showWarningMessage(
      "Codex Lid Guard installed its task hooks. Codex requires you to review them once before automatic protection can start.",
      { modal: true },
      "Review Hooks",
      "Not Now"
    );
    if (choice !== "Review Hooks") {
      return;
    }

    const cliPath = codexCliPath(context);
    try {
      await fs.access(cliPath);
    } catch {
      void vscode.window.showErrorMessage(
        "Codex Lid Guard could not find the Codex CLI bundled with the OpenAI Codex extension. Update or reinstall the OpenAI Codex extension, then run Finish Codex Hook Setup again."
      );
      return;
    }

    const setupRevision = setupStateVersion();
    let storedBaseline = context.globalState.get<StoredTrustBaseline>(setupTrustBaselineStateKey);
    if (!storedBaseline || storedBaseline.revision !== setupRevision) {
      storedBaseline = {
        revision: setupRevision,
        hashes: await readGuardTrustHashes(codexConfigPath(), codexHooksPath())
      };
      await context.globalState.update(setupTrustBaselineStateKey, storedBaseline);
    }
    const trustBefore = storedBaseline.hashes;
    const terminal = vscode.window.createTerminal({
      name: "Codex Lid Guard Hook Review",
      shellPath: "powershell.exe",
      shellArgs: ["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass"],
      cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || os.homedir()
    });
    terminal.show();
    terminal.sendText(`& ${quotePowerShellLiteral(cliPath)} --no-alt-screen`, true);

    while (true) {
      const completed = await vscode.window.showInformationMessage(
        "In the Codex terminal, use the arrow keys to select “Trust all and continue”, then press Enter. Wait for Codex to open, then verify here.",
        "Verify Trust & Reload",
        "Later"
      );
      if (completed !== "Verify Trust & Reload") {
        return;
      }

      if (await waitForGuardTrustChange(trustBefore)) {
        terminal.dispose();
        await context.globalState.update(setupVersionStateKey, setupStateVersion());
        await context.globalState.update(setupTrustBaselineStateKey, undefined);
        await vscode.commands.executeCommand("workbench.action.reloadWindow");
        return;
      }

      const retry = await vscode.window.showWarningMessage(
        "Codex Lid Guard is still untrusted. Switch to the Codex terminal, open /hooks if needed, then press T to trust all five pending hooks. Return here afterward.",
        "Check Again",
        "Later"
      );
      if (retry !== "Check Again") {
        return;
      }
    }
  } finally {
    setupPromptOpen = false;
  }
}

async function waitForGuardTrustChange(before: GuardTrustHashes): Promise<boolean> {
  for (let attempt = 0; attempt < 12; attempt += 1) {
    const after = await readGuardTrustHashes(codexConfigPath(), codexHooksPath());
    if (allGuardTrustHashesChanged(before, after)) {
      return true;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  return false;
}

async function finishSetup(context: vscode.ExtensionContext): Promise<void> {
  await promptToFinishSetup(context);
}

async function disable(
  context: vscode.ExtensionContext,
  statusBar: vscode.StatusBarItem,
  notify = true
): Promise<void> {
  try {
    ensureWindows();
    const hooksPath = codexHooksPath();
    const document = await readHooksDocument(hooksPath);
    const removed = withoutGuardHooks(document);
    if (JSON.stringify(removed) !== JSON.stringify(document)) {
      await writeHooksDocument(hooksPath, removed);
    }
    await runHelper(helperPath(context), "restore");
    if (notify) {
      await setEnabledSetting(false);
      void vscode.window.showInformationMessage("Codex Lid Guard disabled and the saved Windows power settings were restored.");
    }
    setDisabled(statusBar);
  } catch (error) {
    setError(statusBar, messageOf(error));
    if (notify) {
      void vscode.window.showErrorMessage(`Could not disable Codex Lid Guard: ${messageOf(error)}`);
    }
  }
}

async function restorePowerSettings(context: vscode.ExtensionContext, statusBar: vscode.StatusBarItem): Promise<void> {
  try {
    const status = await runHelper(helperPath(context), "restore");
    updateStatusBar(statusBar, status);
    void vscode.window.showInformationMessage(status.message);
  } catch (error) {
    setError(statusBar, messageOf(error));
    void vscode.window.showErrorMessage(`Could not restore the Windows power settings: ${messageOf(error)}`);
  }
}

async function testAlertSounds(context: vscode.ExtensionContext): Promise<void> {
  try {
    ensureWindows();
    if (!configuration().get<boolean>("alertSounds", true)) {
      void vscode.window.showInformationMessage("Codex Lid Guard alert sounds are disabled in Settings.");
      return;
    }
    await runHelper(helperPath(context), "sound-done");
    await new Promise((resolve) => setTimeout(resolve, 1200));
    await runHelper(helperPath(context), "sound-request");
    void vscode.window.showInformationMessage("Played the completion alert, then the needs-response alert.");
  } catch (error) {
    void vscode.window.showErrorMessage(`Could not play Codex Lid Guard alert sounds: ${messageOf(error)}`);
  }
}

async function showStatus(context: vscode.ExtensionContext, statusBar: vscode.StatusBarItem): Promise<void> {
  if (!configuration().get<boolean>("enabled", true)) {
    const choice = await vscode.window.showInformationMessage(
      "Codex Lid Guard is disabled.",
      "Enable"
    );
    if (choice === "Enable") {
      await enable(context, statusBar, true);
    }
    return;
  }

  try {
    const status = await refreshStatus(context, statusBar);
    const lid = status.lidState === "unknown" ? "lid state not reported yet" : `lid ${status.lidState}`;
    if (status.activeTurns > 0) {
      await showAwakeSessions(context, status);
      return;
    }
    const activity = status.sleepPending ? "sleep pending" : "idle";
    void vscode.window.showInformationMessage(`Codex Lid Guard: ${activity}; ${lid}.`);
  } catch (error) {
    setError(statusBar, messageOf(error));
    void vscode.window.showErrorMessage(`Could not read Codex Lid Guard status: ${messageOf(error)}`);
  }
}

type AwakeSessionQuickPickItem = vscode.QuickPickItem & {
  activeItem: GuardActiveItem;
};

async function showAwakeSessions(
  context: vscode.ExtensionContext,
  status: GuardStatus
): Promise<void> {
  const activeItems = status.activeItems ?? [];
  if (activeItems.length === 0) {
    await openCodexSidebar();
    return;
  }

  const sessionTitles = await readCodexSessionTitles(
    codexSessionIndexPath(),
    activeItems.map((item) => item.sessionId)
  ).catch(() => new Map<string, string>());

  try {
    const selectedIndex = await showHelperMenu(
      helperPath(context),
      `Codex awake · ${status.activeTurns}`,
      activeItems.map((item) => awakeSessionMenuLabel(
        item,
        codexSessionTitle(sessionTitles, item.sessionId)
      ))
    );
    if (selectedIndex !== undefined) {
      await openAwakeSession(context, activeItems[selectedIndex]);
    }
    return;
  } catch {
    // Fall back to VS Code's Quick Pick if the native popup is unavailable.
  }

  const quickPickItems: AwakeSessionQuickPickItem[] = activeItems.map((activeItem) => ({
    ...awakeSessionDisplay(
      activeItem,
      codexSessionTitle(sessionTitles, activeItem.sessionId)
    ),
    activeItem
  }));
  const selected = await vscode.window.showQuickPick(quickPickItems, {
    title: `Codex awake · ${status.activeTurns}`,
    placeHolder: "Select an awake Codex session to open",
    matchOnDescription: true,
    matchOnDetail: true
  });
  if (selected) {
    await openAwakeSession(context, selected.activeItem);
  }
}

async function openAwakeSession(
  context: vscode.ExtensionContext,
  activeItem: GuardActiveItem
): Promise<void> {
  try {
    await focusHelperSession(
      helperPath(context),
      activeItem.sessionId,
      activeItem.turnId
    );
    await new Promise((resolve) => setTimeout(resolve, 100));
  } catch {
    // The session route can still open in the current window when the original
    // editor window has closed or Windows declines the focus request.
  }

  const route = codexSessionRoute(activeItem.sessionId);
  if (route) {
    try {
      const opened = await vscode.env.openExternal(vscode.Uri.from({
        scheme: "vscode",
        authority: "openai.chatgpt",
        path: route
      }));
      if (opened) {
        return;
      }
    } catch {
      // Fall through to Codex's supported public sidebar command.
    }
  }
  await openCodexSidebar();
}

async function openCodexSidebar(): Promise<void> {
  try {
    await vscode.commands.executeCommand("chatgpt.openSidebar");
  } catch (error) {
    void vscode.window.showErrorMessage(`Could not open Codex: ${messageOf(error)}`);
  }
}

async function refreshStatus(
  context: vscode.ExtensionContext,
  statusBar: vscode.StatusBarItem
): Promise<GuardStatus> {
  const status = await runHelper(helperPath(context), "status");
  updateStatusBar(statusBar, status);
  return status;
}

function startStatusWatcher(context: vscode.ExtensionContext, statusBar: vscode.StatusBarItem): void {
  stopStatusWatcher();
  stopFallbackRefresh();
  try {
    statusWatcher = watch(path.dirname(helperSettingsPath()), { persistent: false }, (_event, filename) => {
      const changedFile = filename?.toString().toLowerCase();
      if (changedFile && changedFile !== "status.json") {
        return;
      }
      if (statusRefreshDebounce) {
        clearTimeout(statusRefreshDebounce);
      }
      statusRefreshDebounce = setTimeout(() => {
        statusRefreshDebounce = undefined;
        if (configuration().get<boolean>("enabled", true)) {
          void readHelperStatus(helperStatusPath())
            .then((status) => updateStatusBar(statusBar, status))
            .catch(() => refreshStatus(context, statusBar).catch((error) => setError(statusBar, messageOf(error))));
        }
      }, 25);
    });
    statusWatcher.on("error", () => {
      stopStatusWatcher();
      startFallbackRefresh(context, statusBar);
    });
  } catch {
    startFallbackRefresh(context, statusBar);
  }
}

function startFallbackRefresh(context: vscode.ExtensionContext, statusBar: vscode.StatusBarItem): void {
  if (refreshTimer) {
    return;
  }
  refreshTimer = setInterval(() => {
    if (configuration().get<boolean>("enabled", true)) {
      void refreshStatus(context, statusBar).catch((error) => setError(statusBar, messageOf(error)));
    }
  }, 60_000);
}

function stopFallbackRefresh(): void {
  if (refreshTimer) {
    clearInterval(refreshTimer);
    refreshTimer = undefined;
  }
}

function startDaemonLease(context: vscode.ExtensionContext, statusBar: vscode.StatusBarItem): void {
  stopDaemonLease();
  daemonLeaseTimer = setInterval(() => {
    if (configuration().get<boolean>("enabled", true)) {
      void refreshStatus(context, statusBar).catch((error) => setError(statusBar, messageOf(error)));
    }
  }, 4 * 60_000);
  daemonLeaseTimer.unref();
}

function stopDaemonLease(): void {
  if (daemonLeaseTimer) {
    clearInterval(daemonLeaseTimer);
    daemonLeaseTimer = undefined;
  }
}

function stopStatusWatcher(): void {
  if (statusRefreshDebounce) {
    clearTimeout(statusRefreshDebounce);
    statusRefreshDebounce = undefined;
  }
  if (statusWatcher) {
    statusWatcher.close();
    statusWatcher = undefined;
  }
}

async function startCodexTurnStartWatcher(
  context: vscode.ExtensionContext,
  statusBar: vscode.StatusBarItem
): Promise<void> {
  stopCodexTurnStartWatcher();
  const codexLogPath = codexLogPathForExtensionLog(context.logUri.fsPath);
  codexTurnStartWatcher = await watchCodexTurnStarts(codexLogPath, (sessionId) => {
    if (!configuration().get<boolean>("enabled", true)) {
      return;
    }
    const pendingTurnId = `pending-${randomUUID()}`;
    const cwd = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    void preAcquireHelper(helperPath(context), sessionId, pendingTurnId, cwd)
      .then((status) => updateStatusBar(statusBar, status))
      .catch(() => {
        // The trusted UserPromptSubmit hook remains the authoritative fallback.
      });
  });
}

function stopCodexTurnStartWatcher(): void {
  codexTurnStartWatcher?.dispose();
  codexTurnStartWatcher = undefined;
}

function updateStatusBar(statusBar: vscode.StatusBarItem, status: GuardStatus): void {
  if (!status.ok) {
    setError(statusBar, status.message);
    return;
  }
  if (status.isGuarding) {
    statusBar.text = `$(shield) Codex awake · ${status.activeTurns}`;
    statusBar.tooltip = `Windows is being kept awake for ${status.activeTurns} active Codex turn(s). Lid: ${status.lidState}.`;
    statusBar.backgroundColor = new vscode.ThemeColor("statusBarItem.warningBackground");
  } else if (status.sleepPending) {
    statusBar.text = "$(clock) Codex sleep pending";
    statusBar.tooltip = "The task finished with the lid closed. Windows sleep is pending.";
    statusBar.backgroundColor = undefined;
  } else {
    statusBar.text = "$(shield) Codex Lid Guard";
    statusBar.tooltip = `Ready. Lid: ${status.lidState}.`;
    statusBar.backgroundColor = undefined;
  }
  statusBar.show();
}

function setDisabled(statusBar: vscode.StatusBarItem): void {
  statusBar.text = "$(circle-slash) Codex Lid Guard";
  statusBar.tooltip = "Disabled. Click to enable.";
  statusBar.backgroundColor = undefined;
  statusBar.show();
}

function setUnavailable(statusBar: vscode.StatusBarItem, message: string): void {
  statusBar.text = "$(warning) Codex Lid Guard";
  statusBar.tooltip = message;
  statusBar.backgroundColor = new vscode.ThemeColor("statusBarItem.warningBackground");
  statusBar.show();
}

function setError(statusBar: vscode.StatusBarItem, message: string): void {
  statusBar.text = "$(error) Codex Lid Guard";
  statusBar.tooltip = message;
  statusBar.backgroundColor = new vscode.ThemeColor("statusBarItem.errorBackground");
  statusBar.show();
}

async function syncSettings(): Promise<void> {
  const config = configuration();
  await writeHelperSettings(
    helperSettingsPath(),
    config.get<boolean>("alertSounds", true),
    config.get<boolean>("alertSoundsOnlyWhenUnfocused", true),
    config.get<boolean>("sleepWhenLidClosed", true),
    config.get<number>("sleepDelaySeconds", 10)
  );
}

function configuration(): vscode.WorkspaceConfiguration {
  return vscode.workspace.getConfiguration("codexLidGuard");
}

function helperPath(context: vscode.ExtensionContext): string {
  return path.join(context.extensionPath, "bin", "win-x64", "CodexLidGuard.exe");
}

function setupStateVersion(): string {
  return hookSetupRevision;
}

function codexCliPath(context: vscode.ExtensionContext): string {
  const codex = vscode.extensions.getExtension("openai.chatgpt");
  if (!codex) {
    throw new Error("The OpenAI Codex extension is not installed or enabled.");
  }
  const platformDirectory = process.arch === "arm64" ? "windows-aarch64" : "windows-x86_64";
  return path.join(codex.extensionPath, "bin", platformDirectory, "codex.exe");
}

function helperSettingsPath(): string {
  const localAppData = process.env.LOCALAPPDATA;
  if (!localAppData) {
    throw new Error("LOCALAPPDATA is not available.");
  }
  return path.join(localAppData, "CodexLidGuard", "settings.json");
}

function helperStatusPath(): string {
  const localAppData = process.env.LOCALAPPDATA;
  if (!localAppData) {
    throw new Error("LOCALAPPDATA is not available.");
  }
  return path.join(localAppData, "CodexLidGuard", "status.json");
}

function codexHooksPath(): string {
  return path.join(codexHomePath(), "hooks.json");
}

function codexConfigPath(): string {
  return path.join(codexHomePath(), "config.toml");
}

function codexSessionIndexPath(): string {
  return path.join(codexHomePath(), "session_index.jsonl");
}

function codexHomePath(): string {
  return process.env.CODEX_HOME?.trim() || path.join(os.homedir(), ".codex");
}

function ensureWindows(): void {
  if (process.platform !== "win32") {
    throw new Error("Codex Lid Guard supports Windows laptops only.");
  }
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function setEnabledSetting(enabled: boolean): Promise<void> {
  if (configuration().get<boolean>("enabled", true) === enabled) {
    return;
  }
  updatingEnabledSetting = true;
  try {
    await configuration().update("enabled", enabled, vscode.ConfigurationTarget.Global);
  } finally {
    updatingEnabledSetting = false;
  }
}
