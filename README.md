# Codex Lid Guard

if you use  codex on VSCode, you can make codex work for you while the laptop lid is still closed with this little extension.

## What it does

1. Codex's indexed core lifecycle metadata tells the native guardian that a turn started, including automatic continuations. No lifecycle-hook setup is required.
2. The guardian saves the active Windows power scheme's AC and battery lid actions, temporarily changes both to **Do nothing**, and requests continuous system availability.
3. The guardian follows the turn's lifecycle records and releases it only after completion or cancellation. Multiple simultaneous Codex turns are reference-counted, so protection stays active until the final one stops.
4. The prior AC and battery values are restored exactly. If Windows reports that the lid is still closed, the guardian waits for the configured grace period (10 seconds by default) and requests sleep. A new Codex turn or an opened lid cancels that pending sleep.
5. Herdr's original `done` alert plays when a task stops. Optional Codex hooks can add immediate `request` alerts for permission approvals and structured `request_user_input` prompts. By default, automatic alerts play when the originating VS Code window is minimized or covered, another VS Code window is focused, or another chat is selected in that window. The current chat stays quiet.

Lid Guard watches only indexed lifecycle row IDs and thread IDs from Codex's local metadata database, plus newly appended turn-start and session-visibility metadata in the current window's extension log. It queries rollout paths and working directories for cleanup and display, but never queries prompt or response fields. Both native metadata starts and the extension-log fallback are transcript-tracked until a terminal lifecycle record arrives; long and automatic turns do not expire early.

Optional approval/request alerts use [documented Codex lifecycle hooks](https://learn.chatgpt.com/docs/hooks). Core lid protection does not scrape the Codex UI, guess from background processes, or require hooks.

## Install the shared VSIX

Recipients need Windows 10/11 x64, VS Code, and the official OpenAI Codex extension. The VSIX contains a self-contained Rust helper; Node.js, Rust, and the .NET runtime are not required on recipient machines.

```powershell
code --install-extension .\extension\codex-lid-guard.vsix
```

The extension enables itself on first activation. There is no Codex console, review popup, trust step, or additional setup. Previous Lid Guard hook entries are removed automatically during migration. Users who specifically want immediate permission/request alerts can enable `codexLidGuard.optionalHooks`; Codex then requires the standard one-time review for those optional hooks.

The status-bar shield shows whether the guardian is idle, protecting active turns, or waiting to sleep. While turns are active, click it to open a subtly translucent, theme-aware popup styled after Codex's recent-chats panel, without the search field. Every active session is labeled as `folder — chat title`, with a compact header and rounded selection pill. The popup aligns to the bottom-right of the active VS Code window, follows display scaling, and exposes each session as an accessible one-click button. High-contrast themes remain fully opaque. Selecting a session switches to its originating editor window and opens that chat. Titles are read from Codex's local metadata index only when the popup opens; prompt and response content is not read. These commands are available from the Command Palette:

The shield listens for atomic guardian status snapshots and normally updates in well under a second without launching diagnostic processes. VS Code renews the shared daemon lease once every four minutes, while a 60-second refresh runs only if Windows file watching is unavailable. Alert sounds use native Windows multimedia playback rather than PowerShell or WPF.

- `Codex Lid Guard: Enable`
- `Codex Lid Guard: Disable and Restore Power Settings`
- `Codex Lid Guard: Show Status`
- `Codex Lid Guard: Restore Power Settings Now`
- `Codex Lid Guard: Enable Optional Hook Alerts`
- `Codex Lid Guard: Test Alert Sounds`

Before uninstalling, run the disable command to restore any active Windows power-policy changes and remove optional hook entries if they were enabled.

## Safety and recovery

- No administrator elevation is requested.
- A recovery record is written to `%LOCALAPPDATA%\CodexLidGuard\power-recovery.json` *before* the power scheme changes. If the guardian is interrupted, its next launch restores that record first.
- If optional hooks are enabled, `~/.codex/hooks.json.before-codex-lid-guard` contains the most recent pre-edit hook configuration.
- Closing the lid while no local Codex turn is active follows normal Windows behavior.
- The display may turn off while a task runs. The app prevents system sleep; it does not force the screen to stay lit.
- Only local Codex turns need this protection. Codex cloud tasks already run away from the laptop.
- The extension is Windows-only because lid policy and sleep controls are OS-specific.

## Development

Building from source requires Node.js 20+, Rust through `rustup`, and the Visual Studio C++ build tools.

```powershell
cd extension
cmd /c npm install
cmd /c npm test
cd ..
cargo test --manifest-path .\native\CodexLidGuard\Cargo.toml
cargo build --release --manifest-path .\native\CodexLidGuard\Cargo.toml
```

The native executable supports `status`, `restore`, `sound done`, and `sound request` for diagnostics. Logs are written to `%LOCALAPPDATA%\CodexLidGuard\guard.log`, rotate at 1 MB, and the latest event-driven state is stored in `%LOCALAPPDATA%\CodexLidGuard\status.json`.

The two bundled alert files come from Herdr 0.8.2 under Apache-2.0. See `extension/THIRD_PARTY_NOTICES.md` and the license distributed beside the packaged sounds.
