# Codex Lid Guard

Keep a local Codex task running when a Windows laptop lid is closed, then restore the original power settings and sleep once the task is finished.

Requires Windows 10/11 and the official OpenAI Codex VS Code extension. The packaged VSIX includes its native Rust helper, so recipients do not need Node.js, Rust, or the .NET runtime.

The extension uses Codex's content-free indexed lifecycle metadata by default. While one or more local turns are active, its native Windows guardian:

- saves the exact AC and battery lid-close actions;
- temporarily makes lid close do nothing;
- keeps the system awake while allowing the display to turn off;
- restores the saved settings after the final turn; and
- sleeps after a short grace period if the lid remains closed.

The guardian recognizes terminal lifecycle records directly, repairing the counter without imposing a timeout on legitimate long-running or automatic tasks.

It also bundles Herdr's two original alerts: `done` plays when a Codex task completes. Users can optionally enable Codex hooks to add immediate `request` alerts for permission approvals and structured `request_user_input` prompts. Automatic alerts play when the originating VS Code window is minimized or no longer in the foreground, or when another chat is selected in the same window. The current chat stays quiet. Use **Codex Lid Guard: Test Alert Sounds** to hear both at any time.

Lid Guard watches only indexed lifecycle row IDs and thread IDs from Codex's local metadata database, plus newly appended turn-start and session-visibility metadata in the current window's extension log. It queries rollout paths and working directories for cleanup and display, but never queries prompt or response fields. Native metadata starts and the extension-log fallback remain transcript-tracked until the task actually completes.

While Codex is awake, click the status-bar shield to open a subtly translucent, theme-aware popup styled after Codex's recent-chats panel, without the search field. Every active session is labeled as `folder — chat title`—even when there is only one—with a compact header and rounded selection pill. The popup aligns to the bottom-right of the active VS Code window, follows display scaling, and exposes each session as an accessible one-click button. High-contrast themes remain fully opaque. Titles are read from Codex's local metadata index only when the popup opens; prompt and response content is not read. Selecting one switches to its originating editor window and opens that Codex chat. If Windows cannot display the anchored popup, Lid Guard falls back to VS Code's session picker. If exact-chat navigation is unavailable after a Codex extension update, it still opens the Codex sidebar. The `Codex Lid Guard` commands in the Command Palette can also inspect, disable, or immediately restore the power settings.

The shield listens for small atomic status snapshots written only when guardian state changes, so it normally updates in well under a second without launching diagnostic processes. VS Code renews the shared daemon lease once every four minutes so concurrent sessions keep one persistent guardian; a 60-second refresh runs only if Windows file watching is unavailable.

Alerts use native Windows multimedia playback inside the guardian. No PowerShell or WPF process is started for routine sound playback.

Installation and updates require no Codex console, review popup, trust step, or additional setup. Existing Lid Guard hooks are removed automatically while `codexLidGuard.optionalHooks` is off. Enable **Codex Lid Guard: Enable Optional Hook Alerts** only if you want immediate permission/request alerts; Codex requires its standard one-time trust review for those optional hooks.

The bundled Herdr sound files are distributed under Apache-2.0; see `THIRD_PARTY_NOTICES.md` and the license included beside the packaged sounds.

No administrator rights are requested. A crash-recovery record is saved before any power setting is changed. Before uninstalling, run **Codex Lid Guard: Disable and Restore Power Settings** to restore active power changes and remove optional hooks if enabled.

Windows only. Local Codex turns only; cloud tasks do not depend on your laptop staying awake.
