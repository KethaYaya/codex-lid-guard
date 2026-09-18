# Changelog

## 0.2.29

- Single-click a popped-out tab, preview, or session row to immediately expand to the latest message at the same width and keep it open, without waiting for the five-second hover timer.
- Preserve double-click maximization while the first click is still animating.

## 0.2.28

- Click a popped-out tab or preview background to keep its drawer expanded after the mouse leaves or a keyboard preview times out.
- Double-click a tab, preview, or session title to maximize that same overlay and selected chat; Open chat keeps its explicit editor navigation behavior.

## 0.2.27

- Keep overlay tabs, message previews, full chat, and their composers above other windows after app switches and window restores, without taking keyboard focus.
- Repair lost topmost state without disturbing cached animations, and keep sibling tabs from following a hidden or demoted overlay behind other apps.

## 0.2.26

- Open overlay-created chats in the same centered overlay from Open chat, double-click, or the keyboard open shortcut.
- Show background-session approval details and Allow once/Deny controls in the overlay; answer questions through its composer without losing chat drafts.
- Add Open in VS Code for background chats, routing by their saved Codex thread ID and original project. New threads use the editor-compatible history format.

## 0.2.25

- Keep the latest-message preview at the original drawer width; expand only its height.
- Keep the overlay open when a shorter reply, new empty chat, or centered full-chat layout reduces its height. Reserve animation space for both endpoints and preserve native-size text and controls.
- Recover Enable and status reads from incomplete helper replies during an upgrade. Retire only the daemon that owns the pipe, leaving concurrent command clients running.

## 0.2.24

- Smoothly fit the latest reply at the screen edge after a mouse or keyboard preview stays open for five seconds.
- Start typing in that message-sized view; maximize the same overlay after five seconds of composing or when the draft exceeds the available line width.
- Cancel typing deadlines when sending, clearing, switching sessions, or folding; preserve Copilot shortcuts, drafts, scrolling, and cached animations.

## 0.2.23

- Keep compact previews and centered chat history updating when Codex resumes a session in a new transcript file.
- Recheck the current transcript in the background and include messages written before the file switch was detected.
- Reconstruct earlier conversation history from continuation metadata, respecting its recorded boundary and preserving history across live file changes.

## 0.2.22

- After a finished pop-out is expanded by mouse hover, click, or keyboard shortcut, fold it back to the small edge tab.
- Preserve unread markers and question notices; pop out again for a new completion.

## 0.2.21

- Add an icon-only New chat action to the headers of the expanded drawer and maximized overlay.
- Create an empty Codex session in the same project, select it inside the existing overlay, and focus its message box. Preserve other sessions and their drafts; avoid duplicate sessions from double-clicks.
- Keep new chats idle until the first message, wait for startup when sending immediately, and retain normal approval handling and sleep protection during work.

## 0.2.20

- Restyle the centered conversation to match the chat mockup: compact blue user bubbles on the right and Codex answers directly on the glass, with one quiet label per consecutive reply group.
- Render bold text, numbered and bulleted lists with hanging indents, and inline code. Preserve incomplete formatting while replies arrive, Unicode wrapping, history scrolling, and cached expansion animation.
- Show image attachments as compact chips, keep literal image-placeholder text intact, and center muted notices and composer hints.

## 0.2.19

- Fix intermittent overlay sends when saved history lags behind Codex: use the live conversation owner and start a new turn only after a definitive inactive-turn rejection.
- Confirm delivery before reporting Sent, and never automatically resend after a lost connection, timeout, or uncertain response.
- Keep a second draft intact and show an explicit message if Send is pressed while the previous submission is still being confirmed.

## 0.2.18

- Animate three working dots in both the expanded drawer and centered chat, reusing cached conversation pixels and respecting reduced-motion settings.
- Show user messages in blue bubbles aligned right and Codex replies in dark bubbles aligned left, with speaker labels, padding, and paragraph spacing.
- Preserve message roles from saved history and streamed background replies, including follow-ups sent while Codex is working. Keep history scrolling and smooth expansion intact.

## 0.2.17

- Keep finished, unread result tabs popped out until the result is viewed, dismissed, or new work starts; remove the eight-second timeout.
- Typing while hovering a chat preview focuses its composer and smoothly expands the same overlay, preserving the first characters and the current draft.
- Keep Copilot shortcut chords and other modifier shortcuts separate from hover typing, and reject queued typing after hiding or switching sessions.

## 0.2.16

- Ease the centered chat expansion in and out, reducing the abrupt first jump and peak movement between frames.
- Deliver slightly late animation ticks promptly instead of skipping another frame interval, while keeping queued keyboard input responsive and avoiding catch-up bursts after long stalls.

## 0.2.15

- Include the composer in the cached expansion image, avoiding repeated native text-box resizing and repainting during growth.
- Keep growth on its animation timer; input and history notifications no longer insert extra frames between scheduled ticks.
- Keep keyboard focus and accept typing during the animation; restore the live input at the final position or safely cancel on Escape.

## 0.2.14

- Smooth chat expansion by preparing the glass and conversation once, then reusing native-size image sections throughout the animation.
- Keep text sharp, controls anchored, and Escape responsive during growth. Resume live chat updates and controls as soon as the overlay reaches its final size.

## 0.2.13

- Animate the same overlay smoothly from its current position into the centered full chat over 300 ms, retaining its normal font size and glass appearance.
- Animate Escape and the header arrow back to the edge tab, including Escape while expansion is still in progress. Keep the session and draft intact.
- Respect Windows reduced-motion settings, preallocate animation surfaces, and reuse wrapped history text during growth.

## 0.2.12

- Enlarge and center the existing glass overlay when its message box is clicked, preserving the project header, session list, footer controls, opacity, and blur.
- Display the full scrollable conversation in the enlarged message area, with per-session drafts and automatic scrolling only when already at the bottom.
- Remove the separate framed chat presentation. Enter and the send icon still clear the composer immediately; Escape and the header arrow fold the same overlay.

## 0.2.11

- Clicking the overlay message box opens a centered, resizable chat window for that session with its saved user and assistant messages and live history updates.
- Keep drafts and the selected chat when expanding, resizing, and returning to the overlay. Preserve history scroll position while reading earlier messages.
- Clear the composer immediately on Enter or the send icon; restore failed submissions without discarding newer typing. Shift+Enter adds a line. Escape, Back to overlay, and Close return to the compact overlay.

## 0.2.10

- Replace the New chat button with a message box at the bottom left and an icon-only send button. Move Open chat and Dismiss to the right.
- Send with Enter or the up-arrow icon to the selected editor or background conversation, preserving its settings and existing worker. Keep separate drafts per chat and retain text when delivery fails or is unconfirmed.
- Activate keyboard input only on click, preserve normal preview focus behavior, and fold the drawer with Escape while typing.

## 0.2.9

- Add **+ New chat** to project overlay drawers with saved editor details. Open a blank Codex sidebar chat in the matching VS Code project, reopening its window when needed.
- Preserve existing tasks and unread results when starting a chat. Reject navigation if the project or focused window changes, and keep overlay controls available if opening fails.

## 0.2.8

- Replace always-wide project tabs with 18 by 42 DIP slivers, folder monograms, and priority-sorted state beads. Reveal shortcut codes at 44 DIP while the prefix is held; show new results at 152 DIP for eight seconds and questions at 152 DIP until resolved.
- Keep unread halos after a completion card retracts. Preserve hover drawers, independent chat actions, 90 ms tab movement, and sibling clearance before expansion.
- Separate glass background opacity from foreground text and shortcuts. Reduce the default tint to 35%, preserving opaque labels, antialiased edges, and live acrylic blur. Preview three projects with working, completed, and question states.

## 0.2.7

- Add live Windows acrylic blur behind project tabs and drawers on Windows 11 22H2 and newer. Keep content and shortcuts on their existing sharp surface, with the current tint as a fallback when the backdrop API is unavailable.
- Fit separate blur surfaces to the visible panel and tab during sliding, keeping transparent gaps clear. Hide the blur when opening a chat, hiding the overlay, or using full opacity; preserve non-activating input and project stacking.

## 0.2.6

- Give project tabs and drawers a dark glass appearance with translucent surfaces, soft highlights, fine edges, and inset selection and action controls. Keep the existing opacity setting and cached drawing path; no live background blur or extra animation timers.

## 0.2.5

- Keep assigned two-letter shortcuts visible on compact project tabs. Use a quiet badge normally and highlight it while the shortcut prefix is held. Keep the extra-session count visible alongside it.

## 0.2.4

- Halve overlay animation time: neighboring tabs slide in 90 ms and drawers expand or fold in 120 ms. Preserve the clearance check before expansion and the wait before tabs return.

## 0.2.3

- Slide neighboring tabs clear before opening a drawer, then slide them back only after folding finishes. Wait for the final painted positions so expanding panels cannot cover tabs still in motion.
- Keep rapid animation reversals safe and respect reduced-motion preferences. Verify visible intermediate slide positions and opening/folding order in native interaction tests.

## 0.2.2

- Group overlays by project, with minimal two-line tabs showing the project, priority task, status icon, and extra-session count. Use an amber project stripe while a session needs a response.
- Reduce drawers to 344 DIP wide with up to four 28 DIP task rows and a two-line preview. Selecting a task does not reflow the list; larger lists scroll within the drawer. Put Open chat and Dismiss directly below the preview.
- Prioritize needs-response, working, unread completion, then idle/read completion while preserving the selected session and stable order within each status group. Distinguish same-named projects with path labels.
- Move neighboring tabs aside before expansion and return them after folding; wrap long stacks into another column when needed. Always raise an expanded project above sibling tabs without taking keyboard focus, including hover, click, keyboard, refresh, and tab-return paths.
- Reveal shortcut letter codes only while the prefix key is held. Enter opens the selected session; Escape dismisses only that session. The header chevron or existing Tab shortcut folds the project.
- Update the native demo to show two projects and five sample sessions.

## 0.2.1

- Show actual assistant updates and final replies from newer Codex sessions that use `item_completed` / `AgentMessage` records. Previously these chats could stay on the generic working and completion messages.
- Recover final reply text from the task completion record when a large output burst skips the earlier message. Keep legacy messages supported, ignore duplicate response records, and exclude prompts, reasoning, and tool output from previews.

## 0.2.0

- Start independent background Codex sessions from a trusted local VS Code project. The native helper owns the app-server worker so tasks and follow-ups remain available after VS Code closes.
- Add a native conversation window with streamed assistant messages, command/file approval, question responses, stop-turn and end-session controls. Closing the window docks the session without stopping it.
- Integrate background progress and busy indicators with the existing overlay tabs, keyboard shortcuts, and tray menu. Keep sleep protection while tasks await a response.
- Isolate each worker's process tree and clean it up on failure or session exit. Deduplicate retried start requests and keep idle background conversations alive during helper update checks.
- Existing VS Code turns are not transferred. Helper/Windows restarts interrupt background work; saved conversations remain in Codex history. Unsupported interactions receive errors without permission being granted.

## 0.1.81

- After Copilot + Tab expands a preview, release both keys and press Tab alone to fold that preview immediately. Keep Copilot + Tab cycling and the three-second automatic fold.
- Use the actual expanded state and keyboard selection so folded tabs, unrelated typing, mouse-opened previews, and modified Tab navigation do not capture ordinary Tab presses.

## 0.1.80

- Fold previews selected with Copilot + Tab three seconds after releasing Copilot. Keep them expanded while the prefix is held, restart the delay when cycling, and handle keyboards that send the Copilot sequence as a quick tap.
- Show animated amber busy dots in the expanded preview header as well as the minimized tab, with steady dots when Windows animations are disabled.

## 0.1.79

- Redraw the tray icon for small sizes: a bright cyan shield, clear laptop silhouette, and a contrasting outline for dark and light taskbars.
- Embed artwork from 16 to 64 pixels, choose it using the taskbar display scale, and refresh it after display settings changes.

## 0.1.78

- Configure the maximum overlay tab count from 1 to 10, with live resizing and retained chat routing.
- Configure the global shortcut prefix and cycle, open and close keys, or disable overlay shortcuts. Preserve current defaults and cancel pending shortcuts when bindings change.
- Show the configured prefix in overlay hints and keep dense tabs distinct while allowing readable expanded previews.

## 0.1.77

- Use the extension's Lid Guard logo for the helper's tray icon, embedded in the executable and sized for the Windows notification area.

## 0.1.76

- Open saved chats in the existing Codex sidebar on the right instead of creating or selecting a conversation editor tab in the centre.
- Use Codex's sidebar URI handler with VS Code's window-specific URI so navigation stays with the original project. Keep startup confirmation, retry and cancellation.
- Verify sidebar reuse, unchanged editor tabs, native-pipe routing and a full close/reopen with both a delayed sidebar fixture and the installed Codex extension.

## 0.1.75

- Wait for the requested chat to become active in Codex after reopening a project, with progress shown while its renderer starts. Retry the saved editor once if startup restores another selection. Cancel pending navigation when another open is requested, the window loses focus, or its workspace changes.
- Allow 35 seconds for chat confirmation after the new window registers, instead of five seconds.
- Add tests that close and reopen a real VS Code process with saved tabs, delayed content, and competing restoration; include an optional smoke test with the installed Codex extension.

## 0.1.74

- Fix retained tabs disappearing when their VS Code window closes: use the overlay's own display and DPI when the original window handle becomes invalid.
- Add a native regression test that destroys the originating window, then verifies the tab remains visible, updates in place, and can still open its saved project.

## 0.1.73

- Keep recent overlay tabs after closing a project folder or VS Code. Reuse an open project or reopen its folder/saved workspace before selecting the exact chat. Switching the same window to another folder still removes its old tabs.
- Keep the helper running while tabs are retained, without extending lid protection after tasks finish.
- Add a Windows tray shield with Quit Lid Guard to close all tabs, restore power settings, and stop the helper. Background checks respect quitting; use Enable or the stopped status-bar item to restart.

## 0.1.72

- Show the chat's project folder name in the live-updates heading.

## 0.1.71

- Open overlay chats through a direct connection to the owning VS Code extension host, including sessions with no existing editor tab. Keep exact-tab and editor-group reuse.
- Remove old overlay tabs when the same VS Code window switches to a different folder. Reject queued clicks from the previous workspace and preserve tabs in other windows.
- Add navigation diagnostics and end-to-end native-pipe tests for absent tabs, loaded webview content, and workspace changes. Reload existing VS Code windows after installing this update.

## 0.1.70

- Cycle through visible overlay tabs with Copilot + Tab. Expand the selected preview, tuck the previous one, skip hidden tabs, and wrap after the last tab.
- Press Enter to open the keyboard-selected chat and maximize its originating VS Code window. Keep the letter shortcuts and Escape dismissal working.
- Support repeated Copilot key presses as well as holding the key, without capturing ordinary Tab or Enter input outside an active shortcut.

## 0.1.69

- Add a close button to expanded overlays. Close the selected panel and tab until its next turn, without opening or stopping the chat.
- Press Escape after Copilot plus the first shortcut letter to close that tab. Unselected, expired, or cancelled shortcuts leave other applications alone.
- Catch up to the latest bounded transcript tail after large output bursts so completion is visible on the next refresh. Prevent older start records from making an already finished turn look busy again.

## 0.1.68

- Open the exact Codex conversation editor from an overlay or session menu, reusing its existing tab and editor group when several chats share a project window.
- Confirm that the requested session is active before dismissing its overlay or clearing its completion cue. Failed or unconfirmed opens keep the notification available.
- Reload existing VS Code windows once after updating to register the new session navigation handler.

## 0.1.67

- Change the completed-tab background fade to the requested yellow, `#FFD000`, while keeping the same simple animation and steady session initials.

## 0.1.66

- Remove the completion checkmark from tabs. Use the background color fade as the completion cue, keeping the usual open chevron and session initials steady.

## 0.1.65

- Simplify completed tabs to a smooth, uniform background fade between charcoal and green. Remove the edge light and halo, and center the steady checkmark and initials again.
- Reuse the existing completion animation and drawing buffer with a single background fill. Keep completion acknowledgement, expanded-panel dots, and reduced-motion support.

## 0.1.64

- Refine completed tabs with a charcoal surface, a slim pulsing emerald light and soft halo along the exposed edge, a mint checkmark, and steady off-white initials.
- Keep completion easy to spot without flashing the whole tab. Expanded-panel completion dots, fixed screen attachment, input behavior, and reduced-motion support remain intact.

## 0.1.63

- Make completion pulses clearly visible by brightening and dimming the entire green tab and its rim together every 1.8 seconds. Keep white initials and the checkmark readable throughout.
- Keep fixed tab geometry, existing paint buffers and timer, and steady completion styling when Windows animations are disabled.

## 0.1.62

- Make completed tabs clearly recognizable at a glance with a green background, bright pulsing green rim, white initials, and a checkmark. The completion color stays visible between pulses.
- Preserve the existing flush screen edge, tab size, input behavior, and completion acknowledgement. Reduced motion keeps the completion styling steady.

## 0.1.61

- Add a subtle green inner glow to completed session tabs, synchronized with the existing completion dot pulse. Viewing the matching chat or starting a new turn clears both indicators.
- Keep the glow inside the existing tab shape, preserving the flush screen edge, slide geometry, and hover/click targets. Reuse the existing animation timer and drawing buffer; reduced motion uses a steady glow.

## 0.1.60

- Hide the matching overlay when its chat is open in the focused editor, even when newer command-line launches create empty log folders beside the running VS Code instance.
- Recover visibility events from earlier in a long chat log after a helper restart, then read only appended bytes with bounded line buffering.
- Resolve each chat from its own view and turn-start events; unrelated windows mentioning the same conversation no longer override its visibility.

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
