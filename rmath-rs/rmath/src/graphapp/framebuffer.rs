#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! In-memory framebuffer for headless rendering.
//!
//! Provides a simple RGBA pixel buffer that can be attached to
//! window objects and drawing objects. All rendering operations
//! write to this buffer rather than to a physical display.
//!
//! This enables the GraphApp and X11 graphics pipelines to function
//! correctly on headless systems (servers, hospital environments, CI).

use std::os::raw::{c_int, c_ulong, c_void};
use std::ptr;

use super::memory;
use super::types::*;

/// RGBA pixel stored as 4 bytes (R, G, B, A).
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Pixel {
    pub const WHITE: Pixel = Pixel {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const BLACK: Pixel = Pixel {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const TRANSPARENT: Pixel = Pixel {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// Create a pixel from a GraphApp rgb value (0x00RRGGBB).
    #[inline]
    pub fn from_rgb(c: rgb) -> Pixel {
        Pixel {
            r: getred(c) as u8,
            g: getgreen(c) as u8,
            b: getblue(c) as u8,
            a: if getalpha(c) > 0x7F { 0 } else { 255 },
        }
    }

    /// Convert to a GraphApp rgb value (0x00RRGGBB).
    #[inline]
    pub fn to_rgb(self) -> rgb {
        if self.a == 0 {
            Transparent
        } else {
            rgb_make(self.r as c_ulong, self.g as c_ulong, self.b as c_ulong)
        }
    }
}

/// Framebuffer: an in-memory pixel buffer.
///
/// Stored as an opaque pointer in the `handle` field of ObjInfo,
/// so windows and drawing surfaces can share the same backing store.
pub struct Framebuffer {
    pub width: c_int,
    pub height: c_int,
    pub stride: usize, // bytes per row (width * 4)
    pub pixels: Vec<Pixel>,
    /// Clipping rectangle (pixels outside this are not drawn).
    pub clip_x: c_int,
    pub clip_y: c_int,
    pub clip_w: c_int,
    pub clip_h: c_int,
}

impl Framebuffer {
    /// Create a new framebuffer of the given size, filled with white.
    pub fn new(width: c_int, height: c_int) -> Framebuffer {
        let w = width.max(1) as usize;
        let h = height.max(1) as usize;
        let mut fb = Framebuffer {
            width,
            height,
            stride: w * 4,
            pixels: vec![Pixel::WHITE; w * h],
            clip_x: 0,
            clip_y: 0,
            clip_w: width,
            clip_h: height,
        };
        fb.reset_clip();
        fb
    }

    /// Reset clip rectangle to full framebuffer.
    pub fn reset_clip(&mut self) {
        self.clip_x = 0;
        self.clip_y = 0;
        self.clip_w = self.width;
        self.clip_h = self.height;
    }

    /// Set the clip rectangle.
    pub fn set_clip(&mut self, x: c_int, y: c_int, w: c_int, h: c_int) {
        self.clip_x = x;
        self.clip_y = y;
        self.clip_w = w;
        self.clip_h = h;
    }

    /// Check if a pixel coordinate is within bounds and clip rect.
    #[inline]
    pub fn in_bounds(&self, x: c_int, y: c_int) -> bool {
        x >= self.clip_x
            && x < self.clip_x + self.clip_w
            && y >= self.clip_y
            && y < self.clip_y + self.clip_h
            && x >= 0
            && x < self.width
            && y >= 0
            && y < self.height
    }

    /// Get pixel at (x, y). Returns Transparent if out of bounds.
    #[inline]
    pub fn get_pixel(&self, x: c_int, y: c_int) -> Pixel {
        if x < 0 || x >= self.width || y < 0 || y >= self.height {
            return Pixel::TRANSPARENT;
        }
        self.pixels[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// Set pixel at (x, y). Does nothing if out of bounds.
    #[inline]
    pub fn set_pixel(&mut self, x: c_int, y: c_int, p: Pixel) {
        if self.in_bounds(x, y) {
            self.pixels[(y as usize) * (self.width as usize) + (x as usize)] = p;
        }
    }

    /// Set pixel using an rgb value (ignoring alpha > 0x7F = transparent).
    #[inline]
    pub fn set_pixel_rgb(&mut self, x: c_int, y: c_int, c: rgb) {
        let p = Pixel::from_rgb(c);
        self.set_pixel(x, y, p);
    }

    /// Get pixel as rgb value.
    #[inline]
    pub fn get_pixel_rgb(&self, x: c_int, y: c_int) -> rgb {
        self.get_pixel(x, y).to_rgb()
    }

    /// Fill the entire framebuffer with a colour.
    pub fn fill(&mut self, c: rgb) {
        let p = Pixel::from_rgb(c);
        for px in self.pixels.iter_mut() {
            *px = p;
        }
    }

    /// Fill a rectangle with a colour.
    pub fn fill_rect(&mut self, x: c_int, y: c_int, w: c_int, h: c_int, c: rgb) {
        let p = Pixel::from_rgb(c);
        let x1 = x.max(self.clip_x);
        let y1 = y.max(self.clip_y);
        let x2 = (x + w).min(self.clip_x + self.clip_w);
        let y2 = (y + h).min(self.clip_y + self.clip_h);
        for py in y1..y2 {
            for px in x1..x2 {
                self.pixels[(py as usize) * (self.width as usize) + (px as usize)] = p;
            }
        }
    }

    /// Draw a line using Bresenham's algorithm.
    pub fn draw_line(&mut self, x0: c_int, y0: c_int, x1: c_int, y1: c_int, c: rgb) {
        let p = Pixel::from_rgb(c);
        let mut x = x0;
        let mut y = y0;
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let mut sx = if x0 < x1 { 1 } else { -1 };
        let mut sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            self.set_pixel(x, y, p);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Draw a rectangle outline.
    pub fn draw_rect(&mut self, x: c_int, y: c_int, w: c_int, h: c_int, c: rgb) {
        self.draw_line(x, y, x + w - 1, y, c);
        self.draw_line(x + w - 1, y, x + w - 1, y + h - 1, c);
        self.draw_line(x + w - 1, y + h - 1, x, y + h - 1, c);
        self.draw_line(x, y + h - 1, x, y, c);
    }

    /// Draw an ellipse outline using the midpoint algorithm.
    pub fn draw_ellipse(&mut self, cx: c_int, cy: c_int, rx: c_int, ry: c_int, c: rgb) {
        if rx <= 0 || ry <= 0 {
            return;
        }
        let p = Pixel::from_rgb(c);

        // Use parametric approach for general ellipse
        let steps = ((rx + ry) as f64 * std::f64::consts::PI).ceil() as i32;
        let mut prev_x: Option<c_int> = None;
        let mut prev_y: Option<c_int> = None;

        for i in 0..=steps {
            let theta = 2.0 * std::f64::consts::PI * (i as f64) / (steps as f64);
            let px = cx + (rx as f64 * theta.cos()).round() as c_int;
            let py = cy + (ry as f64 * theta.sin()).round() as c_int;

            if let (Some(ox), Some(oy)) = (prev_x, prev_y) {
                // Draw line from previous point to current to avoid gaps
                self.draw_line(ox, oy, px, py, c);
            } else {
                self.set_pixel(px, py, p);
            }
            prev_x = Some(px);
            prev_y = Some(py);
        }
    }

    /// Fill an ellipse.
    pub fn fill_ellipse(&mut self, cx: c_int, cy: c_int, rx: c_int, ry: c_int, c: rgb) {
        if rx <= 0 || ry <= 0 {
            return;
        }
        let p = Pixel::from_rgb(c);
        let steps = ((rx + ry) as f64 * std::f64::consts::PI).ceil() as i32;
        let mut min_y = cy + ry;
        let mut max_y = cy - ry;

        for i in 0..=steps {
            let theta = 2.0 * std::f64::consts::PI * (i as f64) / (steps as f64);
            let py = cy + (ry as f64 * theta.sin()).round() as c_int;
            let px = cx + (rx as f64 * theta.cos()).round() as c_int;
            if py < min_y {
                min_y = py;
            }
            if py > max_y {
                max_y = py;
            }
        }

        for y in min_y..=max_y {
            // Find x intersections for this scanline
            let dy = (y - cy) as f64;
            if dy.abs() > ry as f64 {
                continue;
            }
            let ratio = 1.0 - (dy * dy) / ((ry * ry) as f64);
            if ratio < 0.0 {
                continue;
            }
            let half_w = ((rx as f64) * ratio.sqrt()).round() as c_int;
            for x in (cx - half_w)..=(cx + half_w) {
                self.set_pixel(x, y, p);
            }
        }
    }

    /// Draw an arc (portion of an ellipse) outline.
    pub fn draw_arc(
        &mut self,
        cx: c_int,
        cy: c_int,
        rx: c_int,
        ry: c_int,
        start_deg: c_int,
        end_deg: c_int,
        c: rgb,
    ) {
        if rx <= 0 || ry <= 0 {
            return;
        }
        let p = Pixel::from_rgb(c);
        let start_rad = (start_deg as f64) * std::f64::consts::PI / 180.0;
        let end_rad = (end_deg as f64) * std::f64::consts::PI / 180.0;
        let steps = ((rx + ry) as f64 * std::f64::consts::PI).ceil() as i32;

        let mut prev_x: Option<c_int> = None;
        let mut prev_y: Option<c_int> = None;

        for i in 0..=steps {
            let theta = start_rad + (end_rad - start_rad) * (i as f64) / (steps as f64);
            let px = cx + (rx as f64 * theta.cos()).round() as c_int;
            let py = cy + (ry as f64 * theta.sin()).round() as c_int;

            if let (Some(ox), Some(oy)) = (prev_x, prev_y) {
                self.draw_line(ox, oy, px, py, c);
            } else {
                self.set_pixel(px, py, p);
            }
            prev_x = Some(px);
            prev_y = Some(py);
        }
    }

    /// Fill an arc (pie slice).
    pub fn fill_arc(
        &mut self,
        cx: c_int,
        cy: c_int,
        rx: c_int,
        ry: c_int,
        start_deg: c_int,
        end_deg: c_int,
        c: rgb,
    ) {
        if rx <= 0 || ry <= 0 {
            return;
        }
        // Fill triangle from center to arc, then fill the arc region
        let start_rad = (start_deg as f64) * std::f64::consts::PI / 180.0;
        let end_rad = (end_deg as f64) * std::f64::consts::PI / 180.0;

        // Draw filled triangle from center
        let x0 = cx + (rx as f64 * start_rad.cos()).round() as c_int;
        let y0 = cy + (ry as f64 * start_rad.sin()).round() as c_int;
        let x1 = cx + (rx as f64 * end_rad.cos()).round() as c_int;
        let y1 = cy + (ry as f64 * end_rad.sin()).round() as c_int;

        // Scan-line fill the pie
        let mut angles = vec![(start_rad, end_rad)];
        if end_rad < start_rad {
            angles = vec![(start_rad, 2.0 * std::f64::consts::PI), (0.0, end_rad)];
        }

        let mut all_y_min = cy;
        let mut all_y_max = cy;
        for &(sa, ea) in &angles {
            for i in 0..=64 {
                let t = sa + (ea - sa) * (i as f64) / 64.0;
                let py = cy + (ry as f64 * t.sin()).round() as c_int;
                if py < all_y_min {
                    all_y_min = py;
                }
                if py > all_y_max {
                    all_y_max = py;
                }
            }
        }

        for y in all_y_min..=all_y_max {
            let dy = (y - cy) as f64;
            let mut x_left = cx as f64;
            let mut x_right = cx as f64;

            for &(sa, ea) in &angles {
                for i in 0..=32 {
                    let t = sa + (ea - sa) * (i as f64) / 32.0;
                    let sy = cy as f64 + (ry as f64) * t.sin();
                    if (sy - y as f64).abs() < 1.0 {
                        let sx = cx as f64 + (rx as f64) * t.cos();
                        x_left = x_left.min(sx);
                        x_right = x_right.max(sx);
                    }
                }
            }

            for x in (x_left.round() as c_int)..=(x_right.round() as c_int) {
                self.set_pixel(x, y, Pixel::from_rgb(c));
            }
        }
    }

    /// Draw a filled polygon using scanline algorithm.
    pub fn fill_polygon(&mut self, pts: &[point], c: rgb) {
        if pts.len() < 3 {
            return;
        }
        let p = Pixel::from_rgb(c);

        // Find y range
        let mut y_min = c_int::MAX;
        let mut y_max = c_int::MIN;
        for pt in pts {
            if pt.y < y_min {
                y_min = pt.y;
            }
            if pt.y > y_max {
                y_max = pt.y;
            }
        }

        // Clamp to clip rect
        y_min = y_min.max(self.clip_y);
        y_max = y_max.min(self.clip_y + self.clip_h - 1);

        let n = pts.len();

        for y in y_min..=y_max {
            // Find intersections with each edge
            let mut intersections: Vec<c_int> = Vec::new();
            for i in 0..n {
                let j = (i + 1) % n;
                let y0 = pts[i].y;
                let y1 = pts[j].y;

                if (y0 <= y && y1 > y) || (y1 <= y && y0 > y) {
                    let t = (y - y0) as f64 / (y1 - y0) as f64;
                    let x_intersect =
                        (pts[i].x as f64 + t * (pts[j].x - pts[i].x) as f64).round() as c_int;
                    intersections.push(x_intersect);
                }
            }

            intersections.sort();

            // Fill between pairs
            for k in (0..intersections.len()).step_by(2) {
                if k + 1 < intersections.len() {
                    let x1 = intersections[k].max(self.clip_x);
                    let x2 = intersections[k + 1].min(self.clip_x + self.clip_w - 1);
                    for x in x1..=x2 {
                        if x >= 0 && x < self.width {
                            self.pixels[(y as usize) * (self.width as usize) + (x as usize)] = p;
                        }
                    }
                }
            }
        }
    }

    /// Draw a polygon outline.
    pub fn draw_polygon(&mut self, pts: &[point], c: rgb) {
        if pts.len() < 2 {
            return;
        }
        let n = pts.len();
        for i in 0..n {
            let j = (i + 1) % n;
            self.draw_line(pts[i].x, pts[i].y, pts[j].x, pts[j].y, c);
        }
    }

    /// Draw a rounded rectangle outline.
    pub fn draw_round_rect(&mut self, x: c_int, y: c_int, w: c_int, h: c_int, r: c_int, c: rgb) {
        let radius = r.min(w / 2).min(h / 2);
        if radius <= 0 {
            self.draw_rect(x, y, w, h, c);
            return;
        }
        // Four corners
        self.draw_arc(x + radius, y + radius, radius, radius, 90, 180, c);
        self.draw_arc(x + w - 1 - radius, y + radius, radius, radius, 0, 90, c);
        self.draw_arc(
            x + w - 1 - radius,
            y + h - 1 - radius,
            radius,
            radius,
            -90,
            0,
            c,
        );
        self.draw_arc(x + radius, y + h - 1 - radius, radius, radius, 180, 270, c);
        // Four sides
        self.draw_line(x + radius, y, x + w - 1 - radius, y, c);
        self.draw_line(x + w - 1, y + radius, x + w - 1, y + h - 1 - radius, c);
        self.draw_line(x + radius, y + h - 1, x + w - 1 - radius, y + h - 1, c);
        self.draw_line(x, y + radius, x, y + h - 1 - radius, c);
    }

    /// Fill a rounded rectangle.
    pub fn fill_round_rect(&mut self, x: c_int, y: c_int, w: c_int, h: c_int, r: c_int, c: rgb) {
        let radius = r.min(w / 2).min(h / 2);
        // Fill center rectangle
        self.fill_rect(x + radius, y, w - 2 * radius, h, c);
        self.fill_rect(x, y + radius, w, h - 2 * radius, c);
        // Fill four corner circles
        self.fill_ellipse(x + radius, y + radius, radius, radius, c);
        self.fill_ellipse(x + w - 1 - radius, y + radius, radius, radius, c);
        self.fill_ellipse(x + w - 1 - radius, y + h - 1 - radius, radius, radius, c);
        self.fill_ellipse(x + radius, y + h - 1 - radius, radius, radius, c);
    }

    /// Invert a rectangle (XOR each pixel with 0xFFFFFF).
    pub fn invert_rect(&mut self, x: c_int, y: c_int, w: c_int, h: c_int) {
        let x1 = x.max(0);
        let y1 = y.max(0);
        let x2 = (x + w).min(self.width);
        let y2 = (y + h).min(self.height);
        for py in y1..y2 {
            for px in x1..x2 {
                let idx = (py as usize) * (self.width as usize) + (px as usize);
                let p = self.pixels[idx];
                self.pixels[idx] = Pixel {
                    r: 255 - p.r,
                    g: 255 - p.g,
                    b: 255 - p.b,
                    a: p.a,
                };
            }
        }
    }

    /// Copy rectangle from this framebuffer to another.
    pub fn copy_to(&self, dest: &mut Framebuffer, src_rect: rect, dest_pt: point) {
        for y in 0..src_rect.height {
            for x in 0..src_rect.width {
                let sx = src_rect.x + x;
                let sy = src_rect.y + y;
                let dx = dest_pt.x + x;
                let dy = dest_pt.y + y;
                if sx >= 0
                    && sx < self.width
                    && sy >= 0
                    && sy < self.height
                    && dx >= 0
                    && dx < dest.width
                    && dy >= 0
                    && dy < dest.height
                {
                    dest.pixels[(dy as usize) * (dest.width as usize) + (dx as usize)] =
                        self.pixels[(sy as usize) * (self.width as usize) + (sx as usize)];
                }
            }
        }
    }

    /// Scroll a rectangle within the framebuffer.
    pub fn scroll_rect(&mut self, dx: c_int, dy: c_int, r: rect) {
        // Copy the region to a temp buffer then back at offset
        let x1 = r.x.max(0);
        let y1 = r.y.max(0);
        let x2 = (r.x + r.width).min(self.width);
        let y2 = (r.y + r.height).min(self.height);
        let w = x2 - x1;
        let h = y2 - y1;
        if w <= 0 || h <= 0 {
            return;
        }

        let mut tmp: Vec<Pixel> = Vec::with_capacity((w * h) as usize);
        for y in y1..y2 {
            for x in x1..x2 {
                tmp.push(self.pixels[(y as usize) * (self.width as usize) + (x as usize)]);
            }
        }

        for y in 0..h {
            for x in 0..w {
                let src_x = x1 + x;
                let src_y = y1 + y;
                let dst_x = src_x + dx;
                let dst_y = src_y + dy;
                if dst_x >= 0 && dst_x < self.width && dst_y >= 0 && dst_y < self.height {
                    self.pixels[(dst_y as usize) * (self.width as usize) + (dst_x as usize)] =
                        tmp[(y as usize) * (w as usize) + (x as usize)];
                }
            }
        }
    }

    /// Get raw pixel data as bytes (RGBA format).
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self.pixels.as_ptr() as *const u8,
                self.pixels.len() * std::mem::size_of::<Pixel>(),
            )
        }
    }
}

/// Extract a Framebuffer pointer from an ObjInfo's handle field.
#[inline]
pub unsafe fn get_framebuffer(obj: objptr) -> Option<&'static mut Framebuffer> {
    unsafe {
        if obj.is_null() {
            return None;
        }
        let handle = (*obj).handle;
        if handle.is_null() {
            return None;
        }
        (handle as *mut Framebuffer).as_mut()
    }
}

pub unsafe fn attach_framebuffer(
    obj: objptr,
    width: c_int,
    height: c_int,
) -> Option<*mut Framebuffer> {
    unsafe {
        if obj.is_null() {
            return None;
        }
        let fb = Box::into_raw(Box::new(Framebuffer::new(width, height)));
        (*obj).handle = fb as *mut c_void;
        (*obj).depth = 32;
        (*obj).rect.width = width;
        (*obj).rect.height = height;
        Some(fb)
    }
}

pub unsafe fn detach_framebuffer(obj: objptr) {
    unsafe {
        if obj.is_null() {
            return;
        }
        let handle = (*obj).handle;
        if !handle.is_null() {
            let _ = Box::from_raw(handle as *mut Framebuffer);
            (*obj).handle = ptr::null_mut();
        }
    }
}

pub unsafe fn framebuffer_from_drawstate() -> Option<&'static mut Framebuffer> {
    unsafe {
        let ds = super::drawing::get_current_drawstate();
        if ds.dest.is_null() {
            return None;
        }
        let handle = (*ds.dest).handle;
        if handle.is_null() {
            return None;
        }
        (handle as *mut Framebuffer).as_mut()
    }
}

#[inline]
pub unsafe fn framebuffer_from_drawing(d: drawing) -> Option<&'static mut Framebuffer> {
    unsafe {
        if d.is_null() {
            return None;
        }
        let handle = (*d).handle;
        if handle.is_null() {
            return None;
        }
        (handle as *mut Framebuffer).as_mut()
    }
}
