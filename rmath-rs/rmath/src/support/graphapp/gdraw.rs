#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Extended drawing functions for GraphApp.
//!
//! Ported from gdraw.c - thread-safe and extended drawing functions.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::ptr;

use super::objects;
use super::strings;
use super::types::*;

const DEFAULT_FONT_HEIGHT: c_int = 14;
const MAX_RASTER_PIXELS: usize = 262_144;

#[derive(Clone, Copy, Default)]
struct FontInfo {
    height: c_int,
    style: c_int,
    quality: c_int,
    use_points: c_int,
}

#[derive(Default)]
struct DrawingState {
    clip: Option<rect>,
    pixels: BTreeMap<(c_int, c_int), rgb>,
    odd_even_fill: bool,
}

thread_local! {
    static DRAWING_STATE: RefCell<HashMap<usize, DrawingState>> = RefCell::new(HashMap::new());
    static FONT_STATE: RefCell<HashMap<usize, FontInfo>> = RefCell::new(HashMap::new());
}

fn drawing_key(d: drawing) -> usize {
    d as usize
}

fn font_key(f: font) -> usize {
    f as usize
}

fn normalized_rect(mut r: rect) -> rect {
    if r.width < 0 {
        r.x += r.width;
        r.width = -r.width;
    }
    if r.height < 0 {
        r.y += r.height;
        r.height = -r.height;
    }
    r
}

fn rect_bounds(r: rect) -> Option<(c_int, c_int, c_int, c_int)> {
    let r = normalized_rect(r);
    if r.width <= 0 || r.height <= 0 {
        None
    } else {
        Some((r.x, r.y, r.x + r.width, r.y + r.height))
    }
}

fn rect_area(r: rect) -> Option<usize> {
    rect_bounds(r).and_then(|(_, _, x1, y1)| {
        let w = usize::try_from(x1 - normalized_rect(r).x).ok()?;
        let h = usize::try_from(y1 - normalized_rect(r).y).ok()?;
        w.checked_mul(h)
    })
}

fn point_in_rect(p: point, r: rect) -> bool {
    rect_bounds(r)
        .map(|(x0, y0, x1, y1)| p.x >= x0 && p.x < x1 && p.y >= y0 && p.y < y1)
        .unwrap_or(false)
}

fn object_rect(d: drawing) -> Option<rect> {
    let addr = d as usize;
    if d.is_null() || addr < 4096 || addr % std::mem::align_of::<ObjInfo>() != 0 {
        None
    } else {
        let r = unsafe { (*d).rect };
        rect_bounds(r).map(|_| r)
    }
}

fn get_clip(d: drawing, state: &DrawingState) -> Option<rect> {
    state.clip.or_else(|| object_rect(d))
}

fn with_state<R>(d: drawing, f: impl FnOnce(&DrawingState) -> R) -> R {
    DRAWING_STATE.with(|states| {
        let states = states.borrow();
        match states.get(&drawing_key(d)) {
            Some(state) => f(state),
            None => f(&DrawingState::default()),
        }
    })
}

fn with_state_mut<R>(d: drawing, f: impl FnOnce(&mut DrawingState) -> R) -> R {
    DRAWING_STATE.with(|states| {
        let mut states = states.borrow_mut();
        let state = states.entry(drawing_key(d)).or_default();
        f(state)
    })
}

fn rgb_invert(c: rgb) -> rgb {
    (!c) & 0x00FF_FFFF
}

fn blend_channel(dst: c_int, src: c_int, alpha: c_int) -> c_int {
    (dst * (255 - alpha) + src * alpha + 127) / 255
}

fn blend_rgb(dst: rgb, src: rgb, alpha: c_int) -> rgb {
    let alpha = alpha.clamp(0, 255);
    rgb_make(
        blend_channel(getred(dst) as c_int, getred(src) as c_int, alpha) as u64,
        blend_channel(getgreen(dst) as c_int, getgreen(src) as c_int, alpha) as u64,
        blend_channel(getblue(dst) as c_int, getblue(src) as c_int, alpha) as u64,
    )
}

fn set_pixel_if_visible(d: drawing, x: c_int, y: c_int, color: rgb) {
    if d.is_null() {
        return;
    }
    with_state_mut(d, |state| {
        let p = point { x, y };
        if get_clip(d, state).map(|clip| point_in_rect(p, clip)).unwrap_or(true) {
            state.pixels.insert((x, y), color);
        }
    });
}

fn get_pixel_state(d: drawing, x: c_int, y: c_int) -> rgb {
    with_state(d, |state| state.pixels.get(&(x, y)).copied().unwrap_or(Black))
}

fn stamp_square(d: drawing, center: point, width: c_int, color: rgb) {
    let radius = (width.max(1) - 1) / 2;
    for y in center.y - radius..=center.y + radius {
        for x in center.x - radius..=center.x + radius {
            set_pixel_if_visible(d, x, y, color);
        }
    }
}

fn draw_line_segment(d: drawing, width: c_int, color: rgb, p1: point, p2: point) {
    let mut x0 = p1.x;
    let mut y0 = p1.y;
    let x1 = p2.x;
    let y1 = p2.y;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        stamp_square(d, point { x: x0, y: y0 }, width, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = err * 2;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn fill_rect_pixels(d: drawing, r: rect, color: rgb) {
    if rect_area(r).unwrap_or(usize::MAX) > MAX_RASTER_PIXELS {
        let r = normalized_rect(r);
        set_pixel_if_visible(d, r.x, r.y, color);
        set_pixel_if_visible(d, r.x + r.width.saturating_sub(1), r.y, color);
        set_pixel_if_visible(d, r.x, r.y + r.height.saturating_sub(1), color);
        set_pixel_if_visible(
            d,
            r.x + r.width.saturating_sub(1),
            r.y + r.height.saturating_sub(1),
            color,
        );
        return;
    }

    if let Some((x0, y0, x1, y1)) = rect_bounds(r) {
        for y in y0..y1 {
            for x in x0..x1 {
                set_pixel_if_visible(d, x, y, color);
            }
        }
    }
}

fn draw_rect_outline(d: drawing, width: c_int, color: rgb, r: rect) {
    let r = normalized_rect(r);
    if r.width <= 0 || r.height <= 0 {
        return;
    }

    let tl = point { x: r.x, y: r.y };
    let tr = point {
        x: r.x + r.width.saturating_sub(1),
        y: r.y,
    };
    let bl = point {
        x: r.x,
        y: r.y + r.height.saturating_sub(1),
    };
    let br = point {
        x: r.x + r.width.saturating_sub(1),
        y: r.y + r.height.saturating_sub(1),
    };
    draw_line_segment(d, width, color, tl, tr);
    draw_line_segment(d, width, color, tl, bl);
    draw_line_segment(d, width, color, tr, br);
    draw_line_segment(d, width, color, bl, br);
}

fn ellipse_contains(center_x: f64, center_y: f64, rx: f64, ry: f64, x: c_int, y: c_int) -> bool {
    if rx <= 0.0 || ry <= 0.0 {
        return false;
    }
    let dx = (x as f64 - center_x) / rx;
    let dy = (y as f64 - center_y) / ry;
    (dx * dx) + (dy * dy) <= 1.0
}

fn draw_ellipse_impl(d: drawing, width: c_int, color: rgb, r: rect, fill: bool) {
    let r = normalized_rect(r);
    if r.width <= 0 || r.height <= 0 {
        return;
    }
    if rect_area(r).unwrap_or(usize::MAX) > MAX_RASTER_PIXELS {
        draw_rect_outline(d, width.max(1), color, r);
        return;
    }

    let cx = r.x as f64 + (r.width - 1) as f64 / 2.0;
    let cy = r.y as f64 + (r.height - 1) as f64 / 2.0;
    let rx = (r.width.max(1) - 1) as f64 / 2.0;
    let ry = (r.height.max(1) - 1) as f64 / 2.0;
    let inner_rx = (rx - width.max(1) as f64).max(0.0);
    let inner_ry = (ry - width.max(1) as f64).max(0.0);

    if let Some((x0, y0, x1, y1)) = rect_bounds(r) {
        for y in y0..y1 {
            for x in x0..x1 {
                let in_outer = ellipse_contains(cx, cy, rx.max(0.5), ry.max(0.5), x, y);
                if !in_outer {
                    continue;
                }
                let in_inner = ellipse_contains(cx, cy, inner_rx, inner_ry, x, y);
                if fill || !in_inner {
                    set_pixel_if_visible(d, x, y, color);
                }
            }
        }
    }
}

fn points_from_raw(p: *mut point, n: c_int) -> Vec<point> {
    if p.is_null() || n <= 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(p, n as usize).to_vec() }
    }
}

fn polygon_bounds(points: &[point]) -> Option<rect> {
    let mut iter = points.iter();
    let first = *iter.next()?;
    let mut min_x = first.x;
    let mut max_x = first.x;
    let mut min_y = first.y;
    let mut max_y = first.y;
    for p in iter {
        min_x = min_x.min(p.x);
        max_x = max_x.max(p.x);
        min_y = min_y.min(p.y);
        max_y = max_y.max(p.y);
    }
    Some(rect {
        x: min_x,
        y: min_y,
        width: max_x - min_x + 1,
        height: max_y - min_y + 1,
    })
}

fn point_in_polygon_even_odd(points: &[point], x: f64, y: f64) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let xi = points[i].x as f64;
        let yi = points[i].y as f64;
        let xj = points[j].x as f64;
        let yj = points[j].y as f64;
        let crosses = ((yi > y) != (yj > y))
            && (x < (xj - xi) * (y - yi) / (yj - yi + f64::EPSILON) + xi);
        if crosses {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn point_in_polygon_winding(points: &[point], x: f64, y: f64) -> bool {
    let mut winding = 0;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let p1 = points[j];
        let p2 = points[i];
        if p1.y <= y as c_int {
            if p2.y > y as c_int
                && ((p2.x - p1.x) as f64 * (y - p1.y as f64))
                    - ((x - p1.x as f64) * (p2.y - p1.y) as f64)
                    > 0.0
            {
                winding += 1;
            }
        } else if p2.y <= y as c_int
            && ((p2.x - p1.x) as f64 * (y - p1.y as f64))
                - ((x - p1.x as f64) * (p2.y - p1.y) as f64)
                < 0.0
        {
            winding -= 1;
        }
        j = i;
    }
    winding != 0
}

fn fill_polygon_impl(d: drawing, points: &[point], color: rgb, odd_even: bool) {
    if points.len() < 3 {
        return;
    }
    let Some(bounds) = polygon_bounds(points) else {
        return;
    };
    if rect_area(bounds).unwrap_or(usize::MAX) > MAX_RASTER_PIXELS {
        unsafe {
            gdrawpolygon(
                d,
                1,
                lSolid,
                color,
                points.as_ptr() as *mut point,
                points.len() as c_int,
                0,
                0,
                0,
                0.0,
            );
        }
        return;
    }
    let Some((x0, y0, x1, y1)) = rect_bounds(bounds) else {
        return;
    };
    for y in y0..y1 {
        for x in x0..x1 {
            let inside = if odd_even {
                point_in_polygon_even_odd(points, x as f64 + 0.5, y as f64 + 0.5)
            } else {
                point_in_polygon_winding(points, x as f64 + 0.5, y as f64 + 0.5)
            };
            if inside {
                set_pixel_if_visible(d, x, y, color);
            }
        }
    }
}

unsafe fn c_string_len(s: *const c_char) -> usize {
    if s.is_null() {
        0
    } else {
        unsafe { CStr::from_ptr(s).to_bytes().len() }
    }
}

unsafe fn wide_string(codepoints: *const c_int, count: Option<c_int>) -> Vec<char> {
    if codepoints.is_null() {
        return Vec::new();
    }
    let mut chars = Vec::new();
    let mut idx = 0usize;
    loop {
        if let Some(max) = count
            && idx >= max.max(0) as usize
        {
            break;
        }
        let value = unsafe { *codepoints.add(idx) };
        if count.is_none() && value == 0 {
            break;
        }
        chars.push(char::from_u32(value.max(0) as u32).unwrap_or('\u{FFFD}'));
        idx += 1;
    }
    chars
}

fn font_info(f: font) -> FontInfo {
    if f.is_null() {
        return FontInfo {
            height: DEFAULT_FONT_HEIGHT,
            style: 0,
            quality: 0,
            use_points: 1,
        };
    }

    FONT_STATE.with(|fonts| {
        fonts.borrow().get(&font_key(f)).copied().unwrap_or(FontInfo {
            height: unsafe { (*f).value.max(1) },
            style: unsafe { (*f).flags as c_int },
            quality: unsafe { (*f).max },
            use_points: unsafe { (*f).size },
        })
    })
}

fn font_char_width(f: font) -> c_int {
    let info = font_info(f);
    let base = (info.height.max(1) + 1) / 2;
    if (info.style & FixedWidth) != 0 {
        base
    } else {
        (base - 1).max(1)
    }
}

fn text_width(f: font, len: usize) -> c_int {
    font_char_width(f).saturating_mul(len as c_int)
}

fn text_height(f: font) -> c_int {
    font_info(f).height.max(1)
}

fn write_metric(out: *mut c_int, value: c_int) {
    if !out.is_null() {
        unsafe {
            *out = value;
        }
    }
}

fn read_image_pixel(img: image, x: c_int, y: c_int) -> Option<rgb> {
    if img.is_null() || unsafe { (*img).pixels.is_null() } {
        return None;
    }
    let width = unsafe { (*img).width };
    let height = unsafe { (*img).height };
    if x < 0 || y < 0 || x >= width || y >= height {
        return None;
    }
    let index = usize::try_from(y.checked_mul(width)? + x).ok()?;
    let depth = unsafe { (*img).depth };
    unsafe {
        if depth >= 32 {
            let offset = index.checked_mul(4)?;
            let pixels = (*img).pixels.add(offset);
            Some(rgb_make(
                *pixels as u64,
                *pixels.add(1) as u64,
                *pixels.add(2) as u64,
            ))
        } else if depth >= 8 {
            let value = *(*img).pixels.add(index) as usize;
            if !(*img).cmap.is_null() && value < (*img).cmapsize.max(0) as usize {
                Some(*(*img).cmap.add(value))
            } else {
                Some(rgb_make(value as u64, value as u64, value as u64))
            }
        } else {
            None
        }
    }
}

fn draw_image_into_rect(d: drawing, img: image, dr: rect, sr: rect, mask: Option<image>) {
    let Some((dx0, dy0, dx1, dy1)) = rect_bounds(dr) else {
        return;
    };
    let Some((sx0, sy0, sx1, sy1)) = rect_bounds(sr) else {
        return;
    };
    let src_w = (sx1 - sx0).max(1);
    let src_h = (sy1 - sy0).max(1);
    let dst_w = (dx1 - dx0).max(1);
    let dst_h = (dy1 - dy0).max(1);

    if usize::try_from(dst_w)
        .ok()
        .and_then(|w| usize::try_from(dst_h).ok().and_then(|h| w.checked_mul(h)))
        .unwrap_or(usize::MAX)
        > MAX_RASTER_PIXELS
    {
        return;
    }

    for dy in 0..dst_h {
        for dx in 0..dst_w {
            let sx = sx0 + dx * src_w / dst_w;
            let sy = sy0 + dy * src_h / dst_h;
            if let Some(mask_img) = mask {
                let mask_pixel = read_image_pixel(mask_img, sx, sy).unwrap_or(Black);
                if mask_pixel == Black {
                    continue;
                }
            }
            if let Some(pixel) = read_image_pixel(img, sx, sy) {
                set_pixel_if_visible(d, dx0 + dx, dy0 + dy, pixel);
            }
        }
    }
}

pub unsafe fn ggetcliprect(d: drawing) -> rect {
    with_state(d, |state| get_clip(d, state).unwrap_or_default())
}

pub unsafe fn gsetcliprect(d: drawing, r: rect) {
    if d.is_null() {
        return;
    }
    with_state_mut(d, |state| state.clip = rect_bounds(r).map(|_| normalized_rect(r)));
}

pub unsafe fn gbitblt(db: bitmap, sb: bitmap, p: point, r: rect) {
    let Some((x0, y0, x1, y1)) = rect_bounds(r) else {
        return;
    };
    let mut copied = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            copied.push((p.x + (x - x0), p.y + (y - y0), ggetpixel(sb, point { x, y })));
        }
    }
    for (x, y, color) in copied {
        set_pixel_if_visible(db, x, y, color);
    }
}

pub unsafe fn gscroll(d: drawing, dp: point, r: rect) {
    if d.is_null() {
        return;
    }
    with_state_mut(d, |state| {
        let mut moved = Vec::new();
        state.pixels.retain(|&(x, y), color| {
            if point_in_rect(point { x, y }, r) {
                moved.push(((x + dp.x, y + dp.y), *color));
                false
            } else {
                true
            }
        });
        for ((x, y), color) in moved {
            state.pixels.insert((x, y), color);
        }
    });
}

pub unsafe fn ginvert(d: drawing, r: rect) {
    let Some((x0, y0, x1, y1)) = rect_bounds(r) else {
        return;
    };
    for y in y0..y1 {
        for x in x0..x1 {
            let color = rgb_invert(ggetpixel(d, point { x, y }));
            set_pixel_if_visible(d, x, y, color);
        }
    }
}

pub unsafe fn ggetpixel(d: drawing, p: point) -> rgb {
    get_pixel_state(d, p.x, p.y)
}

pub unsafe fn gsetpixel(d: drawing, p: point, c: rgb) {
    set_pixel_if_visible(d, p.x, p.y, c);
}

pub unsafe fn gdrawline(
    d: drawing,
    width: c_int,
    _style: c_int,
    c: rgb,
    p1: point,
    p2: point,
    _fast: c_int,
    _lend: c_int,
    _ljoin: c_int,
    _lmitre: f32,
) {
    draw_line_segment(d, width.max(1), c, p1, p2);
}

pub unsafe fn gdrawrect(
    d: drawing,
    width: c_int,
    _style: c_int,
    c: rgb,
    r: rect,
    _fast: c_int,
    _lend: c_int,
    _ljoin: c_int,
    _lmitre: f32,
) {
    draw_rect_outline(d, width.max(1), c, r);
}

pub unsafe fn gfillrect(d: drawing, fill: rgb, r: rect) {
    fill_rect_pixels(d, r, fill);
}

pub unsafe fn gcopy(d: drawing, d2: drawing, r: rect) {
    gbitblt(d, d2, point { x: r.x, y: r.y }, r);
}

pub unsafe fn gcopyalpha(d: drawing, d2: drawing, r: rect, alpha: c_int) {
    let Some((x0, y0, x1, y1)) = rect_bounds(r) else {
        return;
    };
    for y in y0..y1 {
        for x in x0..x1 {
            let src = ggetpixel(d2, point { x, y });
            let dst = ggetpixel(d, point { x, y });
            set_pixel_if_visible(d, x, y, blend_rgb(dst, src, alpha));
        }
    }
}

pub unsafe fn gcopyalpha2(d: drawing, src: image, r: rect) {
    gdrawimage(
        d,
        src,
        r,
        rect {
            x: 0,
            y: 0,
            width: r.width,
            height: r.height,
        },
    );
}

pub unsafe fn gdrawellipse(
    d: drawing,
    width: c_int,
    border: rgb,
    r: rect,
    _fast: c_int,
    _lend: c_int,
    _ljoin: c_int,
    _lmitre: f32,
) {
    draw_ellipse_impl(d, width.max(1), border, r, false);
}

pub unsafe fn gfillellipse(d: drawing, fill: rgb, r: rect) {
    draw_ellipse_impl(d, 1, fill, r, true);
}

pub unsafe fn gdrawpolyline(
    d: drawing,
    width: c_int,
    _style: c_int,
    c: rgb,
    p: *mut point,
    n: c_int,
    closepath: c_int,
    _fast: c_int,
    _lend: c_int,
    _ljoin: c_int,
    _lmitre: f32,
) {
    let points = points_from_raw(p, n);
    if points.len() < 2 {
        return;
    }
    for segment in points.windows(2) {
        draw_line_segment(d, width.max(1), c, segment[0], segment[1]);
    }
    if closepath != 0 {
        draw_line_segment(d, width.max(1), c, *points.last().unwrap_or(&points[0]), points[0]);
    }
}

pub unsafe fn gdrawpolygon(
    d: drawing,
    width: c_int,
    style: c_int,
    c: rgb,
    p: *mut point,
    n: c_int,
    fast: c_int,
    lend: c_int,
    ljoin: c_int,
    lmitre: f32,
) {
    gdrawpolyline(d, width, style, c, p, n, 1, fast, lend, ljoin, lmitre);
}

pub unsafe fn gsetpolyfillmode(d: drawing, oddeven: c_int) {
    if d.is_null() {
        return;
    }
    with_state_mut(d, |state| state.odd_even_fill = oddeven != 0);
}

pub unsafe fn gfillpolygon(d: drawing, fill: rgb, p: *mut point, n: c_int) {
    let points = points_from_raw(p, n);
    let odd_even = with_state(d, |state| state.odd_even_fill);
    fill_polygon_impl(d, &points, fill, odd_even);
}

pub unsafe fn gfillpolypolygon(
    d: drawing,
    fill: rgb,
    p: *mut point,
    npoly: c_int,
    nper: *mut c_int,
) {
    if p.is_null() || nper.is_null() || npoly <= 0 {
        return;
    }
    let counts = unsafe { std::slice::from_raw_parts(nper, npoly as usize) };
    let mut offset = 0usize;
    let odd_even = with_state(d, |state| state.odd_even_fill);
    for count in counts {
        let count = (*count).max(0) as usize;
        if count == 0 {
            continue;
        }
        let slice = unsafe { std::slice::from_raw_parts(p.add(offset), count) };
        fill_polygon_impl(d, slice, fill, odd_even);
        offset += count;
    }
}

pub unsafe fn gdrawimage(d: drawing, img: image, dr: rect, sr: rect) {
    draw_image_into_rect(d, img, dr, sr, None);
}

pub unsafe fn gmaskimage(d: drawing, img: image, dr: rect, sr: rect, mask: image) {
    draw_image_into_rect(d, img, dr, sr, Some(mask));
}

pub unsafe fn gdrawstr(
    d: drawing,
    f: font,
    c: rgb,
    p: point,
    s: *const c_char,
) -> c_int {
    let width = gstrwidth(d, f, s);
    let char_w = font_char_width(f).max(1);
    let len = unsafe { c_string_len(s) } as c_int;
    for idx in 0..len {
        set_pixel_if_visible(d, p.x + idx * char_w, p.y, c);
    }
    width
}

pub unsafe fn gdrawstr1(
    d: drawing,
    f: font,
    c: rgb,
    p: point,
    s: *const c_char,
    hadj: f64,
) {
    let width = gstrwidth(d, f, s);
    let start = point {
        x: p.x - (hadj * width as f64).round() as c_int,
        y: p.y,
    };
    gdrawstr(d, f, c, start, s);
}

pub unsafe fn gstrrect(_d: drawing, f: font, s: *const c_char) -> rect {
    rect {
        x: 0,
        y: 0,
        width: gstrwidth(ptr::null_mut(), f, s),
        height: text_height(f),
    }
}

pub unsafe fn gstrsize(_d: drawing, f: font, s: *const c_char) -> point {
    point {
        x: gstrwidth(ptr::null_mut(), f, s),
        y: text_height(f),
    }
}

pub unsafe fn gstrwidth(_d: drawing, f: font, s: *const c_char) -> c_int {
    text_width(f, unsafe { c_string_len(s) })
}

pub unsafe fn gcharmetric(
    _d: drawing,
    f: font,
    _c: c_int,
    ascent: *mut c_int,
    descent: *mut c_int,
    width: *mut c_int,
) {
    let height = text_height(f);
    let asc = (height * 3) / 4;
    write_metric(ascent, asc);
    write_metric(descent, height - asc);
    write_metric(width, font_char_width(f));
}

pub unsafe fn gnewfont(
    d: drawing,
    face: *const c_char,
    style: c_int,
    size: c_int,
    rot: f64,
    usePoints: c_int,
) -> font {
    gnewfont2(d, face, style, size, rot, usePoints, 0)
}

pub unsafe fn gnewfont2(
    _d: drawing,
    face: *const c_char,
    style: c_int,
    size: c_int,
    _rot: f64,
    usePoints: c_int,
    quality: c_int,
) -> font {
    objects::init_objects();
    let font = objects::new_object(FontObject, ptr::null_mut(), ptr::null_mut());
    if font.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*font).text = strings::new_string(face);
        (*font).flags = style as _;
        (*font).value = size.max(1);
        (*font).size = usePoints;
        (*font).max = quality;
    }
    FONT_STATE.with(|fonts| {
        fonts.borrow_mut().insert(
            font_key(font),
            FontInfo {
                height: size.max(1),
                style,
                quality,
                use_points: usePoints,
            },
        );
    });
    font
}

pub unsafe fn ghasfixedwidth(f: font) -> c_int {
    if f.is_null() {
        return 0;
    }
    let style = font_info(f).style;
    let face_is_mono = unsafe {
        if (*f).text.is_null() {
            false
        } else {
            CStr::from_ptr((*f).text)
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("mono")
        }
    };
    if (style & FixedWidth) != 0 || face_is_mono {
        1
    } else {
        0
    }
}

pub unsafe fn newfield_no_border(text: *const c_char, r: rect) -> field {
    objects::init_objects();
    let field = objects::new_object(FieldObject, ptr::null_mut(), ptr::null_mut());
    if field.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*field).rect = normalized_rect(r);
        (*field).text = strings::new_string(text);
        (*field).state |= GA_Visible | GA_Enabled;
        (*field).bg = White;
        (*field).fg = Black;
    }
    field
}

pub unsafe fn gdrawwcs(
    d: drawing,
    f: font,
    c: rgb,
    p: point,
    s: *const c_int,
) -> c_int {
    let chars = unsafe { wide_string(s, None) };
    let width = text_width(f, chars.len());
    let char_w = font_char_width(f).max(1);
    for (idx, _) in chars.iter().enumerate() {
        set_pixel_if_visible(d, p.x + idx as c_int * char_w, p.y, c);
    }
    width
}

pub unsafe fn gwcswidth(_d: drawing, f: font, s: *const c_int) -> c_int {
    text_width(f, unsafe { wide_string(s, None) }.len())
}

pub unsafe fn gwcharmetric(
    d: drawing,
    f: font,
    c: c_int,
    ascent: *mut c_int,
    descent: *mut c_int,
    width: *mut c_int,
) {
    gcharmetric(d, f, c, ascent, descent, width);
}

pub unsafe fn gwdrawstr1(
    d: drawing,
    f: font,
    c: rgb,
    p: point,
    s: *const c_int,
    cnt: c_int,
    hadj: f64,
) {
    let chars = unsafe { wide_string(s, Some(cnt)) };
    let width = text_width(f, chars.len());
    let char_w = font_char_width(f).max(1);
    let start_x = p.x - (hadj * width as f64).round() as c_int;
    for (idx, _) in chars.iter().enumerate() {
        set_pixel_if_visible(d, start_x + idx as c_int * char_w, p.y, c);
    }
}

pub unsafe fn gstrwidth1(
    _d: drawing,
    f: font,
    s: *const c_char,
    _enc: c_int,
) -> c_int {
    text_width(f, unsafe { c_string_len(s) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn clip_rect_and_pixels_are_tracked_per_drawing() {
        let drawing = 1usize as drawing;
        unsafe {
            gsetcliprect(
                drawing,
                rect {
                    x: 1,
                    y: 1,
                    width: 2,
                    height: 2,
                },
            );
            gsetpixel(drawing, point { x: 0, y: 0 }, White);
            gsetpixel(drawing, point { x: 1, y: 1 }, White);

            assert_eq!(ggetcliprect(drawing).width, 2);
            assert_eq!(ggetpixel(drawing, point { x: 0, y: 0 }), Black);
            assert_eq!(ggetpixel(drawing, point { x: 1, y: 1 }), White);
        }
    }

    #[test]
    fn bitblt_and_scroll_move_pixels() {
        let src = 2usize as drawing;
        let dst = 3usize as drawing;
        unsafe {
            gfillrect(
                src,
                gaRed,
                rect {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 2,
                },
            );
            gbitblt(
                dst,
                src,
                point { x: 4, y: 5 },
                rect {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 2,
                },
            );
            assert_eq!(ggetpixel(dst, point { x: 4, y: 5 }), gaRed);

            gscroll(
                dst,
                point { x: 1, y: -1 },
                rect {
                    x: 4,
                    y: 5,
                    width: 2,
                    height: 2,
                },
            );
            assert_eq!(ggetpixel(dst, point { x: 5, y: 4 }), gaRed);
        }
    }

    #[test]
    fn font_metrics_and_text_width_are_coherent() {
        let face = CString::new("Mono").unwrap_or_else(|e| panic!("{e}"));
        let text = CString::new("abcd").unwrap_or_else(|e| panic!("{e}"));
        unsafe {
            let font = gnewfont2(ptr::null_mut(), face.as_ptr(), FixedWidth, 12, 0.0, 1, 2);
            assert_eq!(ghasfixedwidth(font), 1);
            assert_eq!(gstrwidth(ptr::null_mut(), font, text.as_ptr()), 24);

            let mut ascent = 0;
            let mut descent = 0;
            let mut width = 0;
            gcharmetric(ptr::null_mut(), font, 'a' as c_int, &mut ascent, &mut descent, &mut width);
            assert_eq!(width, 6);
            assert_eq!(ascent + descent, 12);
        }
    }
}
