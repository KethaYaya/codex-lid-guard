# Changelog

## 0.1.30

- Used both direct-file and directory notifications for Codex turn starts, improving Windows append reliability.
- Reduced the fallback prompt-detection bound from 250 ms to 100 ms; normal detection remains event-driven.

## 0.1.29

- Reduced worst-case awake-indicator completion lag from two seconds to 250 ms while retaining the low-overhead native reconciliation path.

## 0.1.28

- Closed the final race between validating an active native lifecycle record and creating its completion cursor.
- Kept long, high-output turns detectable even when their start record is beyond the initial transcript tail window.

## 0.1.27

- Activated Lid Guard with Codex views, conversation editors, and launch commands so immediate post-reload prompts are protected without eagerly activating in unrelated VS Code windows.
- Replaced an idle older shared daemon immediately after its final active turn instead of waiting for the four-minute lease refresh.

## 0.1.26

- Prevented completion-time Codex metadata rows and hidden title-generation sessions from creating stale awake-session entries.
- Kept the original terminal-event cursor when native metadata arrives after the fast extension-log signal.
- Promoted still-running new-session turns to durable transcript tracking instead of letting their provisional count expire.

## 0.1.25

- Removed mandatory Codex hook onboarding: native lifecycle metadata now protects turns by default with no console, review popup, or trust step.
- Made lifecycle hooks an explicit optional setting for immediate permission/request alerts and redundant signals; existing Lid Guard hooks are removed automatically when the option is off.
- Promoted the content-free extension-log fallback to a durable transcript-tracked turn, preventing the awake count from expiring before long or automatic work completes.
- Added completion-alert handling to native terminal reconciliation so ordinary done alerts do not require hooks.

## 0.1.24

- Protected automatic continuations and queued turns through Codex's content-free indexed lifecycle metadata, including paths where no `UserPromptSubmit` or extension-log turn-start event is emitted.
- Kept metadata-started turns protected until their normal stop hook or exact terminal lifecycle record, without imposing a timeout on legitimate long tasks.
- Replaced the one-shot directory log subscription with a direct, self-healing file watcher and retained the 250 ms check only as a fallback.
- Replaced the metadata watcher's fixed commit delay with event-driven adaptive retries, eliminating missed early WAL notifications while reducing steady-state detection to roughly 2 ms median.

## 0.1.23

- Warmed the guardian pipe once with a read-only status request during activation, removing the one-time first-prompt initialization penalty.

## 0.1.22

- Sent fast turn-start acquisitions directly to the running local guardian instead of launching a helper process for every prompt.
- Reduced measured warm transport latency from 37 ms to about 8 ms and synthetic watcher-to-guard latency to about 10 ms.
- Preserved the native helper fallback for daemon startup, upgrades, and transient pipe failures.

## 0.1.21

- Made the awake indicator react to newly written turn-start metadata immediately, including before Codex writes the line ending.
- Added a low-cost 250 ms safety check for Windows file notifications that are delayed or coalesced.
- Ignored delayed provisional acquisitions after the authoritative hook has already activated the same session, preventing false idle transitions.

## 0.1.20

- Restyled the active-session popup after Codex's recent-chats panel, without adding a search field.
- Added a compact muted header and rounded neutral selection pills while retaining folder and session titles.

## 0.1.19

- Made the active-session popup subtly translucent while preserving crisp session controls.
- Kept high-contrast themes fully opaque for accessibility.

## 0.1.18

- Replaced the system-framed active-session menu with a notification-style popup aligned to the bottom-right of the active VS Code window.
- Matched dark, light, and high-contrast VS Code themes while removing the bright Windows menu border and cursor-relative offset.
- Kept sessions as one-click, keyboard-navigable, screen-reader-visible buttons inside the popup.

## 0.1.17

- Engaged the guardian directly from Codex's newly appended turn-start metadata, avoiding PowerShell hook startup and occasional cold-hook delays.
- Kept the trusted synchronous `UserPromptSubmit` hook authoritative; it replaces the provisional turn automatically, while an unmatched provisional turn expires after 10 seconds.

## 0.1.16

- Labeled awake-session menu items with each Codex chat title, for example `CodexLidGuard — Fix stale Codex session status`.
- Read titles from Codex's local metadata index only when the menu opens, with the short session ID retained as a fallback.

## 0.1.15

- Custom-drew the anchored awake-session menu with VS Code-style dark notification colors instead of inheriting Windows' light popup-menu theme.
- Added dark session hover selection, muted header text, and a subtle separator while retaining native menu keyboard and dismissal behavior.

## 0.1.14

- Replaced the intermediate status notification with an anchored Windows menu that always lists active Codex sessions as menu items, including when only one session is active.
- Kept VS Code's session picker as a fallback if Windows cannot display the anchored menu.

## 0.1.13

- Restored the native VS Code bottom-right notification when the status-bar item is clicked, with active-session navigation available from the notification.

## 0.1.12

- Made the awake status item list each protected Codex session with its workspace.
- Added one-click switching to the originating editor window and exact Codex chat, with a sidebar fallback when exact navigation is unavailable.

## 0.1.11

- Recovered interrupted or completed turns when Codex misses the `Stop` hook, preventing abandoned turns from keeping the awake-session count permanently elevated.
- Watched only newly appended lifecycle records in the hook-provided local transcript, preserving long-running background turns without relying on an arbitrary timeout.

## 0.1.10

- Detected view-only Codex chat switches from the local extension's session-visibility events, so background-chat alerts no longer require a prompt in the newly selected chat.
- Kept the check event-driven and content-free: Lid Guard reads only the latest active conversation ID when an alert is about to play.

## 0.1.9

- Tracked the most recently messaged Codex chat in each VS Code window.
- Allowed completion and needs-response alerts from another running chat in the same focused window, while keeping the current chat quiet.

## 0.1.8

- Prevented older VS Code windows from replacing a newer shared guardian daemon during extension updates.
- Added versioned client requests and a compatibility response for v0.1.6/v0.1.7 status leases, keeping multi-window protection stable until every window reloads.

## 0.1.7

- Associated each Codex turn with the foreground VS Code window that submitted it.
- Suppressed automatic completion and needs-response sounds while that exact window remains focused; minimized windows, covered windows, and other VS Code sessions still receive alerts.
- Added `codexLidGuard.alertSoundsOnlyWhenUnfocused`, enabled by default. Manual sound tests always play.

## 0.1.6

- Avoided status-file rewrites for read-only requests and unchanged snapshots, reducing background disk and file-watcher activity.
- Engaged Windows' immediate stay-awake state before slower lid-policy work and skipped AC/DC policy writes that were already set to do nothing.
- Recorded exactly which lid settings the guard changed so task completion restores only the values that need restoration.
- Added an idle-only version handoff so an updated helper replaces an older daemon without interrupting active Codex turns.

## 0.1.5

- Replaced PowerShell/WPF alert playback with native Windows multimedia playback scheduled inside the guardian daemon.
- Added atomic, event-driven status snapshots so the status bar no longer rereads the growing log or launches a helper every minute when file watching works.
- Reduced cold daemon startup backoff without shortening the response timeout, preventing duplicate first-acquire retries, and suppressed duplicate alerts fired within 750 milliseconds.
- Made same-protocol daemons compatible across VS Code extension install paths, preventing update and development copies from repeatedly replacing one another.
- Kept the shared daemon outside short-lived hook jobs and renewed its lease every four minutes while VS Code is open, preserving concurrent-session protection without one-minute polling.
- Kept the guardian log open between writes and added 1 MB log rotation.
- Based setup bookkeeping on the hook-definition revision, avoiding redundant prompts when hook commands are unchanged.

## 0.1.4

- Replaced the 35 MB self-contained .NET helper with a sub-megabyte native Rust helper.
- Preserved the existing hook CLI, concurrent-turn tracking, crash recovery, lid notifications, post-task sleep, and Herdr alert sounds.
- Bumped the internal daemon protocol so an older helper process is safely restored and retired during an upgrade.
- Added Herdr's original completion and needs-response sounds.
- Added `PermissionRequest` and targeted `PreToolUse` hooks for immediate approval and structured user-input alerts.
- Added an alert-sound setting and a Command Palette sound test.
- Bundled Herdr's Apache-2.0 license and sound attribution.
- Fixed stale turn IDs after a missed `Stop`, automatically replaces legacy guardian daemons, and removed the heavy WPF payload from routine status checks.

- The trust retry notice is now non-blocking, and its pre-review hash baseline survives retries and reloads so users can operate the Codex terminal before checking again.
- Setup now verifies that Codex persisted new trust hashes for all three Lid Guard hooks before it marks onboarding complete or reloads VS Code.
- Fixed a restart regression where VS Code could lowercase the extension drive letter, changing the exact Codex hook hashes and causing previously trusted hooks to be skipped. Setup now reopens once so the optimized definitions cannot remain silently untrusted.

## 0.1.3

- Fixed hook onboarding for the VS Code Codex experience: the setup flow now opens the interactive Codex CLI, which displays the supported hook-review screen automatically.
- Removed the incorrect instruction to enter `/hooks` in the VS Code chat.
- Made shared VSIX installs portable by locating the Codex CLI inside the recipient's official OpenAI VS Code extension instead of relying on a machine-specific installation path.
- Made the status-bar shield react immediately to guardian acquire/release events; the 15-second poll remains only as a fallback.
- Removed a redundant nested PowerShell launch from Windows hooks, reducing measured helper activation from about 638 ms to about 160 ms.
- Updated the shield directly from the guardian's confirmed-acquire event before running the slower diagnostic status query.

## 0.1.2

- Added a persistent install/update setup flow that opens Codex, copies `/hooks`, and reloads VS Code after the user confirms the required hook review.
- Setup remains pending when dismissed, so the trust step is offered again on the next activation.

## 0.1.1

- Fixed automatic Windows hook invocation with `commandWindows`.
- Added guidance for the required Codex hook trust and reload step.

## 0.1.0

- Initial release with automatic Codex hook integration, Windows lid protection, crash recovery, and post-task sleep.
