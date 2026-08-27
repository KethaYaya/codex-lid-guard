# Changelog

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
