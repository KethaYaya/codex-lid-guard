# Codex Lid Guard

Keep a local Codex task running when a Windows laptop lid is closed, then restore the original power settings and sleep once the task is finished.

Requires Windows 10/11 and the official OpenAI Codex VS Code extension. The packaged VSIX includes its native Rust helper, so recipients do not need Node.js, Rust, or the .NET runtime.

The extension connects to Codex's official `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `Stop`, and `SessionEnd` lifecycle hooks. While one or more local turns are active, its native Windows guardian:

- saves the exact AC and battery lid-close actions;
- temporarily makes lid close do nothing;
- keeps the system awake while allowing the display to turn off;
- restores the saved settings after the final turn; and
- sleeps after a short grace period if the lid remains closed.

It also bundles Herdr's two original alerts: `done` plays when a Codex task completes, and `request` plays for permission approvals and structured `request_user_input` prompts. Use **Codex Lid Guard: Test Alert Sounds** to hear both. The `codexLidGuard.alertSounds` setting disables them without disabling lid protection.

Use the status-bar shield or the `Codex Lid Guard` commands in the Command Palette to inspect, disable, or immediately restore the power settings.

The shield listens for the native guardian's confirmed-acquire event and normally updates in well under a second. A diagnostic status query follows in the background, and a lightweight 60-second periodic refresh remains as a fallback.

After installation or an update, the extension opens a persistent setup flow and launches the interactive Codex CLI in a terminal. Codex displays its supported **Hooks need review** screen automatically. The terminal UI is keyboard-driven: press `T` to trust the five Lid Guard hooks, then use the setup notification to close the terminal and reload VS Code. Clicking the terminal text does not activate it. If you dismiss the flow, the setup prompt returns on the next activation. You can also run **Codex Lid Guard: Finish Codex Hook Setup** from the Command Palette.

The bundled Herdr sound files are distributed under Apache-2.0; see `THIRD_PARTY_NOTICES.md` and the license included beside the packaged sounds.

No administrator rights are requested. A crash-recovery record is saved before any power setting is changed. Before uninstalling, run **Codex Lid Guard: Disable and Restore Power Settings** to remove the installed Codex hooks.

Windows only. Local Codex turns only; cloud tasks do not depend on your laptop staying awake.
