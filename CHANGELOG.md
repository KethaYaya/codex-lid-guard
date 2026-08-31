# Changelog

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
