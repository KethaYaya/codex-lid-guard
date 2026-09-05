# Codex Lid Guard

if you use  codex on VSCode, you can make codex work for you while the laptop lid is still closed with this little extension.

## What it does

1. Codex's indexed core lifecycle metadata tells the native guardian that a turn started, including automatic continuations. No lifecycle-hook setup is required.
2. The guardian saves the active Windows power scheme's AC and battery lid actions, temporarily changes both to **Do nothing**, and requests continuous system availability.
3. The guardian follows the turn's lifecycle records and releases it only after completion or cancellation. Multiple simultaneous Codex turns are reference-counted, so protection stays active until the final one stops.
4. The prior AC and battery values are restored exactly. If Windows reports that the lid is still closed, the guardian waits for the configured grace period (10 seconds by default) and requests sleep. A new Codex turn or an opened lid cancels that pending sleep.
5. Herdr's original `done` alert plays when a task stops. Optional Codex hooks can add immediate `request` alerts for permission approvals and structured `request_user_input` prompts. By default, automatic alerts play when the originating VS Code window is minimized or covered, another VS Code window is focused, or another chat is selected in that window. The current chat stays quiet.

With message previews disabled (the default), Lid Guard watches only indexed lifecycle row IDs and thread IDs from Codex's local metadata database, plus newly appended turn-start and session-visibility metadata in the current window's extension log. It queries rollout paths and working directories for cleanup and display, but never queries prompt or response fields. Both native metadata starts and the extension-log fallback are transcript-tracked until a terminal lifecycle record arrives; long and automatic turns do not expire early.

Optional approval/request alerts use [documented Codex lifecycle hooks](https://learn.chatgpt.com/docs/hooks). Core lid protection does not scrape the Codex UI, guess from background processes, or require hooks.


## Message overlay

Run **Codex Lid Guard: Toggle Message Overlay** to enable translucent message previews when another app or chat is in focus, including when VS Code is minimized or covered. A chat's overlay hides only while that exact chat is visible in its focused VS Code window; other chats keep their tabs. The panel stays on top without taking focus when messages arrive. When VS Code is minimized or loses focus, the active message briefly shrinks toward its edge tab and finishes minimized. Native Windows events start that motion from the system event timestamp, using the cached chat without waiting for the background reader. Delayed events join the motion in progress instead of replaying it afterward. Already tucked tabs stay in place. Right-side panels stay flush with the display edge throughout expansion and restoration, with no gap beside the tab. Hover or click a tab to read its message. It slides out in one continuous motion at its normal text size, with the right edge attached throughout; another focus-loss transition tucks expanded previews again. Messages fade and ease into place when cards arrive or expire. Transitions take about 180 ms, pause during a pending click, and follow the Windows animation setting. The three most recently active eligible chats each get a separate panel showing their latest assistant update. Single-click a message to slide only that chat's panel into the right edge of its display, leaving a small tab. Each chat keeps its own expanded or tucked state, and fixed display lanes prevent the panels from overlapping or shifting each other. Hover over the minimized tab or click it to slide that chat back in; the tab retracts and fades smoothly as the panel settles. New updates stay tucked away until you reopen it; each tab shows a short abbreviation of its chat title. The latest updates for the three most recent chats stay cached so their tabs can return when you switch away. Older cached previews pause expiry while tucked away. Double-click a message to restore and maximize its originating VS Code window and select that exact chat editor, including when several chats share the same project. Existing chat tabs reopen in their original editor group. Reload existing VS Code windows once after installing version 0.1.68 to load this navigation handler. Opening a chat briefly grows and fades the exact visible tab or message as VS Code begins restoring. The 180 ms transition preserves its appearance and proportions and updates its position, size, shape, and transparency together, avoiding a jump into a stretched message card. The animation lets clicks pass through to the editor, then hides only that chat's overlay after confirming the requested session is active. Failed opens restore the notification and its controls. Switching away restores its minimized tab. Message clicks give immediate pressed feedback and wait only for the Windows double-click interval before tucking the panel away. A panel opened by hovering slides back into its tab about 200 ms after the pointer leaves. Moving from the tab onto the message, or returning during that short delay, keeps it open. Panels opened by a click retain their manual controls. Hovering never opens VS Code or clears a completion dot. Hover does not trigger while a mouse button is held or the panel is still docking. The tab reopens from a cached painted message without text layout on the hover path. File reads run in the background, and an input-aware high-resolution timer paces motion at about 60 frames per second without polling while idle. Sliding to and from the tab takes about 240 ms and follows the Windows animation setting. Restoring VS Code hides only the chat you are actually viewing; switching to another editor tab or hiding the Codex panel leaves the chat overlays available. Up to three chats appear, labeled as `project folder - chat title` using Codex's session index. Labels refresh when a title is created or renamed; chats without a title show `Untitled chat`. Long messages are shortened; restore VS Code to read the full reply. Older cached previews expire after 90 seconds by default; the latest updates for the three most recent chats and unread completions remain available. Closed or unidentified editor windows have no overlay.

Each tucked-away tab shows moving amber dots while its own chat is busy. When a session completes, a small green dot pulses in the expanded panel header and beside the completed message title until you double-click to open that chat or view it manually in its focused VS Code window. Completed tabs use a simple, uniform background fade between charcoal and yellow (`#FFD000`) every 1.8 seconds. The usual open chevron and off-white initials stay steady and readable throughout the fade; the background color is the tab's completion cue. The repeating yellow color makes finished tasks easy to spot while using another app. Opening a different chat or unfolding the tab leaves its completion indicators on. The indicators belong to their own chat, so a completed chat can pulse while another tab shows busy dots. A fourth eligible chat replaces the least recently active chat; this does not mark its completion as viewed. Unread completions stay available until viewed; starting a new turn clears that session's previous completion. These indicators follow the Windows animation setting, using steady colors when animations are disabled. Their paint-only timer updates the small indicators and completed tab surface, reuses a native drawing buffer, and stops when hidden or idle.

While overlay tabs are visible, the Copilot key acts as a shortcut prefix. For a tab labeled `DR`, hold **Copilot** and press **D** to expand it, then **R** to open that chat in VS Code. Opening uses the same behavior as double-click: the relevant overlay hides after a successful open and returns as a tab when you switch away. Keyboard expansion stays open like a tab click. For keyboards that emit and release the Copilot key immediately, press the letters within 1.5 seconds of each step. **Escape** cancels an unfinished shortcut. Ordinary `D` and `R` typing does not trigger these actions without the prefix. Tabs receive distinct first letters when titles overlap, and the expanded footer shows their shortcut. Codes stay stable while the tab is visible. With no visible tabs, Copilot keeps its normal behavior. This uses the standard Copilot key sequence (Win+Shift+F23); a key remapped by Windows or another utility must still send that sequence.

Use **Codex Lid Guard: Preview Message Overlay** for a 35-second sample with three independent chats: watch the messages shrink into busy tabs, then hover to expand them and move away to tuck them again. Completion dots appear after 15, 18, and 21 seconds. The sample closes automatically and does not open a real chat. Settings include `codexLidGuard.messageOverlay`, `codexLidGuard.overlayOpacity` (30?100%), `codexLidGuard.overlayPosition` (any display corner), and `codexLidGuard.overlayDurationSeconds` (10?600). The panel uses the originating editor's display. Sessions without an identified editor window are omitted.

This optional feature reads newly appended assistant display messages from the active local session files. It ignores user prompts, reasoning, and tool output; duplicate transcript records are suppressed. Message text remains in memory and is never added to guardian logs or status snapshots. Enabling it starts with new messages, without replaying old history. Disabling it clears the previews. These local file formats may change with Codex updates; messages appear after Codex saves them, rather than token by token.

## Install the shared VSIX

Recipients need Windows 10/11 x64, VS Code, and the official OpenAI Codex extension. The VSIX contains a self-contained Rust helper; Node.js, Rust, and the .NET runtime are not required on recipient machines.

```powershell
code --install-extension .\extension\codex-lid-guard.vsix
```

The extension enables itself on first activation. There is no Codex console, review popup, trust step, or additional setup. Previous Lid Guard hook entries are removed automatically during migration. Users who specifically want immediate permission/request alerts can enable `codexLidGuard.optionalHooks`; Codex then requires the standard one-time review for those optional hooks.

The status-bar shield shows whether the guardian is idle, protecting active turns, or waiting to sleep. Click it at any time to open a subtly translucent, theme-aware popup styled after Codex's recent-chats panel, without the search field. The menu keeps all awake sessions and fills the menu to five entries with the most recently active chats, so completed work remains switchable; its count always reflects awake sessions only. Every session is labeled as `folder — chat title`, with a compact header and rounded selection pill. The session displayed in the current VS Code window is highlighted when the menu opens, every running session retains a separate theme-aware active color, and completed sessions stay blue until their chat is viewed. The popup aligns to the bottom-right of the active VS Code window, follows display scaling, and exposes each session as an accessible one-click button. High-contrast themes remain fully opaque. Selecting an awake session switches to its originating editor window before opening the chat, while completed sessions open directly by their Codex route. Session IDs, folders, and titles are read from Codex's local metadata when the popup opens or an enabled overlay begins tracking a session; prompt and response fields are never queried. These commands are available from the Command Palette:

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
