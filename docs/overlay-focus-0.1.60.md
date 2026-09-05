# Focused chat visibility - 0.1.60

The live overlay stayed visible because log discovery considered only the six newest VS Code run folders. Command-line operations, including extension installation, had created newer folders without editor windows. The still-running editor's actual Codex logs were in an older run and were therefore ignored entirely.

Two further lookup problems affected long chats and multiple windows. A fresh helper read only the last 256 KiB of each log, so it could not recover a visibility event recorded hours earlier. It also treated any conversation ID mentioned by a log as evidence that the chat belonged to that window. Cross-window notifications could consequently override a chat's actual view state.

Discovery now ranks actual Codex log files by modification time across retained run folders. Each log is scanned once on first use with a 16 KiB read buffer and at most 64 KiB of pending line data. Subsequent checks consume only appended bytes; complete visibility events and their revisions remain cached through unrelated log growth. Oversized lines are discarded through their newline, partial events wait until complete, and truncation or replacement resets the cache. Only local view events and the existing local turn-start event establish a chat's association with a log.

The existing overlay filter still requires both the matching session and its originating editor window to be focused. Other chats keep their tabs, and minimizing or switching away returns the relevant tab. The shared native completion-alert lookup uses the same corrected reader.

Validation passed 10 log-reader tests, 14 overlay feed tests, 21 daemon tests, and the owned native minimize/restore visibility test. Clippy passed for all targets with warnings denied. New regressions cover a running editor behind ten newer empty run folders, a visibility event followed by more than 2 MiB of log output, unrelated cross-window conversation mentions, and partial or oversized appended lines.

A read-only probe compiled from the actual reader against the user's live logs resolved neither requested chat before the discovery fix and resolved both correctly afterward. The initial read took 44.251 ms and the cached read took 5.032 ms in this sample, off the overlay's UI thread. The native test verified hidden focused content, immediate cached minimize, restore hiding, and same-window chat switching without moving or activating the user's editor. These checks verify the reader and owned overlay behavior; the installed live helper must activate before the real tabs use this fix.
