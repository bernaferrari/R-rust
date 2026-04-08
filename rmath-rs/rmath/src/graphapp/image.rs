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
            let pixels = memory::memalloc((width * height * 4) as i64);
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
        let byte_len = if (*img).depth > 8 { length * 4 } else { length };
        if !(*img).pixels.is_null() && !pixels.is_null() {
            ptr::copy_nonoverlapping(pixels, (*img).pixels, byte_len as usize);
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
        // TODO: Full implementation
        if img.is_null() {
            return ptr::null_mut();
        }
        if (*img).depth <= 8 {
            return copyimage(img);
        }
        // Stub: return a copy for now
        copyimage(img)
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
            let idx = if (value as c_int) >= (*img).cmapsize {
                ((*img).cmapsize - 1) as usize
            } else {
                value as usize
            };
            let col = if !(*img).cmap.is_null() {
                *(*img).cmap.add(idx)
            } else {
                Black
            };
            *pixel32.add(i) = col;
        }
        new_img
    }
}

/// Sort an image's colour map.
pub unsafe fn sortpalette(_img: image) {
    // TODO: Full implementation
}

/// Scale an image.
pub unsafe fn scaleimage(src: image, dr: rect, _sr: rect) -> image {
    unsafe {
        if src.is_null() {
            return ptr::null_mut();
        }
        let dest = newimage(dr.width, dr.height, (*src).depth);
        if dest.is_null() {
            return ptr::null_mut();
        }
        // TODO: Full scaling implementation
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
pub unsafe fn has_transparent_pixels(_img: image) -> c_int {
    // TODO: Full implementation
    0
}
