# Changelog

## 0.1.59

- Make hover and click expansion one continuous slide, presenting the panel position, visible size, shape, and pixels together. Keep the message at its original size and avoid a separate fill or resize after sliding.
- Keep the drawer's right edge flat against the display, including its top and bottom corners, while retaining rounded corners on the exposed side.
- Reuse the compositor after expansion so settling does not switch back to a differently sized painted surface. Reuse buffers and update only indicator pixels for busy/completion activity.
- Preserve hover-out, interrupted slides, click/double-click, Copilot shortcuts, completion acknowledgement, reduced motion, and the editor-opening transition.

## 0.1.58

- Keep right-side previews and their restore transitions flush with the display edge, removing the gap when a tab expands.
- Preserve the exact visible tab, panel, and rounded edges when opening a chat, including a hover transition already in progress. Enlarge proportionally instead of substituting a stretched message card.
- Begin the 180 ms grow-and-fade when the activation worker is about to restore VS Code, so thread startup does not consume the animation and duplicate notifications cannot restart it.
- Present position, size, shape, and opacity in one layered-window update, with preallocated image buffers and no text layout or bitmap allocation per frame.
- Preserve click-through, independent tabs, failure recovery, stale-frame suppression, reduced motion, and the native minimize-event path.

## 0.1.57

- Expand and fade the selected overlay as its chat opens in a maximized VS Code window, using a 260 ms transition from the current tab or panel bounds.
- Restore and route the editor on a worker with its own Windows message queue so activation cannot stall overlay frames.
- Make the opening animation click-through and suspend that tab's shortcuts immediately; hide it after successful completion and recover controls if opening fails.
- Preserve independent tabs, stale-frame suppression, reduced-motion preferences, and the minimize-event timing improvements.

## 0.1.56

- Start overlay docking from native Windows minimize and focus events, using the event timestamp so delivery delays do not restart the animation late.
- Keep the three latest chat frames cached while the focused chat is hidden. Reveal cached content immediately and notify native windows when the reader publishes an update.
- Coalesce minimize and foreground changes, preserve reopened hover panels when the reader catches up, and restore minimized tabs when switching chats in the same editor.
- Respect Windows minimize-animation and client-area animation preferences, preserve no-focus-stealing behavior, and remove the extra 250 ms UI polling delay.

## 0.1.55

- Add keyboard control for visible chat tabs: hold Copilot and press the first displayed letter to expand, then the second letter to open the matching chat in VS Code.
- Support the standard Win+Shift+F23 Copilot key and keyboards that release its macro immediately, with a short prefix timeout and Escape cancellation. Ordinary typing is unaffected without the Copilot prefix.
- Give visible tabs distinct first letters, keep codes stable while a tab is visible, and show the shortcut in the expanded message footer.
- Process shortcuts on a dedicated input thread without message reads, text logging, or window activation in the hook. Bind queued actions to the exact current chat and ignore stale targets and key repeats.
- Test native shortcut expansion/opening/hiding, failed/stale routes, duplicate title prefixes, key releases, timeout, cancellation, focus changes, and native hook registration.

## 0.1.54

- Hide only the relevant overlay immediately after successfully opening its chat by double-click, without waiting for VS Code view-log updates.
- Prevent stale cached frames from redisplaying the opened chat. Keep other chat tabs available, restore the selected tab on a later focus loss, and leave the overlay visible if opening fails.
- Track actual view-event revisions so stale or unrelated log updates cannot undo an explicit open; resume normal visibility when a new chat view is observed.
- Cover successful and failed native opens, delayed view data, independent tabs, and returning to a minimized tab with regression tests.

## 0.1.53

- Start newly backgrounded chat overlays as small edge tabs, with a brief cached-message shrink animation when Windows animations are enabled.
- Dock expanded previews again when their originating editor loses focus; keep steady background tabs and hover previews independent until the next focus change.
- Replace coarse animation ticks with an input-aware, high-resolution frame timer that stops when motion finishes. Reuse painted message surfaces while sliding and shrinking, and keep hover input off the text-layout path.
- Preserve hover-out, click/double-click, exact-chat focus hiding, independent three-chat lanes, busy indicators, completion dots, and reduced-motion behavior.

## 0.1.52

- Show chat overlays when another app or chat is in focus, including when VS Code is covered or minimized. Hide only the exact chat visible in its focused originating editor window.
- Preserve other chat tabs when restoring VS Code or selecting a different chat. Hidden or unknown chat views do not dismiss notifications.
- Keep the latest updates for the three most recent chats available after viewing, so switching away restores their tabs without restarting acknowledged completion pulses.
- Cache chat-view metadata, share each log read across sessions, and skip view-log discovery while another app has focus. Preserve the latest view event during long-running log appends.
- Show a stopped message after cancellation and cover focus, chat switching, cache retention, log truncation and stale-log handling with regression tests.

## 0.1.51

- Tuck hover-opened chat panels back into their tabs when the pointer leaves, with a 200 ms grace period and the existing smooth slide.
- Keep the original tab and the gap to the panel inside the hover area to prevent flicker while opening or moving onto a message. Returning during the grace period cancels dismissal.
- Preserve explicit click behavior, per-chat completion indicators and original tab positions. Stop cursor checks when the hover preview closes.
- Verify native hover-out, re-entry, independent panels, and timer cleanup alongside existing overlay controls and animations.

## 0.1.50

- Expand an individual minimized chat tab when the pointer moves over it, using the existing smooth slide and cached messages.
- Keep the panel open after hover, preserve click controls and completion indicators, and avoid opening tabs during dragging or while they are still docking.
- Verify native hover response, independent panels, focus preservation, and click behavior.

## 0.1.49

- Give the three most recently active chats independent overlay panels and edge tabs, each showing its latest update.
- Keep each chat's busy animation, completion pulse, click target, and collapsed state independent. A fourth chat replaces the oldest eligible chat without moving the others.
- Reserve separate display lanes, shorten text to fit smaller displays, and label tucked tabs with a short chat-title abbreviation.
- Share one background reader between the three windows, retain cached input handling and small-indicator repainting, and pause expiry only for the tucked chat.
- Update the 35-second preview to demonstrate three chats completing independently; add multi-window interaction, recency, isolation, and display-scaling checks.

## 0.1.48

- Repaint only the small activity indicators during pulses instead of redrawing all notification text and backgrounds.
- Preserve queued native replies when closing a pipe instance, fixing a race that discarded responses before clients could read them.
- Close timed-out guardian pipe connections, bound reply sizes, and assemble fragmented replies once while preserving Unicode.
- Let the 35-second overlay preview finish when launched from VS Code by allowing 45 seconds for its helper process.
- Clear previous-turn previews when new work starts so an old completed result cannot be shown as the new task's result.
- Add timeout, fragmented-response, invalid-response, stale-result, and expanded-pulse regression coverage, plus a repeatable Windows CPU/memory/resource benchmark.

## 0.1.47

- Show the pulsing completion dot in the expanded notification header as well as the notch and completed message titles. Keep it visible when reopening the notch until the chat is viewed.

## 0.1.46

- Replace the completion border glow with a small green dot above the notch arrow and beside completed message titles.
- Pulse only the dot brightness over 2.8 seconds, preserve its size and position, and keep busy dots and the card count visible.

## 0.1.45

- Update the overlay preview to demonstrate busy dots for 15 seconds followed by a completion glow, with 35 seconds to try tucking and reopening the panel. The preview runs independently of the live guardian.

## 0.1.44

- Pulse a soft green glow on completed message cards and the tucked-away tab until that chat is opened from the overlay or viewed in its focused VS Code window.
- Animate amber dots on the tab while a minimized session is busy, including quiet periods before its first update. Show busy and unread completion indicators together when different sessions need them.
- Keep unread completion cards beyond normal preview expiry, prioritize them among visible cards, and clear their glow independently by session. Cancellation does not count as completion.
- Reuse the native paint buffer and run a separate 30 fps paint-only timer for activity indicators; stop it while hidden or idle and use steady indicators when Windows animations are disabled.

## 0.1.43

- Move overlay transcript, metadata, and settings reads to a background worker so file access cannot delay mouse input or animation.
- Use a dedicated click deadline timer to remove the extra wait for the next 250 ms message refresh while preserving Windows double-click timing.
- Show immediate pressed feedback on message cards and the reopen tab; keep only the latest pending frame and stop the reader when the overlay closes.

## 0.1.42

- Fixed the end-of-slide jitter when reopening the message overlay by keeping the native window bounds and paint origin stable as the tab disappears.
- Retract and fade the tab smoothly, keep its glyphs stationary while clipping, and repaint resized frames without reusing stale pixels.
- Keep the invisible tab gutter outside the clickable window region.

## 0.1.41

- Single-click an overlay message to slide the panel into the right screen edge, leaving a small clickable tab; click the tab to slide the messages back in.
- Keep previews available while tucked away, accept new updates without expanding the panel, and resume message expiry after reopening.
- Keep the tab stationary as messages arrive, clip sliding content to its display, and preserve double-click navigation and Windows reduced-motion preferences.

## 0.1.40

- Added gentle fade-and-slide transitions for the message overlay and animated card arrival, dismissal, and reflow.
- Buffer animation frames to prevent flicker, pause motion during pending clicks, and follow the Windows animation preference.
- Run animation ticks only while a transition is active; message polling retains its existing cadence.

## 0.1.39

- Fixed overlay labels to use project folder and chat title from the Codex session index when the database name is empty.
- Refresh existing message labels when titles are created or renamed, and use Untitled chat instead of session IDs while a title is unavailable.

## 0.1.38

- Single-click an overlay message to dismiss only that notification; double-click it to maximize the originating VS Code window and open its chat.
- Respect Windows double-click timing and keep message cards stationary while a click is pending.

## 0.1.37

- Made overlay messages clickable: select a message to restore and maximize its originating editor window and open the corresponding Codex chat.
- Kept incoming messages from taking keyboard focus; each card retains its own window and session target, including completed messages.

## 0.1.36

- Added an optional translucent, always-on-top message overlay for minimized VS Code sessions, with click-through behavior and no focus stealing.
- Show up to three recent assistant messages with session labels; hide them when the editor is restored and expire them automatically.
- Added toggle and preview commands plus opacity, corner, and display-duration settings. Assistant previews stay in memory and are excluded from status snapshots and logs.

## 0.1.35

- Highlighted completed sessions that have not been viewed since finishing with a distinct theme-aware blue accent in the session menu.
- Cleared the completion accent as soon as the chat is viewed, while preserving attention state across VS Code restarts without reading prompt or response content.

## 0.1.34

- Gave every currently running session a persistent theme-aware accent in the session menu.
- Kept focused running sessions visually distinct with a brighter active accent, while completed focused sessions retain the normal selection color.

## 0.1.33

- Highlighted the menu row for the Codex session currently displayed in the VS Code window that opened the Lid Guard menu.
- Preserved the normal first-row keyboard focus when the current chat is not present in the five-session menu.

## 0.1.32

- Kept the five most recently active Codex sessions in the Lid Guard menu so completed chats remain available for quick switching.
- Continued to count only currently awake turns, while prioritizing all awake sessions ahead of completed history entries.
- Kept recent-history database work out of the prompt-acquisition and background-monitoring paths.

## 0.1.31

- Captured the originating VS Code window after fast direct acquisition so awake-session navigation returns to the session's actual workspace window.
- Prevented later metadata discovery from replacing a prompt-time window association and repaired associations during direct-pipe fallback races.

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
