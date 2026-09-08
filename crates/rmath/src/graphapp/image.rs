#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Image handling for GraphApp.
//!
//! Ported from image.c - provides cross-platform image type with
//! 8-bit indexed and 32-bit true-colour support.

use std::os::raw::c_int;
use std::ptr;

use super::memory;
use super::types::*;

fn normalized_rgb(pixel: rgb) -> rgb {
    if getalpha(pixel) > 0x7F {
        Transparent
    } else {
        pixel & White
    }
}

fn palette_sort_key(pixel: rgb) -> (u8, u64, u64, u64, u64) {
    if pixel == Transparent {
        return (1, 0, 0, 0, 0);
    }
    let r = getred(pixel);
    let g = getgreen(pixel);
    let b = getblue(pixel);
    let luminance = r * 30 + g * 59 + b * 11;
    (0, luminance, r, g, b)
}

fn nearest_palette_index(palette: &[rgb], color: rgb) -> GAbyte {
    if palette.is_empty() {
        return 0;
    }
    if color == Transparent {
        if let Some((index, _)) = palette
            .iter()
            .enumerate()
            .find(|(_, value)| **value == Transparent)
        {
            return index as GAbyte;
        }
    }

    let target_r = getred(color) as i64;
    let target_g = getgreen(color) as i64;
    let target_b = getblue(color) as i64;

    palette
        .iter()
        .enumerate()
        .min_by_key(|(_, entry)| {
            if **entry == Transparent {
                i64::MAX
            } else {
                let dr = getred(**entry) as i64 - target_r;
                let dg = getgreen(**entry) as i64 - target_g;
                let db = getblue(**entry) as i64 - target_b;
                dr * dr + dg * dg + db * db
            }
        })
        .map(|(index, _)| index as GAbyte)
        .unwrap_or(0)
}

fn sample_coordinate(
    out_coord: c_int,
    out_size: c_int,
    start: c_int,
    span: c_int,
    limit: c_int,
) -> c_int {
    if limit <= 0 {
        return 0;
    }
    let mapped = start + (out_coord * span.max(1)) / out_size.max(1);
    mapped.clamp(0, limit - 1)
}

/// Create a new image.
pub unsafe fn newimage(width: c_int, height: c_int, depth: c_int) -> image {
    unsafe {
        if depth != 8 && depth != 32 {
            return ptr::null_mut();
        }

        let img = memory::memalloc(std::mem::size_of::<imagedata>() as i64) as image;
        if img.is_null() {
            return ptr::null_mut();
        }
        ptr::write_bytes(img as *mut u8, 0, std::mem::size_of::<imagedata>());

        (*img).width = width;
        (*img).height = height;

        if depth == 8 {
            (*img).depth = 8;
            let pixels = memory::memalloc((width * height) as i64);
            (*img).pixels = pixels;
        } else {
            (*img).depth = 32;
            let pixels = memory::memalloc(
                (width as i64 * height as i64 * std::mem::size_of::<rgb>() as i64) as i64,
            );
            (*img).pixels = pixels;
        }

        img
    }
}

/// Copy an image.
pub unsafe fn copyimage(img: image) -> image {
    unsafe {
        if img.is_null() {
            return ptr::null_mut();
        }
        let new_img = newimage((*img).width, (*img).height, (*img).depth);
        if !new_img.is_null() {
            setpixels(new_img, (*img).pixels);
            setpalette(new_img, (*img).cmapsize, (*img).cmap);
        }
        new_img
    }
}

/// Delete an image.
pub unsafe fn delimage(img: image) {
    unsafe {
        if img.is_null() {
            return;
        }
        if !(*img).cmap.is_null() {
            memory::memfree((*img).cmap as *mut u8);
        }
        if !(*img).pixels.is_null() {
            memory::memfree((*img).pixels);
        }
        memory::memfree(img as *mut u8);
    }
}

/// Get the depth of an image.
pub unsafe fn imagedepth(img: image) -> c_int {
    unsafe { if img.is_null() { 0 } else { (*img).depth } }
}

/// Get the width of an image.
pub unsafe fn imagewidth(img: image) -> c_int {
    unsafe { if img.is_null() { 0 } else { (*img).width } }
}

/// Get the height of an image.
pub unsafe fn imageheight(img: image) -> c_int {
    unsafe { if img.is_null() { 0 } else { (*img).height } }
}

/// Set the pixels of an image.
pub unsafe fn setpixels(img: image, pixels: *mut super::types::GAbyte) {
    unsafe {
        if img.is_null() {
            return;
        }
        let length = (*img).width * (*img).height;
        let byte_len = if (*img).depth > 8 {
            length as usize * std::mem::size_of::<rgb>()
        } else {
            length as usize
        };
        if !(*img).pixels.is_null() && !pixels.is_null() {
            ptr::copy_nonoverlapping(pixels, (*img).pixels, byte_len);
        }
    }
}

/// Get the pixel array of an image.
pub unsafe fn getpixels(img: image) -> *mut super::types::GAbyte {
    unsafe {
        if img.is_null() {
            ptr::null_mut()
        } else {
            (*img).pixels
        }
    }
}

/// Set the colour palette of an image.
pub unsafe fn setpalette(img: image, cmapsize: c_int, cmap: *mut rgb) {
    unsafe {
        if img.is_null() {
            return;
        }
        if !(*img).cmap.is_null() {
            memory::memfree((*img).cmap as *mut u8);
        }
        (*img).cmapsize = cmapsize;
        if cmapsize > 0 && !cmap.is_null() {
            let new_cmap = memory::memalloc((cmapsize as usize * std::mem::size_of::<rgb>()) as i64)
                as *mut rgb;
            if !new_cmap.is_null() {
                ptr::copy_nonoverlapping(cmap, new_cmap, cmapsize as usize);
                (*img).cmap = new_cmap;
            }
        } else {
            (*img).cmap = ptr::null_mut();
        }
    }
}

/// Get the colour palette of an image.
pub unsafe fn getpalette(img: image) -> *mut rgb {
    unsafe {
        if img.is_null() {
            ptr::null_mut()
        } else {
            (*img).cmap
        }
    }
}

/// Get the palette size.
pub unsafe fn getpalettesize(img: image) -> c_int {
    unsafe { if img.is_null() { 0 } else { (*img).cmapsize } }
}

/// Convert a 32-bit image to 8-bit.
pub unsafe fn convert32to8(img: image) -> image {
    unsafe {
        if img.is_null() {
            return ptr::null_mut();
        }
        if (*img).depth <= 8 {
            return copyimage(img);
        }

        let dest = newimage((*img).width, (*img).height, 8);
        if dest.is_null() {
            return ptr::null_mut();
        }

        let len = ((*img).width * (*img).height).max(0) as usize;
        let src_pixels = (*img).pixels as *const rgb;
        let mut palette: Vec<rgb> = Vec::with_capacity(256);

        for i in 0..len {
            let color = normalized_rgb(*src_pixels.add(i));
            if !palette.contains(&color) && palette.len() < 256 {
                palette.push(color);
            }
        }
        if palette.is_empty() {
            palette.push(Black);
        }

        setpalette(dest, palette.len() as c_int, palette.as_mut_ptr());

        let dest_pixels = (*dest).pixels;
        for i in 0..len {
            let color = normalized_rgb(*src_pixels.add(i));
            let index = palette
                .iter()
                .position(|entry| *entry == color)
                .map(|idx| idx as GAbyte)
                .unwrap_or_else(|| nearest_palette_index(&palette, color));
            *dest_pixels.add(i) = index;
        }

        dest
    }
}

/// Convert an 8-bit image to 32-bit.
pub unsafe fn convert8to32(img: image) -> image {
    unsafe {
        if img.is_null() {
            return ptr::null_mut();
        }
        let new_img = newimage((*img).width, (*img).height, 32);
        if new_img.is_null() {
            return ptr::null_mut();
        }
        let length = (*img).width * (*img).height;
        let pixel8 = (*img).pixels;
        let pixel32 = (*new_img).pixels as *mut rgb;

        for i in 0..length as usize {
            let value = *pixel8.add(i);
            let col = if !(*img).cmap.is_null() && (*img).cmapsize > 0 {
                let idx = (value as c_int).min((*img).cmapsize - 1) as usize;
                *(*img).cmap.add(idx)
            } else {
                Black
            };
            *pixel32.add(i) = col;
        }
        new_img
    }
}

/// Sort an image's colour map and remap indexed pixels to keep colours stable.
pub unsafe fn sortpalette(img: image) {
    unsafe {
        if img.is_null() || (*img).depth > 8 || (*img).cmapsize <= 1 || (*img).cmap.is_null() {
            return;
        }

        let cmapsize = (*img).cmapsize as usize;
        let mut entries: Vec<(usize, rgb)> =
            (0..cmapsize).map(|i| (i, *(*img).cmap.add(i))).collect();
        entries.sort_by_key(|(_, color)| palette_sort_key(*color));

        let mut remap = vec![0u8; cmapsize];
        for (new_index, (old_index, color)) in entries.iter().enumerate() {
            remap[*old_index] = new_index as u8;
            *(*img).cmap.add(new_index) = *color;
        }

        let len = ((*img).width * (*img).height).max(0) as usize;
        for i in 0..len {
            let pixel = *(*img).pixels.add(i) as usize;
            let remapped = remap
                .get(pixel)
                .copied()
                .unwrap_or_else(|| remap[cmapsize - 1]);
            *(*img).pixels.add(i) = remapped;
        }
    }
}

/// Scale an image.
pub unsafe fn scaleimage(src: image, dr: rect, sr: rect) -> image {
    unsafe {
        if src.is_null() || dr.width <= 0 || dr.height <= 0 {
            return ptr::null_mut();
        }
        let dest = newimage(dr.width, dr.height, (*src).depth);
        if dest.is_null() {
            return ptr::null_mut();
        }

        if (*src).depth <= 8 {
            setpalette(dest, (*src).cmapsize, (*src).cmap);
        }

        let source_rect = if sr.width > 0 && sr.height > 0 {
            sr
        } else {
            rect {
                x: 0,
                y: 0,
                width: (*src).width,
                height: (*src).height,
            }
        };

        for y in 0..dr.height {
            let sy = sample_coordinate(
                y,
                dr.height,
                source_rect.y,
                source_rect.height,
                (*src).height,
            );
            for x in 0..dr.width {
                let sx =
                    sample_coordinate(x, dr.width, source_rect.x, source_rect.width, (*src).width);
                let src_index = (sy * (*src).width + sx) as usize;
                let dest_index = (y * dr.width + x) as usize;
                if (*src).depth <= 8 {
                    *(*dest).pixels.add(dest_index) = *(*src).pixels.add(src_index);
                } else {
                    *((*dest).pixels as *mut rgb).add(dest_index) =
                        *((*src).pixels as *const rgb).add(src_index);
                }
            }
        }

        dest
    }
}

/// Get pixel value from an image.
pub unsafe fn get_image_pixel(img: image, x: c_int, y: c_int) -> rgb {
    unsafe {
        if img.is_null() {
            return Transparent;
        }
        if x < 0 || x >= (*img).width || y < 0 || y >= (*img).height {
            return Transparent;
        }
        if (*img).depth <= 8 {
            let value = *(*img).pixels.add((y * (*img).width + x) as usize);
            if !(*img).cmap.is_null() && (value as c_int) < (*img).cmapsize {
                *(*img).cmap.add(value as usize)
            } else {
                Black
            }
        } else {
            let pixel = *((*img).pixels as *const rgb).add((y * (*img).width + x) as usize);
            if getalpha(pixel) > 0x7F {
                Transparent
            } else {
                pixel & White
            }
        }
    }
}

/// Get monochrome pixel value.
#[allow(clippy::if_same_then_else)]
pub unsafe fn get_monochrome_pixel(img: image, x: c_int, y: c_int) -> rgb {
    unsafe {
        let pixel = get_image_pixel(img, x, y);
        if pixel == Transparent {
            Transparent
        } else {
            // Simple threshold
            let r = getred(pixel);
            let g = getgreen(pixel);
            let b = getblue(pixel);
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            if min > 0xE0 {
                White
            } else if max < 0x60 {
                Black
            } else {
                Black
            }
        }
    }
}

/// Get grey pixel value.
pub unsafe fn get_grey_pixel(img: image, x: c_int, y: c_int) -> rgb {
    unsafe {
        let pixel = get_image_pixel(img, x, y);
        if pixel == Transparent {
            Transparent
        } else {
            let r = getred(pixel);
            let g = getgreen(pixel);
            let b = getblue(pixel);
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            if min > 0xE0 {
                White
            } else if max < 0x10 {
                Black
            } else if max < 0x60 {
                DarkGrey
            } else if max < 0xD0 {
                Grey
            } else {
                LightGrey
            }
        }
    }
}

/// Check if image has transparent pixels.
pub unsafe fn has_transparent_pixels(img: image) -> c_int {
    unsafe {
        if img.is_null() {
            return 0;
        }

        let len = ((*img).width * (*img).height).max(0) as usize;
        if (*img).depth <= 8 {
            if (*img).cmap.is_null() || (*img).cmapsize <= 0 {
                return 0;
            }
            for i in 0..len {
                let index = (*(*img).pixels.add(i) as c_int).min((*img).cmapsize - 1) as usize;
                if normalized_rgb(*(*img).cmap.add(index)) == Transparent {
                    return 1;
                }
            }
            0
        } else {
            let pixels = (*img).pixels as *const rgb;
            for i in 0..len {
                if normalized_rgb(*pixels.add(i)) == Transparent {
                    return 1;
                }
            }
            0
        }
    }
}
