# Codex Lid Guard

if you use  codex on VSCode, you can make codex work for you while the laptop lid is still closed with this little extension.

## What it does

1. A `UserPromptSubmit` Codex lifecycle hook tells the native guardian that a turn started.
2. The guardian saves the active Windows power scheme's AC and battery lid actions, temporarily changes both to **Do nothing**, and requests continuous system availability.
3. `Stop` releases that turn. Multiple simultaneous Codex turns are reference-counted, so protection stays active until the final one stops.
4. The prior AC and battery values are restored exactly. If Windows reports that the lid is still closed, the guardian waits for the configured grace period (10 seconds by default) and requests sleep. A new Codex turn or an opened lid cancels that pending sleep.
5. `SessionEnd` provides cleanup if a conversation closes without a normal stop event.
6. Herdr's original `done` alert plays when a task stops. Its `request` alert plays for permission approvals and structured `request_user_input` prompts. Sounds are enabled by default and can be disabled in VS Code settings.

This uses [documented Codex lifecycle hooks](https://learn.chatgpt.com/docs/hooks). It does not scrape the Codex UI or guess based on background processes.

## Install the shared VSIX

Recipients need Windows 10/11 x64, VS Code, and the official OpenAI Codex extension. The VSIX contains a self-contained Rust helper; Node.js, Rust, and the .NET runtime are not required on recipient machines.

```powershell
code --install-extension .\extension\codex-lid-guard.vsix
```

The extension enables itself on first activation and merges its hook groups into `~/.codex/hooks.json`. Existing hook groups and unknown JSON fields are preserved. On installation or update, a persistent setup flow launches the interactive Codex CLI, where Codex automatically displays its **Hooks need review** screen. This terminal UI is keyboard-driven: press `T` to trust the five hooks; clicking the text does not activate it. The extension then closes the terminal and reloads VS Code. Codex's trust requirement cannot be silently bypassed by a VS Code extension.

The status-bar shield shows whether the guardian is idle, protecting active turns, or waiting to sleep. These commands are available from the Command Palette:

The shield listens for atomic guardian status snapshots and normally updates in well under a second without launching diagnostic processes. VS Code renews the shared daemon lease once every four minutes, while a 60-second refresh runs only if Windows file watching is unavailable. Alert sounds use native Windows multimedia playback rather than PowerShell or WPF.

- `Codex Lid Guard: Enable`
- `Codex Lid Guard: Disable and Restore Power Settings`
- `Codex Lid Guard: Show Status`
- `Codex Lid Guard: Restore Power Settings Now`
- `Codex Lid Guard: Test Alert Sounds`

Before uninstalling, run the disable command so its lifecycle hook entries are removed.

## Safety and recovery

- No administrator elevation is requested.
- A recovery record is written to `%LOCALAPPDATA%\CodexLidGuard\power-recovery.json` *before* the power scheme changes. If the guardian is interrupted, its next launch restores that record first.
- `~/.codex/hooks.json.before-codex-lid-guard` contains the most recent pre-edit hook configuration.
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
