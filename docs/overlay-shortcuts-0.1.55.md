# Overlay keyboard shortcuts - 0.1.55

For a tab labeled DR, hold Copilot and press D to expand its cached message; add R to open that session in VS Code. Keyboard expansion stays open like a clicked tab. Opening uses the existing success/failure and immediate-hide path. Other tabs keep their state.

The standard Copilot sequence is Win+Shift+F23. While tabs are visible it begins an overlay shortcut; with no visible tabs the key passes through normally. Hardware that releases its macro immediately can use a 1.5-second prefix window. Holding Copilot keeps the chord available. Escape, an unrelated key/modifier, focus change, or replacement of the selected binding cancels it. Ordinary letters without a Copilot prefix pass through unchanged. Codes use distinct leading letters and remain stable while visible; the expanded footer shows the code.

One dedicated Windows hook thread holds a fixed-size keyboard state and three cached bindings. It does not read chat data, log text or activate a window from the hook. Actions are posted to the relevant overlay thread with a generation token, which prevents stale commands from opening a different chat. UI publishing avoids locks and allocations when a binding is unchanged. The Copilot macro's Windows-key release is masked with a tagged, unassigned virtual key to avoid opening Start; the listener ignores its own injected mask events. No letters are synthesized.

Microsoft recommends a dedicated thread that immediately delegates work from a low-level keyboard callback: [LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc). The native event layout and injection marker follow [KBDLLHOOKSTRUCT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-kbdllhookstruct).

Validation covered 118 native checks across the full and targeted runs. The real Windows hook registered and released successfully. The keyboard UI test sent synthetic events only into the dedicated shortcut thread and observed expansion in 19.09 and 19.29 ms, correct open targets, immediate hiding, unaffected other tabs, and rejection of stale route tokens. No synthetic input was sent to another app. Tests also covered plain typing, modifier handling, key repeats/releases, macro release, code collisions, timeout, Escape, and focus/session changes. The physical OEM Copilot key was not exercised; a remapped key must still emit the standard sequence. Clippy passed with warnings denied and TypeScript compiled during packaging.

Raw validation: [results](test-results/validation-0.1.55.json).
