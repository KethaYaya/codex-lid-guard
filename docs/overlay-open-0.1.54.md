# Overlay double-click fix ? 0.1.54

A successful double-click previously cleared completion attention but left visibility dependent on VS Code view logs. Delayed or missing activity events could leave the selected overlay visible over the newly opened chat.

The native window now uses the successful open result to hide that overlay immediately. An optimistic per-chat latch ignores stale cached frames until the worker processes the action or a later focus-loss transition makes the chat eligible again. Failed opens preserve the notification and its completion state.

The worker treats the explicit open as the current chat while its editor remains focused. It overrides stale view data for that window only, so other chat tabs remain available. A matching view confirmation returns control to the normal visibility filter; a newer view event or loss of editor focus clears the override. View revisions change only for actual view events, including a new event naming the same chat, and survive unrelated log growth.

Validation covered 108 native checks across the full run and the final 22-test overlay rerun. Native UI regressions exercise failed and successful double-clicks, immediate hiding, continued suppression through stale snapshots, unaffected other tabs, and return to a minimized tab on later focus loss. The existing hover, click, animation, completion, display-scale, and native pipe tests passed. UI hit testing was isolated from unrelated live overlay windows by querying the owned HWND's native region. TypeScript compiled during packaging; extension behavior was unchanged.

Raw validation: [checks](test-results/validation-0.1.54.json).
