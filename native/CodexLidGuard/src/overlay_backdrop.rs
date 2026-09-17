//! DWM draws the live desktop acrylic beneath our existing, sharp content surface.
//! A separate non-layered window keeps backdrop composition compatible with the
//! per-pixel layered surface used by the overlay's sliding animation.
use super::*;

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmExtendFrameIntoClientArea(window: Hwnd, margins: *const i32) -> i32;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindow(window: Hwnd, command: u32) -> Hwnd;
}

struct Surface {
    window: Hwnd,
    class: Vec<u16>,
    instance: Handle,
    last: Option<Rect>,
    visible: bool,
}

impl Surface {
    unsafe fn new(content: Hwnd, part: &str) -> io::Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(null());
            let class = wide(format!(
                "CodexLidGuardBackdrop.{}.{}.{part}",
                GetCurrentProcessId(),
                content as usize
            ));
            let definition = WindowClassExW {
                size: size_of::<WindowClassExW>() as u32,
                style: 0,
                window_procedure: Some(procedure),
                class_extra: 0,
                window_extra: 0,
                instance,
                icon: null_mut(),
                cursor: null_mut(),
                background: null_mut(),
                menu_name: null(),
                class_name: class.as_ptr(),
                small_icon: null_mut(),
            };
            if RegisterClassExW(&definition) == 0 {
                return Err(error("Register overlay backdrop"));
            }
            let window = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | 0x20,
                class.as_ptr(),
                wide("Overlay backdrop").as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            if window.is_null() {
                UnregisterClassW(class.as_ptr(), instance);
                return Err(error("Create overlay backdrop"));
            }
            let result = Self {
                window,
                class,
                instance,
                last: None,
                visible: false,
            };
            let dark = 1u32;
            let corners = if part == "panel" { 2u32 } else { 3u32 };
            let no_border = 0xffff_fffeu32;
            let acrylic = 3u32; // DWMSBT_TRANSIENTWINDOW, Windows 11 build 22621+.
            for (attribute, value) in [(20, dark), (33, corners), (34, no_border), (3, 1)] {
                DwmSetWindowAttribute(window, attribute, (&value as *const u32).cast(), 4);
            }
            let hr = DwmExtendFrameIntoClientArea(window, [-1i32; 4].as_ptr());
            if hr < 0 {
                return Err(io::Error::other(format!("Extend backdrop frame: {hr:#x}")));
            }
            let hr = DwmSetWindowAttribute(window, 38, (&acrylic as *const u32).cast(), 4);
            if hr < 0 {
                return Err(io::Error::other(format!(
                    "Desktop acrylic unavailable: {hr:#x}"
                )));
            }
            // Style the transient surface as active without activating an HWND.
            DefWindowProcW(window, 0x0086, 1, -1);
            Ok(result)
        }
    }

    unsafe fn present(&mut self, after: Hwnd, rect: Rect) -> io::Result<()> {
        unsafe {
            if self.last != Some(rect) {
                if SetWindowPos(
                    self.window,
                    after,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOCOPYBITS | SWP_SHOWWINDOW,
                ) == 0
                {
                    return Err(error("Position overlay backdrop"));
                }
                InvalidateRect(self.window, null(), 0);
                UpdateWindow(self.window);
                self.last = Some(rect);
            } else if (!self.visible || GetWindow(after, 2) != self.window)
                && SetWindowPos(
                    self.window,
                    after,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                ) == 0
            {
                return Err(error("Show overlay backdrop"));
            }
            self.visible = true;
            Ok(())
        }
    }

    pub unsafe fn hide(&mut self) {
        unsafe {
            if self.visible {
                ShowWindow(self.window, 0);
                self.visible = false;
            }
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.window);
            UnregisterClassW(self.class.as_ptr(), self.instance);
        }
    }
}

// DWM system backdrops ignore complex SetWindowRgn cutouts. Separate,
// tightly bounded surfaces prevent blur from leaking into the slide gutter.
pub(super) struct Backdrop {
    panel: Surface,
    tab: Surface,
}

fn visible_part(layout: DockLayout, part: Option<Rect>) -> Option<Rect> {
    let part = part?;
    let rect = Rect {
        left: part.left.max(0),
        top: part.top.max(0),
        right: part.right.min(layout.window.right - layout.window.left),
        bottom: part.bottom.min(layout.window.bottom - layout.window.top),
    };
    (rect.right > rect.left && rect.bottom > rect.top).then_some(Rect {
        left: rect.left + layout.window.left,
        right: rect.right + layout.window.left,
        top: rect.top + layout.window.top,
        bottom: rect.bottom + layout.window.top,
    })
}

impl Backdrop {
    pub unsafe fn new(content: Hwnd) -> io::Result<Self> {
        unsafe {
            Ok(Self {
                panel: Surface::new(content, "panel")?,
                tab: Surface::new(content, "tab")?,
            })
        }
    }
    pub unsafe fn present(
        &mut self,
        content: Hwnd,
        layout: DockLayout,
        _dpi: u32,
    ) -> io::Result<()> {
        unsafe {
            let mut after = content;
            if let Some(rect) = visible_part(layout, layout.panel) {
                self.panel.present(after, rect)?;
                after = self.panel.window;
            } else {
                self.panel.hide();
            }
            if let Some(rect) = visible_part(layout, layout.tab) {
                self.tab.present(after, rect)?;
            } else {
                self.tab.hide();
            }
            Ok(())
        }
    }
    pub unsafe fn hide(&mut self) {
        unsafe {
            self.panel.hide();
            self.tab.hide();
        }
    }
}

unsafe extern "system" fn procedure(
    window: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
) -> Lresult {
    unsafe {
        match message {
            WM_MOUSEACTIVATE => 3,                            // MA_NOACTIVATE.
            0x0084 => -1, // HTTRANSPARENT: input belongs to the content above.
            0x0086 => DefWindowProcW(window, message, 1, -1), // Keep transient acrylic while unfocused.
            WM_ERASEBKGND => 1,
            0x031e => {
                // WM_DWMCOMPOSITIONCHANGED.
                DwmExtendFrameIntoClientArea(window, [-1i32; 4].as_ptr());
                DefWindowProcW(window, message, wparam, lparam)
            }
            WM_PAINT => {
                let mut paint: PaintStruct = zeroed();
                let dc = BeginPaint(window, &mut paint);
                let mut rect: Rect = zeroed();
                GetClientRect(window, &mut rect);
                // Zero alpha in the extended glass area exposes the system backdrop.
                fill_rectangle(dc, &rect, 0);
                EndPaint(window, &paint);
                0
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backdrop_parts_follow_visible_slide_pixels_without_covering_the_gutter() {
        for dpi in [96, 144, 192] {
            let d = |n| scale_dip(n, dpi);
            let work = Rect {
                left: -1920,
                top: 0,
                right: 0,
                bottom: 1080,
            };
            let expanded = Rect {
                left: -d(344),
                top: 100,
                right: 0,
                bottom: 100 + d(193),
            };
            for step in 0..=100 {
                let layout = overlay_dock::dock_layout_sized(
                    expanded,
                    work,
                    step as f32 / 100.0,
                    dpi,
                    None,
                    Some(TabPlacement {
                        center: expanded.bottom - d(21),
                        height: d(42),
                    }),
                    d(152),
                );
                let panel = visible_part(layout, layout.panel);
                let tab = visible_part(layout, layout.tab);
                for rect in [panel, tab].into_iter().flatten() {
                    assert!(
                        rect.left >= layout.window.left
                            && rect.right <= layout.window.right
                            && rect.top >= layout.window.top
                            && rect.bottom <= layout.window.bottom
                    );
                }
                if let (Some(a), Some(b)) = (panel, tab) {
                    assert!(
                        a.right <= b.left || b.right <= a.left,
                        "backdrop parts must not overlap"
                    );
                }
                if step == 100 {
                    assert!(panel.is_none());
                    assert_eq!(tab, Some(layout.window));
                }
            }
        }
    }
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmFlush() -> i32;
    }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn GetPixel(dc: Handle, x: i32, y: i32) -> u32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn PeekMessageW(
            message: *mut Message,
            window: Hwnd,
            first: u32,
            last: u32,
            remove: u32,
        ) -> Bool;
    }
    unsafe fn pump() {
        unsafe {
            let until = Instant::now() + Duration::from_millis(300);
            while Instant::now() < until {
                let mut message: Message = zeroed();
                while PeekMessageW(&mut message, null_mut(), 0, 0, 1) != 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            DwmFlush();
        }
    }
    unsafe extern "system" fn fixture(
        window: Hwnd,
        message: u32,
        wparam: Wparam,
        lparam: Lparam,
    ) -> Lresult {
        unsafe {
            if message == WM_MOUSEACTIVATE {
                return 3;
            }
            if message == WM_PAINT {
                let mut paint: PaintStruct = zeroed();
                let dc = BeginPaint(window, &mut paint);
                for y in (0..360).step_by(8) {
                    for x in (0..560).step_by(8) {
                        let swapped = GetWindowLongPtrW(window, GWLP_USERDATA) != 0;
                        let color = if (x / 8 + y / 8) % 2 == 0 {
                            if (x < 280) ^ swapped {
                                color_ref(255, 80, 20)
                            } else {
                                color_ref(20, 80, 255)
                            }
                        } else {
                            color_ref(235, 235, 235)
                        };
                        fill_rectangle(
                            dc,
                            &Rect {
                                left: x,
                                top: y,
                                right: x + 8,
                                bottom: y + 8,
                            },
                            color,
                        );
                    }
                }
                EndPaint(window, &paint);
                return 0;
            }
            DefWindowProcW(window, message, wparam, lparam)
        }
    }

    #[test]
    #[ignore = "shows owned checkerboard and backdrop fixture; samples only inside that fixture"]
    fn native_backdrop_blurs_live_fixture_without_activation() {
        unsafe {
            let previous = SetThreadDpiAwarenessContext(-4isize as Handle);
            let instance = GetModuleHandleW(null());
            let class = wide("CodexLidGuardBackdropFixture");
            let definition = WindowClassExW {
                size: size_of::<WindowClassExW>() as u32,
                style: 0,
                window_procedure: Some(fixture),
                class_extra: 0,
                window_extra: 0,
                instance,
                icon: null_mut(),
                cursor: null_mut(),
                background: null_mut(),
                menu_name: null(),
                class_name: class.as_ptr(),
                small_icon: null_mut(),
            };
            assert_ne!(RegisterClassExW(&definition), 0);
            let window = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class.as_ptr(),
                wide("Owned blur fixture").as_ptr(),
                WS_POPUP,
                80,
                80,
                560,
                360,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            assert!(!window.is_null());
            struct Cleanup(Hwnd, Vec<u16>, Handle, Handle);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    unsafe {
                        DestroyWindow(self.0);
                        UnregisterClassW(self.1.as_ptr(), self.2);
                        SetThreadDpiAwarenessContext(self.3);
                    }
                }
            }
            let _cleanup = Cleanup(window, class, instance, previous);
            ShowWindow(window, 4);
            UpdateWindow(window);
            pump();
            let mut backdrop = Backdrop::new(window).unwrap();
            let layout = DockLayout {
                window: Rect {
                    left: 120,
                    top: 120,
                    right: 600,
                    bottom: 400,
                },
                panel: Some(Rect {
                    left: 0,
                    top: 0,
                    right: 480,
                    bottom: 280,
                }),
                tab: None,
                flush_right: false,
            };
            backdrop.present(-1isize as Hwnd, layout, 96).unwrap();
            pump();
            let capture = || {
                let screen = GetDC(null_mut());
                let mut buffer = PaintBuffer::default();
                let dc = buffer.get(screen, 480, 280);
                assert_ne!(BitBlt(dc, 0, 0, 480, 280, screen, 120, 120, 0x40cc0020), 0);
                ReleaseDC(null_mut(), screen);
                let mut pixels = Vec::new();
                for y in 0..280 {
                    for x in 0..480 {
                        pixels.push(GetPixel(dc, x, y));
                    }
                }
                pixels
            };
            let pixels = capture();
            if let Some(directory) = std::env::var_os("CODEX_OVERLAY_RENDER_DIR") {
                std::fs::create_dir_all(&directory).unwrap();
                let mut bytes = b"P6\n480 280\n255\n".to_vec();
                for pixel in &pixels {
                    bytes.extend_from_slice(&[
                        *pixel as u8,
                        (*pixel >> 8) as u8,
                        (*pixel >> 16) as u8,
                    ]);
                }
                std::fs::write(
                    std::path::Path::new(&directory).join("live-backdrop.ppm"),
                    bytes,
                )
                .unwrap();
            }
            let sample = |x: usize, y: usize| pixels[y * 480 + x];
            let red_blue = |pixel: u32| (pixel & 255) as i32 - ((pixel >> 16) & 255) as i32;
            assert!(
                red_blue(sample(80, 100)) > 40 && red_blue(sample(400, 100)) < -40,
                "backdrop must preserve broad background colors, not turn opaque"
            );
            let mut contrast = 0u64;
            let mut count = 0u64;
            for y in 40..240 {
                for x in 40..180 {
                    for shift in [0, 8, 16] {
                        contrast += (((sample(x, y) >> shift) & 255) as i32
                            - ((sample(x + 8, y) >> shift) & 255) as i32)
                            .unsigned_abs() as u64;
                        count += 1;
                    }
                }
            }
            let detail = contrast as f64 / count as f64;
            assert!(
                detail < 12.0,
                "fine checkerboard detail must be blurred: {detail}"
            );
            SetWindowLongPtrW(window, GWLP_USERDATA, 1);
            InvalidateRect(window, null(), 0);
            UpdateWindow(window);
            pump();
            let changed = capture();
            assert!(
                red_blue(changed[100 * 480 + 80]) < -40 && red_blue(changed[100 * 480 + 400]) > 40,
                "the native blur must follow background changes without repainting the overlay"
            );
            println!("Live acrylic verified; residual checker detail {detail:.2}/255");
            // Verify the finished two-layer material, not just the blur behind it.
            {
                let content = CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    wide("STATIC").as_ptr(),
                    wide("Owned glass text fixture").as_ptr(),
                    WS_POPUP,
                    120,
                    120,
                    480,
                    280,
                    null_mut(),
                    null_mut(),
                    instance,
                    null(),
                );
                assert!(!content.is_null());
                struct Owned(Hwnd);
                impl Drop for Owned {
                    fn drop(&mut self) {
                        unsafe {
                            DestroyWindow(self.0);
                        }
                    }
                }
                let _owned = Owned(content);
                let screen = GetDC(null_mut());
                let mut buffers = [
                    PaintBuffer::default(),
                    PaintBuffer::default(),
                    PaintBuffer::default(),
                ];
                let mut dcs = [null_mut(); 3];
                let bounds = layout.panel.unwrap();
                let font = CreateFontW(
                    -24,
                    0,
                    0,
                    0,
                    600,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    5,
                    0,
                    wide("Segoe UI").as_ptr(),
                );
                for (i, buffer) in buffers.iter_mut().enumerate() {
                    let dc = buffer.get(screen, 480, 280);
                    dcs[i] = dc;
                    fill_rectangle(dc, &bounds, [color_ref(30, 40, 54), 0, 0xffffff][i]);
                    SetBkMode(dc, TRANSPARENT);
                    let old = SelectObject(dc, font);
                    let mut text_rect = Rect {
                        left: 175,
                        top: 24,
                        right: 350,
                        bottom: 60,
                    };
                    draw_text(
                        dc,
                        "Glass preview",
                        &mut text_rect,
                        color_ref(242, 247, 253),
                        DT_SINGLELINE,
                    );
                    fill_rectangle(
                        dc,
                        &Rect {
                            left: 210,
                            top: 65,
                            right: 250,
                            bottom: 85,
                        },
                        color_ref(242, 247, 253),
                    );
                    SelectObject(dc, old);
                }
                DeleteObject(font);
                ReleaseDC(null_mut(), screen);
                let mut frame = FrameSurface::new(480, 280).unwrap();
                frame
                    .present_material(
                        content,
                        dcs[0],
                        layout,
                        96,
                        255,
                        Some(([dcs[1], dcs[2]], 89)),
                    )
                    .unwrap();
                ShowWindow(content, 4);
                backdrop.present(content, layout, 96).unwrap();
                pump();
                let glass = capture();
                assert!(
                    red_blue(glass[100 * 480 + 80]) < -20 && red_blue(glass[100 * 480 + 400]) > 20,
                    "the finished overlay must visibly transmit the broad background colors"
                );
                let solid = glass[75 * 480 + 230];
                assert!(
                    (solid & 255) > 230 && ((solid >> 16) & 255) > 240,
                    "foreground labels must stay solid while the background is transparent"
                );
                assert_eq!(frame.alpha_at(80, 100), 89);
                assert_eq!(frame.alpha_at(230, 75), 255);
                if let Some(directory) = std::env::var_os("CODEX_OVERLAY_RENDER_DIR") {
                    let mut bytes = b"P6\n480 280\n255\n".to_vec();
                    for pixel in glass {
                        bytes.extend_from_slice(&[
                            pixel as u8,
                            (pixel >> 8) as u8,
                            (pixel >> 16) as u8,
                        ]);
                    }
                    std::fs::write(
                        std::path::Path::new(&directory).join("finished-glass.ppm"),
                        bytes,
                    )
                    .unwrap();
                }
                println!(
                    "Finished glass: 35% tint, live background colors visible, solid foreground verified"
                );
            }

            // Only the panel and tab should blur; the gutter must stay clear.
            let split = DockLayout {
                panel: Some(Rect {
                    left: 0,
                    top: 0,
                    right: 180,
                    bottom: 280,
                }),
                tab: Some(Rect {
                    left: 400,
                    top: 0,
                    right: 480,
                    bottom: 42,
                }),
                ..layout
            };
            backdrop.present(-1isize as Hwnd, split, 96).unwrap();
            pump();
            let clipped = capture();
            let a = clipped[80 * 480 + 220];
            let b = clipped[80 * 480 + 228];
            let gutter_detail: i32 = [0, 8, 16]
                .into_iter()
                .map(|shift| (((a >> shift) & 255) as i32 - ((b >> shift) & 255) as i32).abs())
                .sum();
            assert!(
                gutter_detail > 150,
                "transparent gutter must expose the unblurred fixture: {gutter_detail}"
            );
            assert_ne!(GetForegroundWindow(), backdrop.panel.window);
            backdrop.hide();
            let handle = backdrop.panel.window;
            drop(backdrop);
            assert_eq!(
                IsWindow(handle),
                0,
                "the blur window must be destroyed with its owner"
            );
        }
    }
}
