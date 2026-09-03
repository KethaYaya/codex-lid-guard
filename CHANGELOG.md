# Changelog

## 0.1.34

- Gave every currently running session a persistent theme-aware accent in the session menu.
- Kept focused running sessions visually distinct with a brighter active accent, while completed focused sessions retain the normal selection color.

## 0.1.33

- Highlighted the menu row for the Codex session currently displayed in the VS Code window that opened the Lid Guard menu.
- Preserved the normal first-row keyboard focus when the current chat is not present in the five-session menu.

## 0.1.32

- Kept the five most recently active Codex sessions in the Lid Guard menu so completed chats remain available for quick switching.
- Continued to count only currently awake turns, while prioritizing all awake sessions ahead of completed history entries.
- Loaded recent-session history only for interactive status-menu requests, leaving prompt acquisition and background monitoring performance unchanged.

## 0.1.31

- Bound fast-path Codex turn acquisitions to their originating VS Code window, so selecting an awake session switches to the correct workspace before opening the chat.
- Made prompt-time window associations authoritative over later metadata-watcher guesses, including when a direct pipe request succeeds but its response times out.

## 0.1.30

- Combined direct Codex-log and containing-directory notifications on Windows, avoiding delayed prompt acquisition when either watcher misses an append.
- Tightened the low-frequency turn-start safety check from 250 ms to 100 ms while retaining event-driven acquisition as the primary path.

## 0.1.29

- Reduced worst-case terminal reconciliation latency from two seconds to 250 ms so the awake indicator clears almost immediately after Codex finishes.

## 0.1.28

- Made native lifecycle validation and transcript cursor creation atomic, closing the final narrow completion race while eliminating a duplicate transcript read.
- Located the latest lifecycle state beyond the initial transcript tail window when a running turn produces unusually large output.

## 0.1.27

- Activated Lid Guard when a Codex view, conversation editor, or launch command is used, so a prompt sent immediately after reload cannot outrun turn tracking without adding work to unrelated VS Code windows.
- Automatically handed an idle older shared daemon over to the installed version as soon as the active turn finished, eliminating persistent old-daemon sessions after upgrades.

## 0.1.26

- Prevented completion-time Codex metadata rows and hidden title-generation sessions from creating stale awake-session entries.
- Preserved the extension log's existing transcript cursor when delayed native metadata arrives, so an already-observed terminal event cannot be skipped.
- Promoted still-running provisional turns to transcript tracking when their session metadata becomes available, preventing new-session turns from expiring after 10 seconds.

## 0.1.25

- Removed mandatory Codex hook onboarding: native lifecycle metadata now protects turns by default with no console, review popup, or trust step.
- Made lifecycle hooks an explicit optional setting for immediate permission/request alerts and redundant signals; existing Lid Guard hooks are removed automatically when the option is off.
- Promoted the content-free extension-log fallback to a durable transcript-tracked turn, preventing the awake count from expiring before long or automatic work completes.
- Added completion-alert handling to native terminal reconciliation so ordinary done alerts do not require hooks.

## 0.1.24

- Added a single event-driven native watcher for Codex's indexed lifecycle metadata, engaging the guard for automatic continuations and queued turns that do not emit the extension's normal turn-start line.
- Read only lifecycle row IDs, thread IDs, rollout paths, and working directories; prompt and response fields remain unqueried.
- Made the extension-log watcher direct and self-healing after log replacement or watcher errors, while retaining its low-frequency safety check.
- Replaced the metadata watcher's fixed commit delay with event-driven adaptive retries, eliminating missed early WAL notifications while reducing steady-state detection to roughly 2 ms median.

## 0.1.23

- Warmed the read-only local guardian connection once at activation so the first protected prompt uses the same low-latency path as later prompts.

## 0.1.22

- Removed per-prompt helper-process startup from the fast path by sending provisional acquisitions directly to the already-running per-user guardian pipe.
- Retained the native helper as an automatic fallback when the guardian is unavailable or restarting.

## 0.1.21

- Bounded fast turn-start detection to 250 ms when Windows coalesces a Codex log notification, and handled complete metadata before the trailing newline arrives.
- Prevented a delayed provisional signal from replacing and later expiring an already-active authoritative Codex turn.

## 0.1.20

- Restyled the active-session popup after Codex's recent-chats panel, with a compact header and rounded selection pills.

## 0.1.19

- Added subtle translucency to the active-session popup, with fully opaque high-contrast themes.

## 0.1.18

- Replaced the system-framed active-session menu with a DPI-aware, notification-style popup aligned above VS Code's status bar.
- Added theme-aware colors and accessible session buttons without changing one-click session navigation.

## 0.1.17

- Engaged the guardian directly from Codex's newly appended turn-start metadata, avoiding PowerShell hook startup and occasional cold-hook delays.
- Kept the trusted synchronous `UserPromptSubmit` hook authoritative; it replaces the provisional turn automatically, while an unmatched provisional turn expires after 10 seconds.

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

- Moved alert playback from PowerShell/WPF to native Windows multimedia APIs and scheduled sounds inside the Rust guardian.
- Replaced log-driven status refreshes with small atomic status snapshots and made periodic polling a watcher-failure fallback only.
- Improved cold-start retry latency without shortening response timeouts, plus same-protocol daemon handoff, duplicate-alert suppression, and log rotation.
- Added Windows job-breakaway startup and a four-minute VS Code daemon lease so short-lived hook runners cannot end protection for other sessions.
- Based setup bookkeeping on the hook-definition revision so unchanged hook commands do not trigger redundant setup prompts.

## 0.1.4

- Rewrote the native Windows guardian in Rust for faster hook startup and a much smaller VSIX while preserving power-policy recovery and concurrent-turn behavior.

- The trust retry notice is now non-blocking, and its pre-review hash baseline survives retries and reloads so users can operate the Codex terminal before checking again.
- Setup now verifies that Codex persisted new trust hashes for all three Lid Guard hooks before it marks onboarding complete or reloads VS Code.
- Fixed a restart regression where VS Code could lowercase the extension drive letter, changing the exact Codex hook hashes and causing previously trusted hooks to be skipped. Setup now reopens once so the optimized definitions cannot remain silently untrusted.

## 0.1.1

- Fixed Codex hook execution on Windows by using the documented `commandWindows` override and an explicit non-interactive PowerShell invocation.
- Added a setup command and notification for Codex's required hook review and window reload.

## 0.1.0

- Initial Windows VS Code extension.
- Automatic Codex lifecycle-hook integration.
- Concurrent-turn tracking and lid-state detection.
- Exact AC/DC lid-policy restoration and crash recovery.
- Closed-lid sleep with a configurable cancellation grace period.
