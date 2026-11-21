#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

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

fn palette_sort_key(pixel: rgb) -> (u8, rgb, rgb, rgb, rgb) {
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
pub fn newimage(width: c_int, height: c_int, depth: c_int) -> image {
    if depth != 8 && depth != 32 {
        return ptr::null_mut();
    }

    let Ok(width_size) = usize::try_from(width) else {
        return ptr::null_mut();
    };
    let Ok(height_size) = usize::try_from(height) else {
        return ptr::null_mut();
    };
    let Some(bytes) = width_size.checked_mul(height_size).and_then(|pixels| {
        pixels.checked_mul(if depth == 8 {
            1
        } else {
            std::mem::size_of::<rgb>()
        })
    }) else {
        return ptr::null_mut();
    };
    let pixels = memory::memalloc_bytes(bytes);
    if pixels.is_null() {
        return ptr::null_mut();
    }
    let img = memory::memalloc_bytes(std::mem::size_of::<imagedata>()) as image;
    let Some(data) = ptr_mut(img) else {
        memory::memfree(pixels);
        return ptr::null_mut();
    };
    data.width = width;
    data.height = height;
    data.depth = depth;
    data.pixels = pixels;

    img
}

/// Copy an image.
pub fn copyimage(img: image) -> image {
    let Some(src) = ptr_ref(img) else {
        return ptr::null_mut();
    };
    let new_img = newimage(src.width, src.height, src.depth);
    if !new_img.is_null() {
        setpixels(new_img, src.pixels);
        setpalette(new_img, src.cmapsize, src.cmap);
    }
    new_img
}

/// Delete an image.
pub fn delimage(img: image) {
    let Some(data) = ptr_ref(img) else {
        return;
    };
    if !data.cmap.is_null() {
        memory::memfree(data.cmap as *mut u8);
    }
    if !data.pixels.is_null() {
        memory::memfree(data.pixels);
    }
    memory::memfree(img as *mut u8);
}

/// Get the depth of an image.
pub fn imagedepth(img: image) -> c_int {
    ptr_ref(img).map_or(0, |data| data.depth)
}

/// Get the width of an image.
pub fn imagewidth(img: image) -> c_int {
    ptr_ref(img).map_or(0, |data| data.width)
}

/// Get the height of an image.
pub fn imageheight(img: image) -> c_int {
    ptr_ref(img).map_or(0, |data| data.height)
}

/// Set the pixels of an image.
pub fn setpixels(img: image, pixels: *mut super::types::GAbyte) {
    let Some(data) = ptr_mut(img) else {
        return;
    };
    let length = data.width * data.height;
    let byte_len = if data.depth > 8 {
        length as usize * std::mem::size_of::<rgb>()
    } else {
        length as usize
    };
    if !data.pixels.is_null() && !pixels.is_null() {
        // Copy `byte_len` bytes from the caller's buffer into the owned
        // pixel allocation; both are at least that long by construction.
        if let (Some(dest), Some(src)) = (
            slice_mut(data.pixels, byte_len),
            slice_ref(pixels, byte_len),
        ) {
            dest.copy_from_slice(src);
        }
    }
}

/// Get the pixel array of an image.
pub fn getpixels(img: image) -> *mut super::types::GAbyte {
    ptr_ref(img).map_or(ptr::null_mut(), |data| data.pixels)
}

/// Set the colour palette of an image.
pub fn setpalette(img: image, cmapsize: c_int, cmap: *mut rgb) {
    let Some(data) = ptr_mut(img) else {
        return;
    };
    let new_cmap = if cmapsize > 0 && !cmap.is_null() {
        let Some(bytes) = (cmapsize as usize).checked_mul(std::mem::size_of::<rgb>()) else {
            return;
        };
        let replacement = memory::memalloc_bytes(bytes) as *mut rgb;
        if replacement.is_null() {
            return;
        }
        // Copy `cmapsize` entries from the caller's palette into the new
        // allocation sized exactly for them.
        if let (Some(dest), Some(src)) = (
            slice_mut(replacement, cmapsize as usize),
            slice_ref(cmap, cmapsize as usize),
        ) {
            dest.copy_from_slice(src);
        }
        replacement
    } else {
        ptr::null_mut()
    };
    // Publish only after copying: callers may pass the current palette.
    memory::memfree(data.cmap as *mut u8);
    data.cmap = new_cmap;
    data.cmapsize = if new_cmap.is_null() { 0 } else { cmapsize };
}

/// Get the colour palette of an image.
pub fn getpalette(img: image) -> *mut rgb {
    ptr_ref(img).map_or(ptr::null_mut(), |data| data.cmap)
}

/// Get the palette size.
pub fn getpalettesize(img: image) -> c_int {
    ptr_ref(img).map_or(0, |data| data.cmapsize)
}

/// Convert a 32-bit image to 8-bit.
pub fn convert32to8(img: image) -> image {
    let Some(src) = ptr_ref(img) else {
        return ptr::null_mut();
    };
    if src.depth <= 8 {
        return copyimage(img);
    }

    let dest = newimage(src.width, src.height, 8);
    let Some(dest_data) = ptr_mut(dest) else {
        return ptr::null_mut();
    };

    let len = (src.width * src.height).max(0) as usize;
    // Borrow the owned source rgb buffer of `len` entries.
    let Some(src_pixels) = slice_ref(src.pixels as *const rgb, len) else {
        return dest;
    };
    let mut palette: Vec<rgb> = Vec::with_capacity(256);

    for &c in src_pixels {
        let color = normalized_rgb(c);
        if !palette.contains(&color) && palette.len() < 256 {
            palette.push(color);
        }
    }
    if palette.is_empty() {
        palette.push(Black);
    }

    setpalette(dest, palette.len() as c_int, palette.as_mut_ptr());

    // Write palette indices into the owned destination byte buffer of
    // `len` entries.
    let Some(dest_pixels) = slice_mut(dest_data.pixels, len) else {
        return dest;
    };
    for (i, &c) in src_pixels.iter().enumerate() {
        let color = normalized_rgb(c);
        let index = palette
            .iter()
            .position(|entry| *entry == color)
            .map(|idx| idx as GAbyte)
            .unwrap_or_else(|| nearest_palette_index(&palette, color));
        dest_pixels[i] = index;
    }

    dest
}

/// Convert an 8-bit image to 32-bit.
pub fn convert8to32(img: image) -> image {
    let Some(src) = ptr_ref(img) else {
        return ptr::null_mut();
    };
    let new_img = newimage(src.width, src.height, 32);
    let Some(new_data) = ptr_mut(new_img) else {
        return ptr::null_mut();
    };
    let length = (src.width * src.height).max(0) as usize;
    let cmapsize = src.cmapsize.max(0) as usize;

    // Borrow the owned source index buffer, palette, and destination
    // rgb buffer; each holds `length` or `cmapsize` entries.
    let Some(pixel8) = slice_ref(src.pixels, length) else {
        return new_img;
    };
    let palette = slice_ref(src.cmap, cmapsize).unwrap_or(&[]);
    let Some(pixel32) = slice_mut(new_data.pixels as *mut rgb, length) else {
        return new_img;
    };

    for (i, &value) in pixel8.iter().enumerate() {
        let col = if palette.is_empty() {
            Black
        } else {
            palette[(value as usize).min(palette.len() - 1)]
        };
        pixel32[i] = col;
    }
    new_img
}

/// Sort an image's colour map and remap indexed pixels to keep colours stable.
pub fn sortpalette(img: image) {
    let Some(data) = ptr_mut(img) else {
        return;
    };
    if data.depth > 8 || data.cmapsize <= 1 || data.cmap.is_null() {
        return;
    }

    let cmapsize = data.cmapsize as usize;
    let len = (data.width * data.height).max(0) as usize;

    // Rewrite the owned palette and index buffers in place; the palette
    // has `cmapsize` entries and the index buffer has `len` bytes.
    let Some(cmap) = slice_mut(data.cmap, cmapsize) else {
        return;
    };
    let Some(pixels) = slice_mut(data.pixels, len) else {
        return;
    };

    let mut entries: Vec<(usize, rgb)> = cmap.iter().copied().enumerate().collect();
    entries.sort_by_key(|(_, color)| palette_sort_key(*color));

    let mut remap = vec![0u8; cmapsize];
    for (new_index, (old_index, color)) in entries.iter().enumerate() {
        remap[*old_index] = new_index as u8;
        cmap[new_index] = *color;
    }

    for pixel in pixels.iter_mut() {
        *pixel = remap
            .get(*pixel as usize)
            .copied()
            .unwrap_or_else(|| remap[cmapsize - 1]);
    }
}

/// Scale an image.
pub fn scaleimage(src: image, dr: rect, sr: rect) -> image {
    if dr.width <= 0 || dr.height <= 0 {
        return ptr::null_mut();
    }
    let Some(src_data) = ptr_ref(src) else {
        return ptr::null_mut();
    };
    let dest = newimage(dr.width, dr.height, src_data.depth);
    if dest.is_null() {
        return ptr::null_mut();
    }

    if src_data.depth <= 8 {
        setpalette(dest, src_data.cmapsize, src_data.cmap);
    }

    let source_rect = if sr.width > 0 && sr.height > 0 {
        sr
    } else {
        rect {
            x: 0,
            y: 0,
            width: src_data.width,
            height: src_data.height,
        }
    };

    let src_width = src_data.width;
    let src_height = src_data.height;
    let src_depth = src_data.depth;
    let src_len = (src_width * src_height).max(0) as usize;
    let dest_len = (dr.width * dr.height) as usize;
    let src_pixels = src_data.pixels;
    let dest_pixels = ptr_ref(dest).map_or(ptr::null_mut(), |d| d.pixels);

    // Copy sampled entries between the owned source and destination
    // pixel buffers; `sample_coordinate` clamps indices into range.
    if src_depth <= 8 {
        let (Some(src_px), Some(dest_px)) = (
            slice_ref(src_pixels, src_len),
            slice_mut(dest_pixels, dest_len),
        ) else {
            return dest;
        };
        for y in 0..dr.height {
            let sy = sample_coordinate(y, dr.height, source_rect.y, source_rect.height, src_height);
            for x in 0..dr.width {
                let sx =
                    sample_coordinate(x, dr.width, source_rect.x, source_rect.width, src_width);
                dest_px[(y * dr.width + x) as usize] = src_px[(sy * src_width + sx) as usize];
            }
        }
    } else {
        let (Some(src_px), Some(dest_px)) = (
            slice_ref(src_pixels as *const rgb, src_len),
            slice_mut(dest_pixels as *mut rgb, dest_len),
        ) else {
            return dest;
        };
        for y in 0..dr.height {
            let sy = sample_coordinate(y, dr.height, source_rect.y, source_rect.height, src_height);
            for x in 0..dr.width {
                let sx =
                    sample_coordinate(x, dr.width, source_rect.x, source_rect.width, src_width);
                dest_px[(y * dr.width + x) as usize] = src_px[(sy * src_width + sx) as usize];
            }
        }
    }

    dest
}

/// Get pixel value from an image.
pub fn get_image_pixel(img: image, x: c_int, y: c_int) -> rgb {
    let Some(data) = ptr_ref(img) else {
        return Transparent;
    };
    if x < 0 || x >= data.width || y < 0 || y >= data.height {
        return Transparent;
    }
    let index = (y * data.width + x) as usize;
    let len = (data.width * data.height).max(0) as usize;
    if data.depth <= 8 {
        // Read one byte from the owned index buffer.
        let Some(pixels) = slice_ref(data.pixels, len) else {
            return Transparent;
        };
        let value = pixels[index];
        // Read one entry from the owned palette.
        let palette = slice_ref(data.cmap, data.cmapsize.max(0) as usize).unwrap_or(&[]);
        palette.get(value as usize).copied().unwrap_or(Black)
    } else {
        // Read one rgb entry from the owned pixel buffer.
        let Some(pixels) = slice_ref(data.pixels as *const rgb, len) else {
            return Transparent;
        };
        let pixel = pixels[index];
        if getalpha(pixel) > 0x7F {
            Transparent
        } else {
            pixel & White
        }
    }
}

/// Get monochrome pixel value.
#[allow(clippy::if_same_then_else)]
pub fn get_monochrome_pixel(img: image, x: c_int, y: c_int) -> rgb {
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

/// Get grey pixel value.
pub fn get_grey_pixel(img: image, x: c_int, y: c_int) -> rgb {
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

/// Check if image has transparent pixels.
pub fn has_transparent_pixels(img: image) -> c_int {
    let Some(data) = ptr_ref(img) else {
        return 0;
    };
    let len = (data.width * data.height).max(0) as usize;
    if data.depth <= 8 {
        if data.cmap.is_null() || data.cmapsize <= 0 {
            return 0;
        }
        // Scan the owned index buffer and its `cmapsize`-entry palette.
        let Some(pixels) = slice_ref(data.pixels, len) else {
            return 0;
        };
        let Some(cmap) = slice_ref(data.cmap, data.cmapsize as usize) else {
            return 0;
        };
        for &b in pixels {
            if normalized_rgb(cmap[(b as usize).min(cmap.len() - 1)]) == Transparent {
                return 1;
            }
        }
        0
    } else {
        // Scan the owned 32-bit pixel buffer of `len` entries.
        let Some(pixels) = slice_ref(data.pixels as *const rgb, len) else {
            return 0;
        };
        for &pixel in pixels {
            if normalized_rgb(pixel) == Transparent {
                return 1;
            }
        }
        0
    }
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    #[test]
    fn graphapp_image_rejects_invalid_dimensions_and_preserves_pixel_storage() {
        unsafe {
            assert!(newimage(-1, 1, 8).is_null());
            assert!(newimage(1, -1, 32).is_null());
            assert!(newimage(c_int::MAX, c_int::MAX, 32).is_null());
            for depth in [8, 32] {
                let img = newimage(2, 3, depth);
                assert!(!img.is_null());
                assert!(!(*img).pixels.is_null());
                let bytes = 6 * if depth == 8 {
                    1
                } else {
                    std::mem::size_of::<rgb>()
                };
                assert_eq!(
                    std::slice::from_raw_parts((*img).pixels, bytes),
                    vec![0; bytes]
                );
                delimage(img);
            }
        }
    }

    #[test]
    fn graphapp_image_can_replace_its_palette_from_the_current_palette() {
        unsafe {
            let img = newimage(2, 2, 8);
            assert!(!img.is_null());
            let mut colors = [rgb_make(1, 2, 3), rgb_make(4, 5, 6)];
            setpalette(img, 2, colors.as_mut_ptr());
            let old = (*img).cmap;
            assert!(!old.is_null());
            setpalette(img, 2, old);
            assert_eq!(std::slice::from_raw_parts((*img).cmap, 2), colors);
            setpalette(img, 0, ptr::null_mut());
            assert!((*img).cmap.is_null());
            assert_eq!((*img).cmapsize, 0);
            delimage(img);
        }
    }
}
