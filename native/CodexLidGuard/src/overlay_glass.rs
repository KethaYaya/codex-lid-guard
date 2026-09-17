//! Static glass lighting painted into the overlay's existing cached surfaces.
//! Transparency comes from the layered window; no backdrop capture or blur pass.
use super::*;

#[repr(C)]
struct Vertex {
    x: i32,
    y: i32,
    red: u16,
    green: u16,
    blue: u16,
    alpha: u16,
}

impl Vertex {
    fn new(x: i32, y: i32, red: u8, green: u8, blue: u8) -> Self {
        Self {
            x,
            y,
            red: u16::from(red) << 8,
            green: u16::from(green) << 8,
            blue: u16::from(blue) << 8,
            alpha: 0,
        }
    }
}

#[link(name = "msimg32")]
unsafe extern "system" {
    fn GradientFill(
        dc: Handle,
        vertices: *const Vertex,
        count: u32,
        mesh: *const u32,
        mesh_count: u32,
        mode: u32,
    ) -> Bool;
}

pub(super) unsafe fn surface(dc: Handle, rect: Rect) {
    unsafe {
        let vertices = [
            Vertex::new(rect.left, rect.top, 55, 71, 90),
            Vertex::new(rect.right, rect.top, 38, 50, 66),
            Vertex::new(rect.left, rect.bottom, 27, 37, 51),
            Vertex::new(rect.right, rect.bottom, 22, 30, 43),
        ];
        // Two triangles give the surface a quiet diagonal reflection.
        let triangles = [0, 1, 2, 1, 3, 2];
        if GradientFill(dc, vertices.as_ptr(), 4, triangles.as_ptr(), 2, 2) == 0 {
            fill_rectangle(dc, &rect, color_ref(30, 40, 54));
        }
    }
}

pub(super) unsafe fn rim(dc: Handle, rect: Rect, radius: i32) {
    unsafe {
        let saved = SaveDC(dc);
        if saved == 0 {
            return;
        }
        let old_brush = SelectObject(dc, GetStockObject(5)); // NULL_BRUSH
        for (upper, color) in [
            (false, color_ref(61, 79, 100)),
            (true, color_ref(103, 124, 147)),
        ] {
            let pen = CreatePen(0, 1, color);
            if pen.is_null() {
                continue;
            }
            let old_pen = SelectObject(dc, pen);
            if upper {
                IntersectClipRect(
                    dc,
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.top + (rect.bottom - rect.top) / 2,
                );
            }
            RoundRect(
                dc,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                radius * 2,
                radius * 2,
            );
            SelectObject(dc, old_pen);
            DeleteObject(pen);
        }
        SelectObject(dc, old_brush);
        RestoreDC(dc, saved);
    }
}

pub(super) unsafe fn plate(dc: Handle, rect: Rect, dpi: u32) {
    unsafe {
        let radius = scale_dip(5, dpi);
        fill_rounded_rectangle(dc, &rect, color_ref(48, 64, 83), radius);
        rim(dc, rect, radius);
    }
}
