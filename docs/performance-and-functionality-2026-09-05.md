# Performance and functionality review ? 2026-09-05

Version 0.1.48 passed all 128 tests: 92 native release tests, including the desktop and Node integration tests, plus 36 extension tests. Rust Clippy passed with warnings treated as errors. The VSIX was built with the release helper.

The review reproduced and fixed four functional issues:

- The 35-second preview was killed by the extension's 20-second timeout. The timeout is now 45 seconds; the actual extension launcher completed a preview in 35.325 seconds.
- A timed-out pipe request left a pending read and an open connection. Socket cancellation now releases it. Fragmented Unicode, malformed replies, incomplete replies, and the one-MiB limit are covered.
- A new turn could display the previous turn's final result as Done. Starting a turn now clears that session's previous previews.
- Explicitly disconnecting a native pipe discarded replies before slower clients read them. Each connection already owns a fresh instance, so closing its handle preserves queued data. A delayed-reader regression and 50 sequential requests from the actual extension client passed against an isolated native server, with no failures.

Activity frames now repaint only the small indicator areas. The expanded header, completed-card dot, busy dots, clicking, docking, and reopening retain their behavior.

| Check | Result |
| --- | --- |
| Notch click to first slide movement | 16.26 ms in final release test |
| Message click to first slide movement | 538.85 ms, including the Windows 500 ms double-click interval |
| Extension-to-native pipe, 50 requests | 0 failures; median 0.095 ms; maximum 2.534 ms |
| Lifecycle metadata watcher, 128 samples | median 1.755 ms; p95 1.941 ms; maximum 8.615 ms |
| Completion retention | Survives expiry and tucking; clears only for the viewed session or a new turn |
| Native interaction | No focus theft on arrival, correct card targets, expanded pulse, reopening, and transparent-gutter hit testing passed |
| Geometry and accessibility | Corner placement, adjacent-display clipping, 96/144/192-DPI geometry, and reduced-motion logic passed |
| Core protection | Lifecycle, cancellation, concurrent turns, provisional acquisitions, power-policy recovery, and version handoff tests passed |

CPU is expressed as a percentage of **one logical processor**, measured using Windows process times over 3?5-second phases in a 35-second preview. The first post-change run overlapped a Rust test build; the final repeat ran without a build. These short samples vary, and a displayed zero means no measurable CPU-time increment in that interval, not literally zero work.

| Phase | 0.1.47 baseline | First optimized run | Final quiet run |
| --- | ---: | ---: | ---: |
| Expanded, busy | 0.000% | 0.000% | 0.000% |
| Notch, busy | 0.311% | 0.311% | 0.623% |
| Expanded, complete | 2.797% | 0.311% | 0.000% |
| Notch, complete | 0.777% | 0.000% | 0.000% |
| Reopened, complete | 2.590% | 1.554% | 0.000% |

The initial expanded-completion comparison fell from 2.797% to 0.311% of one core, about 89%. The final quiet completion phases were below the counter's resolution. Busy-notch figures varied and do not establish a performance improvement there.

In the final run, private memory remained between 2.23 and 2.25 MB and peak working set was 14.24 MB. GDI objects stayed between 6 and 8 and USER objects between 5 and 6, with no sustained growth in this short run. This is not a long-duration leak test.

Power recovery was tested using the isolated test policy; no physical lid close, real sleep cycle, or real-chat navigation was triggered. Native window tests ran on this desktop; other DPI/display combinations were tested through geometry logic.

Reproduce the checks from the repository root:

```powershell
npm.cmd --prefix extension test
cargo test --manifest-path native/CodexLidGuard/Cargo.toml --release -- --include-ignored --test-threads=1 --nocapture
cargo clippy --manifest-path native/CodexLidGuard/Cargo.toml --all-targets -- -D warnings
python scripts/measure-overlay.py native/CodexLidGuard/target/release/CodexLidGuard.exe docs/test-results/local-overlay.json
```

Raw measurements: [baseline](test-results/overlay-before-0.1.47.json), [first optimized run](test-results/overlay-after-0.1.48.json), [quiet repeat](test-results/overlay-final-0.1.48.json), and [validation summary](test-results/validation-0.1.48.json).
