# Overlay motion and focus validation ? 0.1.53

Newly backgrounded chat messages shrink into their own edge tabs over 260 ms. Existing tucked tabs stay in place. Hover expands a cached message over 240 ms, and leaving for 200 ms starts its return slide. A stable focus-transition token prevents regular background polling from undoing a hover expansion; a later loss of focus tucks an expanded preview again. Only the matching visible chat in its focused editor hides its overlay.

Motion now uses a high-resolution waitable timer with input-aware waits, without changing the system timer resolution. Timers stop when transitions finish. The message is painted into a reusable bitmap and copied during sliding or smoothly downsampled during arrival, avoiding repeated text rasterization. Hover no longer enters the feed/layout refresh path. Pulses retain their small-area repainting and pause during motion; Windows reduced-motion settings snap transitions directly to their destination.

All 106 native tests and 36 extension tests passed. The native run included three explicit Windows UI tests and 50 extension/native pipe requests with no failures. The final refinement preventing already tucked tabs from replaying arrival was rerun in its native focus regression test. Clippy passed for all targets with warnings denied. Tests cover no focus theft, exact chat identity, hover leave/re-entry, independent tabs, double-click routing, completion pulses, focus-loss requests, viewed-chat hide/return, display clipping, geometry at 100%, 150%, and 200% scaling, and reduced motion. Native hover response was 18.6 ms; hover-out including its grace period and slide was 553 ms in that run.

## Release measurements

Both builds used the same owned three-window preview, with all tabs initially tucked and six repeated expansions of the middle tab. The other tabs' positions were checked after each expansion. Measurements track native window geometry delivery, not compositor or physical display presentation, so they do not establish an absolute stutter-free guarantee under every workload.

| Measure | 0.1.52 | 0.1.53 |
| --- | ---: | ---: |
| Median of six within-animation median intervals | 22.69 ms | 16.47 ms |
| Worst per-animation p95 update interval | 39.47 ms | 25.70 ms |
| Geometry changes per expansion | 9?11 | 13?15 |
| CPU during repeated interactions, % of one logical processor | 15.897 | 10.945 |

CPU during this interaction workload decreased by 31.1%. The final run's GDI and USER counts remained at 23 and 18 between its initial and final samples. Private memory changed from 2.82 to 2.84 MiB. These are short local measurements, not a long-duration leak test.

A separate 35-second preview exercised every tab through busy and completion states and closed normally. No accumulating resource growth was observed.

| Phase, three previews | CPU, % of one logical processor | Maximum working set, MiB |
| --- | ---: | ---: |
| expanded busy | 0.512 | 13.95 |
| collapsed busy | 3.084 | 13.96 |
| expanded complete | 2.075 | 13.97 |
| collapsed complete | 0.000 | 13.97 |
| reopened complete | 1.512 | 13.95 |

CPU counters have finite resolution; a zero value means no measured counter increment in that interval. The expanded and tucked release previews were visually checked: text remained readable and the tab retained its caption and busy indicator. The transient screenshot was removed because transparent margins include desktop pixels.

The packaged helper, extension client, README, and installed payload were verified. The installed version is 0.1.53. Activation is scheduled for the existing guardian's next idle state, preserving the currently running chats.

Raw data: [before motion](test-results/overlay-motion-before-0.1.52.json), [after motion](test-results/overlay-motion-after-0.1.53.json), [35-second preview](test-results/overlay-three-chats-0.1.53.json), [validation](test-results/validation-0.1.53.json).
