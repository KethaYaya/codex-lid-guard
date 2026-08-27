# Changelog

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
