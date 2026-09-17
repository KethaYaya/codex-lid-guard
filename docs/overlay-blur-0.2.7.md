# Live overlay blur (0.2.7)

Project tabs and drawers use the system's desktop acrylic backdrop on Windows 11 build 22621 (22H2) and later. The previous dark glass tint remains the fallback when the native backdrop API is unavailable. Windows controls transparency, accessibility, and power-related material fallbacks.

The content keeps its existing layered drawing surface, text, shortcut badges, opacity, and animation timing. The opacity setting controls that foreground surface; at 100% opacity the hidden background blur is suppressed. The app does not capture the desktop in production or run a background repaint timer. DWM performs the live blur, which adds compositor work; no claim of zero GPU or battery cost is made.

Each project has two non-activating background windows, with only the visible panel or tab shown. During the slide both may be visible. Their bounds are clipped to the visible content and stay beneath it in Z order. Separate rectangles matter: system backdrops do not respect the complex region used for the layered overlay's transparent gutter. The native corner preference provides the backdrop's rounded corners.

Background windows hide with the project, at full opacity, and when chat activation begins. Their resources are destroyed when the overlay closes. A backdrop creation or positioning failure leaves the normal overlay usable with its glass tint. Existing neighbor clearance and folding barriers still apply before acknowledging the final positions.

Validation includes:

- Native pixels over an owned checkerboard fixture: fine detail is blurred while the large red/blue areas remain distinct, ruling out an opaque replacement.
- Changing only that background fixture reverses the backdrop colors without repainting the blur window.
- The transparent gap between separate panel/tab areas retains the sharp checkerboard.
- Native group tests check panel/tab blur positions, Z order, hiding, resource cleanup, and lack of activation, alongside the existing slide and exact-chat interaction checks.
- Geometry checks at 100%, 150%, and 200% scale constrain every backdrop rectangle to visible slide pixels.

The desktop sampling test is opt-in and samples only within its own visible fixture:

```powershell
cargo test --locked --manifest-path native/CodexLidGuard/Cargo.toml native_backdrop_blurs -- --ignored --nocapture --test-threads=1
cargo test --locked --manifest-path native/CodexLidGuard/Cargo.toml native_group_ -- --ignored --nocapture --test-threads=1
```

Native API references: [system backdrop materials](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_systembackdrop_type), [extending the glass frame](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmextendframeintoclientarea), and [window backdrop attributes](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute).
