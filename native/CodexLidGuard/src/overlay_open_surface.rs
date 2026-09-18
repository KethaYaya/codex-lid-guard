//! Composited frames for sliding panels and frozen editor-restore images.
//! Only our own client pixels are copied; no editor/desktop capture is needed.
use super::*;

#[repr(C)]
struct BitmapInfo {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bits: u16,
    compression: u32,
    image_size: u32,
    x_resolution: i32,
    y_resolution: i32,
    colors: u32,
    important: u32,
    color: u32,
}

#[repr(C)]
struct Size {
    width: i32,
    height: i32,
}
#[repr(C)]
struct Blend {
    operation: u8,
    flags: u8,
    alpha: u8,
    format: u8,
}
impl Blend {
    fn alpha(alpha: u8) -> Self {
        Self {
            operation: 0,
            flags: 0,
            alpha,
            format: 1,
        }
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn UpdateLayeredWindow(
        window: Hwnd,
        screen: Handle,
        position: *const Point,
        size: *const Size,
        source: Handle,
        origin: *const Point,
        key: u32,
        blend: *const Blend,
        flags: u32,
    ) -> Bool;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateDIBSection(
        dc: Handle,
        info: *const BitmapInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: Handle,
        offset: u32,
    ) -> Handle;
    fn FillRgn(dc: Handle, region: Handle, brush: Handle) -> Bool;
    fn GdiFlush() -> Bool;
    fn SelectClipRgn(dc: Handle, region: Handle) -> i32;
}
#[link(name = "msimg32")]
unsafe extern "system" {
    fn AlphaBlend(
        destination: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        source: Handle,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        blend: Blend,
    ) -> Bool;
}

struct Surface {
    dc: Handle,
    bitmap: Handle,
    original: Handle,
    bits: *mut u32,
    width: i32,
    height: i32,
}
impl Surface {
    unsafe fn new(width: i32, height: i32) -> io::Result<Self> {
        unsafe {
            let info = BitmapInfo {
                size: 40,
                width,
                height: -height,
                planes: 1,
                bits: 32,
                compression: 0,
                image_size: 0,
                x_resolution: 0,
                y_resolution: 0,
                colors: 0,
                important: 0,
                color: 0,
            };
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() {
                return Err(error("Create restore surface DC"));
            }
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(dc, &info, 0, &mut bits, null_mut(), 0);
            if bitmap.is_null() {
                DeleteDC(dc);
                return Err(error("Create restore surface bitmap"));
            }
            let original = SelectObject(dc, bitmap);
            Ok(Self {
                dc,
                bitmap,
                original,
                bits: bits.cast(),
                width,
                height,
            })
        }
    }
    unsafe fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.width * self.height) as usize) }
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.original);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

pub(super) struct OpenSurface {
    source: Surface,
    output: Surface,
    last: Option<(Rect, u8)>,
}
impl OpenSurface {
    pub(super) unsafe fn capture(
        cached: Handle,
        layout: DockLayout,
        dpi: u32,
        to: Rect,
        preserve_alpha: bool,
    ) -> io::Result<Self> {
        unsafe {
            let width = layout.window.right - layout.window.left;
            let height = layout.window.bottom - layout.window.top;
            let mut source = Surface::new(width, height)?;
            let mut output = Surface::new(
                width.max(to.right - to.left),
                height.max(to.bottom - to.top),
            )?;
            output.pixels().fill(0);
            // The shared shape also works after a hover slide uses per-pixel alpha.
            let region = create_overlay_region(layout, dpi)?;
            let brush = CreateSolidBrush(0x00ff_ffff);
            let mask_result = if !brush.is_null() {
                FillRgn(output.dc, region, brush)
            } else {
                0
            };
            if !brush.is_null() {
                DeleteObject(brush);
            }
            DeleteObject(region);
            if mask_result == 0 {
                return Err(error("Copy restore shape"));
            }
            if BitBlt(source.dc, 0, 0, width, height, cached, 0, 0, 0x00cc0020) == 0 {
                return Err(error("Copy visible overlay"));
            }
            GdiFlush(); // Finish GDI writes before touching DIB memory.
            let stride = output.width as usize;
            let mask = output.pixels();
            for (index, pixel) in source.pixels().iter_mut().enumerate() {
                *pixel = if mask[(index / width as usize) * stride + index % width as usize] != 0 {
                    if preserve_alpha {
                        *pixel
                    } else {
                        *pixel | 0xff00_0000
                    }
                } else {
                    0
                };
            }
            output.pixels().fill(0);
            Ok(Self {
                source,
                output,
                last: None,
            })
        }
    }

    pub(super) unsafe fn present(
        &mut self,
        window: Hwnd,
        bounds: Rect,
        alpha: u8,
    ) -> io::Result<()> {
        unsafe {
            if self.last == Some((bounds, alpha)) {
                return Ok(());
            }
            let width = bounds.right - bounds.left;
            let height = bounds.bottom - bounds.top;
            if width <= 0 || height <= 0 || width > self.output.width || height > self.output.height
            {
                return Err(io::Error::other(
                    "Restore frame exceeds its preallocated surface",
                ));
            }
            GdiFlush();
            self.output.pixels().fill(0);
            if AlphaBlend(
                self.output.dc,
                0,
                0,
                width,
                height,
                self.source.dc,
                0,
                0,
                self.source.width,
                self.source.height,
                Blend::alpha(255),
            ) == 0
            {
                return Err(error("Scale restore surface"));
            }
            let position = Point {
                x: bounds.left,
                y: bounds.top,
            };
            let size = Size { width, height };
            if UpdateLayeredWindow(
                window,
                null_mut(),
                &position,
                &size,
                self.output.dc,
                &Point { x: 0, y: 0 },
                0,
                &Blend::alpha(alpha),
                2,
            ) == 0
            {
                return Err(error("Present restore frame"));
            }
            self.last = Some((bounds, alpha));
            Ok(())
        }
    }
}

// Present each visible slice at its native pixel size. In particular, this never
// scales a narrow previous window image to the newly exposed client width.
pub(super) struct GrowSurface {
    source: Surface,
    output: Surface,
    header: i32,
    footer: i32,
    dpi: u32,
}

impl GrowSurface {
    pub(super) unsafe fn capture(cached: Handle, width: i32, height: i32, header: i32, footer: i32, dpi: u32, from_size: (i32, i32)) -> io::Result<Self> {
        unsafe {
            let source = Surface::new(width, height)?;
            let output = Surface::new(width.max(from_size.0), height.max(from_size.1))?;
            if BitBlt(source.dc, 0, 0, width, height, cached, 0, 0, 0x00cc0020) == 0 {
                return Err(error("Cache growing overlay"));
            }
            Ok(Self { source, output, header, footer, dpi })
        }
    }

    pub(super) fn cached_dc(&self) -> Handle { self.output.dc }

    pub(super) unsafe fn include_input(&mut self, rect: Rect, paint: impl FnOnce(Handle)) -> io::Result<()> {
        unsafe {
            let width = rect.right-rect.left;
            let height = rect.bottom-rect.top;
            let mut input = Surface::new(width, height)?;
            fill_rectangle(input.dc, &Rect { left: 0, top: 0, right: width, bottom: height }, color_ref(28, 37, 51));
            paint(input.dc);
            GdiFlush();
            for pixel in input.pixels() { *pixel |= 0xff00_0000; }
            let radius = scale_dip(8, self.dpi);
            let clip = CreateRoundRectRgn(rect.left, rect.top, rect.right+1, rect.bottom+1, radius, radius);
            if clip.is_null() { return Err(error("Cache rounded reply input")); }
            let saved = SaveDC(self.source.dc);
            SelectClipRgn(self.source.dc, clip);
            let copied = BitBlt(self.source.dc, rect.left, rect.top, width, height, input.dc, 0, 0, 0x00cc0020);
            RestoreDC(self.source.dc, saved);
            DeleteObject(clip);
            if copied == 0 { return Err(error("Copy cached reply input")); }
            Ok(())
        }
    }

    unsafe fn compose(&mut self, width: i32, height: i32) -> io::Result<()> {
        unsafe {
            if width <= 0 || height <= 0 || width > self.output.width || height > self.output.height {
                return Err(io::Error::other("Growth frame exceeds its cached layout"));
            }
            if width > self.source.width || height > self.source.height {
                // A shorter reply, smaller monitor, or wide full-chat layout can
                // shrink one dimension. Fill the extra space with cached glass,
                // while keeping the reply and footer at their native pixel size.
                GdiFlush();
                let background = *self.source.bits.add((self.header.min(self.source.height-1) * self.source.width
                    + scale_dip(8, self.dpi).min(self.source.width-1)) as usize);
                self.output.pixels().fill(background);
            }
            let header = self.header.min(height);
            let footer = self.footer.min(height-header);
            // Copy cached pixels, rather than reflowing/repainting glass and text.
            // Left text stays anchored; status, scrollbars and buttons stay on the right.
            for (y, sy, band_height, right) in [
                (0, 0, header, scale_dip(160, self.dpi)),
                (header, self.header, (height-header-footer).min((self.source.height-self.header-self.footer).max(0)), scale_dip(18, self.dpi)),
                (height-footer, self.source.height-footer, footer, scale_dip(200, self.dpi)),
            ] {
                let right = right.min(width).min(self.source.width);
                for (x, sx, band_width) in [(0, 0, (width-right).min(self.source.width-right)), (width-right, self.source.width-right, right)] {
                    if band_height > 0 && band_width > 0
                        && BitBlt(self.output.dc, x, y, band_width, band_height, self.source.dc, sx, sy, 0x00cc0020) == 0 {
                        return Err(error("Copy growing overlay section"));
                    }
                }
            }
            Ok(())
        }
    }

    pub(super) unsafe fn present(&mut self, window: Hwnd, bounds: Rect, alpha: u8) -> io::Result<()> {
        unsafe {
            #[cfg(test)]
            let profile = Instant::now();
            let width = bounds.right-bounds.left;
            let height = bounds.bottom-bounds.top;
            self.compose(width, height)?;
            if UpdateLayeredWindow(window, null_mut(), &Point { x: bounds.left, y: bounds.top }, &Size { width, height },
                self.output.dc, &Point { x: 0, y: 0 }, 0, &Blend::alpha(alpha), 2) == 0 {
                return Err(error("Present growing overlay"));
            }
            #[cfg(test)]
            if std::env::var_os("LIDGUARD_PROFILE").is_some() {
                eprintln!("cached growth {width}x{height}: {}us", profile.elapsed().as_micros());
            }
            Ok(())
        }
    }
}

pub(super) struct FrameSurface {
    image: Surface,
    mask: Surface,
    shape: Option<(DockLayout, u32)>,
    coverage: Option<(Surface, Surface)>,
}
impl FrameSurface {
    pub(super) unsafe fn reserve(&mut self, width: i32, height: i32) -> io::Result<()> {
        unsafe {
            if width > self.image.width || height > self.image.height {
                *self = Self::new(width.max(self.image.width), height.max(self.image.height))?;
            }
            Ok(())
        }
    }
    pub(super) fn cached_dc(&self) -> Handle {
        self.image.dc
    }
    #[cfg(test)]
    pub(super) fn alpha_at(&self, x: i32, y: i32) -> u8 {
        let Some((layout, _)) = self.shape else {
            return 0;
        };
        if x < 0
            || y < 0
            || x >= layout.window.right - layout.window.left
            || y >= layout.window.bottom - layout.window.top
        {
            return 0;
        }
        unsafe { (*self.image.bits.add((y * self.image.width + x) as usize) >> 24) as u8 }
    }

    pub(super) unsafe fn new(width: i32, height: i32) -> io::Result<Self> {
        unsafe {
            Ok(Self {
                image: Surface::new(width, height)?,
                mask: Surface::new(width, height)?,
                shape: None,
                coverage: None,
            })
        }
    }

    pub(super) unsafe fn present(
        &mut self,
        window: Hwnd,
        cached: Handle,
        layout: DockLayout,
        dpi: u32,
        alpha: u8,
    ) -> io::Result<()> {
        unsafe { self.present_material(window, cached, layout, dpi, alpha, None) }
    }

    pub(super) unsafe fn present_material(
        &mut self,
        window: Hwnd,
        cached: Handle,
        layout: DockLayout,
        dpi: u32,
        alpha: u8,
        material: Option<([Handle; 2], u8)>,
    ) -> io::Result<()> {
        unsafe {
            self.cache_material(cached, layout, dpi, material)?;
            self.present_cached(window, layout, alpha)
        }
    }

    pub(super) unsafe fn cache_material(
        &mut self, cached: Handle, layout: DockLayout, dpi: u32, material: Option<([Handle; 2], u8)>,
    ) -> io::Result<()> {
        unsafe {
            let bounds = layout.window;
            let width = bounds.right - bounds.left;
            let height = bounds.bottom - bounds.top;
            self.reserve(width, height)?;
            if self.shape != Some((layout, dpi)) {
                GdiFlush();
                self.mask.pixels().fill(0);
                let region = create_overlay_region(layout, dpi)?;
                let brush = CreateSolidBrush(0x00ff_ffff);
                let result = if brush.is_null() {
                    0
                } else {
                    FillRgn(self.mask.dc, region, brush)
                };
                if !brush.is_null() {
                    DeleteObject(brush);
                }
                DeleteObject(region);
                if result == 0 {
                    return Err(error("Draw overlay frame mask"));
                }
                self.shape = Some((layout, dpi));
            }
            if BitBlt(self.image.dc, 0, 0, width, height, cached, 0, 0, 0x00cc0020) == 0 {
                return Err(error("Copy overlay frame pixels"));
            }
            if let Some((samples, _)) = material {
                if self.coverage.is_none() {
                    self.coverage = Some((
                        Surface::new(self.image.width, self.image.height)?,
                        Surface::new(self.image.width, self.image.height)?,
                    ));
                }
                let (black, white) = self.coverage.as_mut().unwrap();
                for (surface, source) in [(black, samples[0]), (white, samples[1])] {
                    if BitBlt(surface.dc, 0, 0, width, height, source, 0, 0, 0x00cc0020) == 0 {
                        return Err(error("Copy glass foreground coverage"));
                    }
                }
            }
            GdiFlush();
            let coverage = self
                .coverage
                .as_mut()
                .map(|(black, white)| (black.pixels(), white.pixels()));
            let stride = self.image.width as usize;
            let mask = self.mask.pixels();
            let pixels = self.image.pixels();
            for y in 0..height as usize {
                let start = y * stride;
                for x in start..start + width as usize {
                    pixels[x] = if mask[x] != 0 {
                        if let Some((_, tint)) = material {
                            let (black, white) = coverage.as_ref().unwrap();
                            glass_pixel(pixels[x], black[x], white[x], tint)
                        } else {
                            pixels[x] | 0xff00_0000
                        }
                    } else {
                        0
                    };
                }
            }
            Ok(())
        }
    }

    pub(super) unsafe fn present_cached(&self, window: Hwnd, layout: DockLayout, alpha: u8) -> io::Result<()> {
        unsafe {
            let bounds = layout.window;
            let width = bounds.right-bounds.left;
            let height = bounds.bottom-bounds.top;
            if UpdateLayeredWindow(
                window,
                null_mut(),
                &Point {
                    x: bounds.left,
                    y: bounds.top,
                },
                &Size { width, height },
                self.image.dc,
                &Point { x: 0, y: 0 },
                0,
                &Blend::alpha(alpha),
                2,
            ) == 0
            {
                return Err(error("Present overlay slide frame"));
            }
            Ok(())
        }
    }
}

// GDI's black/white coverage pair preserves fully opaque glyphs and their
// antialiased edges. Only the background contribution receives the tint alpha.
fn glass_pixel(color: u32, black: u32, white: u32, tint: u8) -> u32 {
    let channel = |pixel: u32, shift: u32| ((pixel >> shift) & 255u32) as i32;
    let transparent = [0, 8, 16]
        .into_iter()
        .map(|shift| (channel(white, shift) - channel(black, shift)).clamp(0, 255))
        .min()
        .unwrap();
    let alpha = 255 - transparent + (transparent * i32::from(tint) + 127) / 255;
    [0, 8, 16]
        .into_iter()
        .fold((alpha as u32) << 24, |pixel, shift| {
            let foreground = channel(black, shift);
            let background = channel(color, shift) - foreground;
            let premultiplied =
                (foreground + (background * i32::from(tint) + 127) / 255).clamp(0, alpha);
            pixel | ((premultiplied as u32) << shift)
        })
}

// SetLayeredWindowAttributes and UpdateLayeredWindow use different presentation
// modes. Reset once at each handoff, never during the animation's frame loop.
pub(super) unsafe fn reset_layered_mode(window: Hwnd) {
    unsafe {
        let style = GetWindowLongPtrW(window, -20);
        SetWindowLongPtrW(window, -20, style & !(WS_EX_LAYERED as isize));
        SetWindowLongPtrW(window, -20, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_keeps_native_pixels_alpha_and_edge_controls_without_allocating_frames() {
        unsafe {
            for dpi in [96, 144, 192] {
                let d = |value| scale_dip(value, dpi);
                let (width, height) = (d(780), d(680));
                let mut cached = Surface::new(width, height).unwrap();
                for (i, pixel) in cached.pixels().iter_mut().enumerate() {
                    *pixel = (i as u32).wrapping_mul(7919);
                }
                let mut growth = GrowSurface::capture(cached.dc, width, height, d(94), d(62), dpi, (width, height)).unwrap();
                let buffers = (growth.output.dc, growth.output.bitmap);
                for (w, h) in [(d(344), d(166)), (d(520), d(400)), (width, height)] {
                    growth.compose(w, h).unwrap();
                    GdiFlush();
                    let pixels = growth.output.pixels();
                    let source = cached.pixels();
                    // Distinct points on text, right controls, and both lower corners
                    // must retain their original color and alpha at native scale.
                    for (x, y, sx, sy) in [
                        (0, 0, 0, 0),
                        (w-1, 0, width-1, 0),
                        (d(10), d(95), d(10), d(95)),
                        (w-1, d(95), width-1, d(95)),
                        (0, h-1, 0, height-1),
                        (w-1, h-1, width-1, height-1),
                        (w-d(184), h-d(20), width-d(184), height-d(20)),
                    ] {
                        assert_eq!(pixels[(y*width+x) as usize], source[(sy*width+sx) as usize]);
                    }
                    if (w, h) == (width, height) {
                        assert_eq!(pixels, source, "final frame must exactly match the live full-size image");
                    }
                    assert_eq!(buffers, (growth.output.dc, growth.output.bitmap));
                }
            }
        }
    }

    #[test]
    fn shorter_replies_and_tall_previews_can_resize_without_exceeding_the_cached_frame() {
        unsafe {
            for dpi in [96, 144, 192] {
                let d = |value| scale_dip(value, dpi);
                for (width, height, from_width, from_height) in [(344, 240, 344, 900), (780, 680, 344, 900), (344, 240, 780, 680)] {
                    let (width, height, from_width, from_height) = (d(width), d(height), d(from_width), d(from_height));
                    let mut cached = Surface::new(width, height).unwrap();
                    for (i, pixel) in cached.pixels().iter_mut().enumerate() { *pixel = (i as u32).wrapping_mul(7919); }
                    let mut frame = GrowSurface::capture(cached.dc, width, height, d(94), d(62), dpi, (from_width, from_height)).unwrap();
                    let buffers = (frame.output.dc, frame.output.bitmap);
                    let stride = frame.output.width;
                    for step in 0..=20 {
                        let w = from_width + (width-from_width)*step/20;
                        let h = from_height + (height-from_height)*step/20;
                        frame.compose(w, h).unwrap();
                        GdiFlush();
                        let output = frame.output.pixels();
                        let source = cached.pixels();
                        assert_eq!(output[0], source[0]);
                        assert_eq!(output[((h-1)*stride+w-1) as usize], source[(height*width-1) as usize]);
                        assert_eq!(output[((h-d(20))*stride+w-d(100)) as usize], source[((height-d(20))*width+width-d(100)) as usize]);
                        if h > height {
                            assert_eq!(output[((h-d(62)-1)*stride) as usize], source[(d(94)*width+d(8)) as usize], "extra height must contain cached glass, not stale text");
                        }
                        if step == 20 {
                            for y in 0..height {
                                assert_eq!(&output[(y*stride) as usize..(y*stride+width) as usize], &source[(y*width) as usize..((y+1)*width) as usize]);
                            }
                        }
                        assert_eq!(buffers, (frame.output.dc, frame.output.bitmap));
                    }
                }
            }
        }
    }

    #[test]
    fn glass_keeps_solid_labels_and_shortcuts_opaque_while_reducing_background() {
        let tint = 89; // 35% background, independently of foreground coverage.
        assert_eq!(glass_pixel(0x1e2836, 0, 0xffffff, tint) >> 24, 89);
        for ink in [0xf2f7fd, 0xbfccdc, 0xa4b5c9, 0xcab98b, 0x86d5a9, 0xf1ba75] {
            assert_eq!(glass_pixel(ink, ink, ink, tint), 0xff000000 | ink);
        }
        // A half-covered white edge over the dark tint remains premultiplied.
        let edge = glass_pixel(0x8f939b, 0x808080, 0xffffff, tint);
        assert!((171..=173).contains(&(edge >> 24)));
        for shift in [0, 8, 16] {
            assert!(((edge >> shift) & 255) <= (edge >> 24));
        }
        assert_eq!(glass_pixel(0x8f939b, 0x808080, 0xffffff, 255), 0xff8f939b);
    }

    #[test]
    fn native_slide_frames_preserve_pixel_scale_and_fill_the_right_edge_without_a_second_resize() {
        unsafe {
            SetThreadDpiAwarenessContext(-4isize as Handle);
            let window = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                wide("STATIC").as_ptr(),
                wide("Owned slide pixels test").as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null_mut(),
            );
            assert!(!window.is_null());
            let result = std::panic::catch_unwind(|| {
                let work = Rect {
                    left: -1920,
                    top: 0,
                    right: 0,
                    bottom: 1080,
                };
                let mut timings = Vec::new();
                for dpi in [96, 144, 192] {
                    let expanded = overlay_bounds(
                        work,
                        scale_dip(440, dpi),
                        scale_dip(200, dpi),
                        scale_dip(20, dpi),
                        "bottom-right",
                    );
                    let full_width = scale_dip(468, dpi);
                    let full_height = scale_dip(200, dpi);
                    let mut cached = Surface::new(full_width, full_height).unwrap();
                    let mut frame = FrameSurface::new(full_width, full_height).unwrap();
                    let allocations = (frame.image.bitmap, frame.mask.bitmap);
                    reset_layered_mode(window);
                    // Expand, reverse, tuck, and expand again; include the exact final frame.
                    for p in [
                        1.0, 0.95, 0.75, 0.4, 0.1, 0.01, 0.0, 0.01, 0.3, 0.6, 0.9, 1.0, 0.5, 0.0,
                    ] {
                        let layout = dock_layout(expanded, work, p, dpi, None);
                        let width = layout.window.right - layout.window.left;
                        let height = layout.window.bottom - layout.window.top;
                        cached.pixels().fill(0x0044352c);
                        if let Some(panel) = layout.panel {
                            assert_eq!(panel.right - panel.left, scale_dip(440, dpi));
                            // Fixed-size vertical bars expose any accidental stretch/crop.
                            for y in panel.top..panel.bottom {
                                for x in panel.left..panel.right.min(full_width) {
                                    cached.pixels()[(y * full_width + x) as usize] =
                                        0x00102000 | ((x - panel.left) as u32 % 251);
                                }
                            }
                        }
                        let before = Instant::now();
                        frame.present(window, cached.dc, layout, dpi, 209).unwrap();
                        timings.push(before.elapsed().as_secs_f64() * 1000.0);
                        let mut actual: Rect = zeroed();
                        assert_ne!(GetWindowRect(window, &mut actual), 0);
                        assert_eq!(actual, layout.window);
                        assert_eq!(actual.right, work.right);
                        assert_eq!((frame.image.bitmap, frame.mask.bitmap), allocations);
                        if let Some(panel) = layout.panel {
                            let y = (panel.top + panel.bottom) / 2;
                            for x in panel.left..width {
                                let pixel = *frame.image.bits.add((y * full_width + x) as usize);
                                assert_eq!(
                                    pixel,
                                    0xff102000 | ((x - panel.left) as u32 % 251),
                                    "the cached message must slide at its original pixel scale"
                                );
                            }
                            for y in 0..height {
                                assert_eq!(
                                    frame.alpha_at(width - 1, y),
                                    255,
                                    "the right edge, including corners, stays filled throughout the slide"
                                );
                            }
                        }
                    }
                }
                timings.sort_by(f64::total_cmp);
                println!(
                    "Slide frame presentation (42 frames, 100/150/200% DPI): median={:.3}ms p95={:.3}ms max={:.3}ms",
                    timings[timings.len() / 2],
                    timings[timings.len() * 95 / 100],
                    timings[timings.len() - 1]
                );
            });
            DestroyWindow(window);
            if let Err(cause) = result {
                std::panic::resume_unwind(cause);
            }
        }
    }

    #[test]
    fn native_restore_surface_preserves_pixels_and_shape_and_reuses_buffers() {
        unsafe {
            SetThreadDpiAwarenessContext(-4isize as Handle);
            let window = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                wide("STATIC").as_ptr(),
                wide("Owned restore surface test").as_ptr(),
                WS_POPUP,
                0,
                0,
                800,
                600,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null_mut(),
            );
            assert!(!window.is_null());
            let result = std::panic::catch_unwind(|| {
                let work = Rect {
                    left: 0,
                    top: 0,
                    right: 1920,
                    bottom: 1040,
                };
                let mut timings = Vec::new();
                // Cover a tab, partly expanded hover, and an expanded message at 100/200% DPI.
                for dpi in [96, 192] {
                    let expanded = overlay_bounds(
                        work,
                        scale_dip(440, dpi),
                        scale_dip(200, dpi),
                        scale_dip(20, dpi),
                        "bottom-right",
                    );
                    for slide in [1.0, 0.45, 0.0] {
                        let layout = dock_layout(expanded, work, slide, dpi, None);
                        let width = layout.window.right - layout.window.left;
                        let height = layout.window.bottom - layout.window.top;
                        SetWindowPos(
                            window,
                            null_mut(),
                            layout.window.left,
                            layout.window.top,
                            width,
                            height,
                            SWP_NOACTIVATE | 0x0004,
                        );
                        apply_overlay_region(window, layout, dpi).unwrap();
                        let to = opening_target(layout.window, work, dpi);
                        let mut cached = Surface::new(width, height).unwrap();
                        // A unique source value at each pixel detects crop/source substitution.
                        for (index, pixel) in cached.pixels().iter_mut().enumerate() {
                            *pixel = (index as u32 * 7919) & 0x00ff_ffff;
                        }
                        let mut surface =
                            OpenSurface::capture(cached.dc, layout, dpi, to, false).unwrap();
                        let allocations = (
                            surface.source.dc,
                            surface.source.bitmap,
                            surface.output.dc,
                            surface.output.bitmap,
                        );
                        reset_layered_mode(window);
                        SetWindowRgn(window, null_mut(), 0);
                        surface.present(window, layout.window, 208).unwrap();
                        GdiFlush();
                        let output_stride = surface.output.width as usize;
                        let output = surface.output.pixels();
                        let source = surface.source.pixels();
                        assert!(source.contains(&0), "rounded corners stay transparent");
                        let mut opaque = 0;
                        for (index, &pixel) in source.iter().enumerate() {
                            assert_eq!(
                                pixel,
                                output[index / width as usize * output_stride
                                    + index % width as usize],
                                "first composed frame must match the captured pixels exactly"
                            );
                            if pixel != 0 {
                                opaque += 1;
                                assert_eq!(pixel & 0x00ff_ffff, cached.pixels()[index]);
                            }
                        }
                        assert!(opaque > (width * height / 3) as usize);
                        for frame in 1..=12 {
                            let before = Instant::now();
                            let p = frame as f32 / 12.0;
                            let bounds = opening_bounds(layout.window, to, p);
                            surface
                                .present(window, bounds, ((1.0 - p).powi(2) * 208.0) as u8)
                                .unwrap();
                            timings.push(before.elapsed().as_secs_f64() * 1000.0);
                            assert_eq!(
                                allocations,
                                (
                                    surface.source.dc,
                                    surface.source.bitmap,
                                    surface.output.dc,
                                    surface.output.bitmap
                                ),
                                "no bitmap allocation per frame"
                            );
                        }
                        reset_layered_mode(window);
                        assert_ne!(SetLayeredWindowAttributes(window, 0, 208, LWA_ALPHA), 0);
                        apply_overlay_region(window, layout, dpi).unwrap();
                    }
                }
                timings.sort_by(f64::total_cmp);
                println!(
                    "Atomic restore presentation (72 frames, up to 200% DPI): median={:.3}ms p95={:.3}ms max={:.3}ms",
                    timings[timings.len() / 2],
                    timings[timings.len() * 95 / 100],
                    timings[timings.len() - 1]
                );
            });
            DestroyWindow(window);
            if let Err(cause) = result {
                std::panic::resume_unwind(cause);
            }
        }
    }
}
