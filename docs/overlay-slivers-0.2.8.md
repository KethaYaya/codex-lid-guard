# Project slivers (0.2.8)

Implements the four-board overlay concept supplied on September 17, 2026. All states stay 42 DIP tall and keep their right edge anchored:

| State | Width | Content and duration |
| --- | --- | --- |
| Calm | 18 DIP | Folder monogram, identity stripe, three priority-sorted beads, then a dash for additional sessions. |
| Prefix held | 44 DIP | Monogram and beads plus the assigned two-letter shortcut. Releasing the prefix retracts it. |
| New result | 152 DIP | Project, check, and completed task title for eight seconds. The unread halo survives retraction. |
| Question | 152 DIP | Amber stripe and question text until the question is resolved. Takes priority over completion notices. |

A prefix held over a full-width notice reveals its shortcut without hiding the notice. Working beads breathe over 2.4 seconds; read/idle beads are hollow grey, unread results have green halos, and questions use amber rings. Windows' reduced-motion preference disables breathing and width animation. Width transitions use the existing 90 ms cadence. The working-bead timer runs only for a visible, collapsed, working group with beads showing; cached drawer text is not repainted by its activity ticks.

The stack retains its existing maximum column reservation, 42 DIP tab height, and 120 ms drawer timing. Smaller tabs do not cause sideways column churn. Native painted-position acknowledgements still prevent a drawer from covering siblings before they slide clear, and release the reserved space after folding. Hover, direct actions, prefix navigation, per-session dismissal, and chat identity remain intact.

## Glass material correction

The default background tint is now 35%. The opacity setting changes the background contribution, leaving labels, glyphs, and shortcut badges opaque. GDI black/white coverage passes recover foreground alpha, including antialiased edges; only the background contribution is reduced. Three cached drawer surfaces prevent re-laying out text on animation ticks. Frozen chat-opening frames preserve the same per-pixel alpha.

The existing DWM acrylic underlay supplies live blur. If that API is unavailable, a minimum 82% fallback tint keeps text legible against the unblurred desktop. At 100% opacity the underlay is hidden. Explicit opacity preferences remain respected; users retaining an old 82% preference can lower **Overlay Opacity** to 35%. The standalone Preview Message Overlay demonstrates the new 35% default without changing saved preferences.

## Verification

- Unit tests cover completion expiry without resetting on refresh, subsequent completions, question precedence, prefix reversal, and retaining unread state.
- Native window tests exercise 18/44/152 DIP widths, the eight-second notice, return to a sliver with unread state intact, question/read transitions, exact-session actions, focus preservation, and sibling clearance.
- Rendering fixtures cover 100%, 150%, and 200% display scaling, calm/prefix/completed/question states, and task hit targets.
- The native checkerboard fixture verifies both the live acrylic and the final 35% material: broad background colors remain visible while the foreground remains opaque. Only pixels inside the owned fixture are sampled.
- Coverage tests check text, shortcut, and state colors stay opaque, background alpha is reduced, and antialiased pixels remain premultiplied.

```powershell
cargo test --locked --manifest-path native/CodexLidGuard/Cargo.toml
cargo clippy --locked --manifest-path native/CodexLidGuard/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path native/CodexLidGuard/Cargo.toml native_group_ -- --ignored --nocapture --test-threads=1
cargo test --locked --manifest-path native/CodexLidGuard/Cargo.toml native_backdrop_blurs -- --ignored --nocapture --test-threads=1
```
