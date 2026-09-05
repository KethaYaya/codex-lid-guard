# Three-chat overlay validation ? 0.1.49

All 99 native tests and 36 extension tests passed. Native checks included real Windows interaction with three separate panels and the extension socket integration. Rust Clippy passed for all targets with warnings denied. The preceding release test run passed 98 tests; the final native run also includes a new regression for late metadata promotion preserving completion.

The overlay selects the three most recently active eligible chats, shows one current message per chat, and keeps surviving chats in their existing display lanes. Tests cover independent collapse and expiry, per-chat busy/completion state and double-click targets, fourth-chat replacement, no focus theft, cached input during blocked file reads, and non-overlapping geometry at 100%, 150%, and 200% scaling. These previews still appear only for tracked sessions whose originating editor is minimized.

The final release helper's 35-second preview displayed three panels. Every panel was collapsed and reopened while checking that the other two windows kept their positions. Busy dots changed to independent completion dots at 15, 18, and 21 seconds. The helper closed normally.

| Phase (three panels) | CPU, % of one logical processor | Maximum working set, MiB |
| --- | ---: | ---: |
| expanded busy | 0.000 | 14.09 |
| collapsed busy | 3.625 | 14.12 |
| expanded complete | 3.601 | 14.12 |
| collapsed complete | 3.854 | 14.14 |
| reopened complete | 3.098 | 14.11 |

Private memory remained approximately 2.64?2.70 MiB. GDI resources varied from 17 to 22 while painting, and USER resources stabilized at 18 with the activity timers running. No accumulating growth was observed during this short run. CPU measurements use Windows process times over 1?3-second intervals; zero means no measurable increment at that counter's resolution. These measurements cover the owned preview process, not the entire live guardian or other apps.

The release input check measured 39 ms before the tab began opening. A message click took 542 ms, including the user's 500 ms Windows double-click interval. The final debug run measured 30 ms and 550 ms respectively. Sliding animation duration remains 240 ms.

The VSIX's helper, extension client, and README were byte-compared with the local release outputs before installation.

Raw results: [three-panel performance](test-results/overlay-three-chats-0.1.49.json), [functional validation](test-results/validation-0.1.49.json).
