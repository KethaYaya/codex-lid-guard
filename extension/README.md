# Codex Lid Guard

Keep a local Codex task running when a Windows laptop lid is closed, then restore the original power settings and sleep once the task is finished.

Requires Windows 10/11 and the official OpenAI Codex VS Code extension. The packaged VSIX includes its native Rust helper, so recipients do not need Node.js, Rust, or the .NET runtime.

The extension connects to Codex's official `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `Stop`, and `SessionEnd` lifecycle hooks. While one or more local turns are active, its native Windows guardian:

- saves the exact AC and battery lid-close actions;
- temporarily makes lid close do nothing;
- keeps the system awake while allowing the display to turn off;
- restores the saved settings after the final turn; and
- sleeps after a short grace period if the lid remains closed.

If Codex interrupts a turn without running its normal `Stop` hook, the guardian also recognizes the exact terminal lifecycle record in the hook-provided local transcript. This repairs the counter without imposing a timeout on legitimate long-running tasks.

It also bundles Herdr's two original alerts: `done` plays when a Codex task completes, and `request` plays for permission approvals and structured `request_user_input` prompts. Automatic alerts play when the originating VS Code window is minimized or no longer in the foreground, or when another chat is selected in the same window. The current chat stays quiet. Use **Codex Lid Guard: Test Alert Sounds** to hear both at any time. The `codexLidGuard.alertSoundsOnlyWhenUnfocused` setting controls the focus filter, while `codexLidGuard.alertSounds` disables sounds without disabling lid protection.

Lid Guard watches only newly appended turn-start and session-visibility metadata in the current window's local Codex extension log; it never reads prompt or response content. A turn-start event engages the already-running guardian directly, avoiding Windows shell startup, and the trusted `UserPromptSubmit` hook then supplies the authoritative turn details. If that hook never arrives, the provisional guard expires after 10 seconds.

While Codex is awake, click the status-bar shield to open a dark, VS Code-style menu directly above it with every active session labeled as `folder — chat title`—even when there is only one. Titles are read from Codex's local metadata index only when the menu opens; prompt and response content is not read. Selecting one switches to its originating editor window and opens that Codex chat. If Windows cannot display the anchored menu, Lid Guard falls back to VS Code's session picker. If exact-chat navigation is unavailable after a Codex extension update, it still opens the Codex sidebar. The `Codex Lid Guard` commands in the Command Palette can also inspect, disable, or immediately restore the power settings.

The shield listens for small atomic status snapshots written only when guardian state changes, so it normally updates in well under a second without launching diagnostic processes. VS Code renews the shared daemon lease once every four minutes so concurrent sessions keep one persistent guardian; a 60-second refresh runs only if Windows file watching is unavailable.

Alerts use native Windows multimedia playback inside the guardian. No PowerShell or WPF process is started for routine sound playback.

After installation or an update, the extension opens a persistent setup flow and launches the interactive Codex CLI in a terminal. Codex displays its supported **Hooks need review** screen automatically. The terminal UI is keyboard-driven: press `T` to trust the five Lid Guard hooks, then use the setup notification to close the terminal and reload VS Code. Clicking the terminal text does not activate it. If you dismiss the flow, the setup prompt returns on the next activation. You can also run **Codex Lid Guard: Finish Codex Hook Setup** from the Command Palette.

The bundled Herdr sound files are distributed under Apache-2.0; see `THIRD_PARTY_NOTICES.md` and the license included beside the packaged sounds.

No administrator rights are requested. A crash-recovery record is saved before any power setting is changed. Before uninstalling, run **Codex Lid Guard: Disable and Restore Power Settings** to remove the installed Codex hooks.

Windows only. Local Codex turns only; cloud tasks do not depend on your laptop staying awake.
