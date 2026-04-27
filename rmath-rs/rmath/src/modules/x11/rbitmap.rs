
//! X11 bitmap save routines (rbitmap.c)
//!
//! Port of R's bitmap image writers: PNG, JPEG, TIFF, BMP.
//! These routines are called from devX11.c to save X11 window
//! contents to files.
//!
//! The BMP writer (R_SaveAsBmp) is a pure algorithm with no X11
//! dependency, so it is ported with a real implementation.
//! PNG/JPEG/TIFF remain as stubs since they require external libraries.
//!
//! NOTE: The #[unsafe(no_mangle)] versions of R_SaveAsPng/Jpeg/TIFF/Bmp
//! already exist in library/grdevices/winbitmap.rs.  This module provides
//! module-private stubs so that the code compiles without duplicate
//! symbol errors.  The real BMP implementation is provided as a
//! pub(crate) function for use within this module.

use core::ffi::{c_char, c_int, c_uint, c_void};

// ── Color channel extraction macros ──────────────────────────────────

/// Compute bit shifts for BGR vs RGB color order.
/// Returns (r_shift, g_shift, b_shift).
const fn compute_shifts(bgr: bool) -> (u32, u32, u32) {
    if bgr { (0, 8, 16) } else { (16, 8, 0) }
}

#[inline(always)]
fn get_red(col: u32, rshift: u32) -> u8 {
    ((col >> rshift) & 0xFF) as u8
}

#[inline(always)]
fn get_green(col: u32, _gshift: u32) -> u8 {
    ((col >> 8) & 0xFF) as u8
}

#[inline(always)]
fn get_blue(col: u32, bshift: u32) -> u8 {
    ((col >> bshift) & 0xFF) as u8
}

#[inline(always)]
fn get_alpha(col: u32) -> u8 {
    ((col >> 24) & 0xFF) as u8
}

// ── BMP writing helpers ──────────────────────────────────────────────

const BMP_HEADERSIZE: u32 = 54;

/// Write a little-endian 16-bit word to a FILE.
unsafe fn bmpw(x: u16, fp: *mut c_void) {
    let bytes = x.to_le_bytes();
    unsafe {
        libc::fwrite(bytes.as_ptr() as *const c_void, 2, 1, fp as *mut libc::FILE);
    }
}

/// Write a little-endian 32-bit double word to a FILE.
unsafe fn bmpdw(x: u32, fp: *mut c_void) {
    let bytes = x.to_le_bytes();
    unsafe {
        libc::fwrite(bytes.as_ptr() as *const c_void, 4, 1, fp as *mut libc::FILE);
    }
}

/// Write a single byte to a FILE.
unsafe fn bmpputc(a: u8, fp: *mut c_void) -> bool {
    unsafe { libc::fputc(a as c_int, fp as *mut libc::FILE) != libc::EOF }
}

// ── Real BMP writer implementation ───────────────────────────────────

/// Save device contents as Windows BMP.
///
/// This is a platform-independent implementation of the BMP format writer.
/// If the number of distinct colors is less than 256, an 8-bit palette
/// BMP is produced; otherwise a 24-bit truecolor BMP is written.
///
/// Returns 1 on success, 0 on failure.
///
/// Ported from r-source/src/modules/X11/rbitmap.c R_SaveAsBmp().
pub(crate) unsafe fn save_as_bmp(
    d: *mut c_void,
    width: c_int,
    height: c_int,
    gp: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int) -> c_uint>,
    bgr: c_int,
    fp: *mut c_void,
    res: c_int,
) -> c_int {
    if fp.is_null() {
        return 0;
    }

    let gp_fn = match gp {
        Some(f) => f,
        None => return 0,
    };
    let pixel_at = |row: u32, col: u32| -> u32 { unsafe { gp_fn(d, row as c_int, col as c_int) } };
    let put_byte = |value: u8| -> bool { unsafe { bmpputc(value, fp) } };
    let write_u16 = |value: u16| unsafe { bmpw(value, fp) };
    let write_u32 = |value: u32| unsafe { bmpdw(value, fp) };

    let w = width as u32;
    let h = height as u32;
    let (rshift, _gshift, bshift) = compute_shifts(bgr != 0);

    // Build palette: try to fit into 256 colors
    let mut palette: [u32; 256] = [0; 256];
    let mut ncols: usize = 0;
    let mut withpalette = true;

    for i in 0..h {
        if !withpalette {
            break;
        }
        for j in 0..w {
            if !withpalette {
                break;
            }
            let col = pixel_at(i, j) & 0xFFFFFF;

            // Binary search the palette
            let mut low: isize = 0;
            let mut high: isize = ncols as isize - 1;
            let mut mid: isize = 0;
            while low <= high {
                mid = (low + high) / 2;
                if col < palette[mid as usize] {
                    high = mid - 1;
                } else if col > palette[mid as usize] {
                    low = mid + 1;
                } else {
                    break;
                }
            }

            if high < low {
                // Didn't find colour in palette, insert it
                if ncols >= 256 {
                    withpalette = false;
                } else {
                    let insert_pos = low as usize;
                    for r in (insert_pos + 1..=ncols).rev() {
                        palette[r] = palette[r - 1];
                    }
                    palette[insert_pos] = col;
                    ncols += 1;
                }
            }
        }
    }

    // Compute header fields
    let (bf_off_bits, bf_size, bi_bit_count, bi_clr_used): (u32, u32, u16, u32);
    if withpalette {
        bf_off_bits = BMP_HEADERSIZE + 4 * 256;
        bf_size = bf_off_bits + w * h;
        bi_bit_count = 8;
        bi_clr_used = 256;
    } else {
        bf_off_bits = BMP_HEADERSIZE + 4;
        bf_size = bf_off_bits + 3 * w * h;
        bi_bit_count = 24;
        bi_clr_used = 0;
    }

    // Write the BMP file header (14 bytes)
    if !put_byte(b'B') || !put_byte(b'M') {
        return 0;
    }
    write_u32(bf_size); // bfSize
    write_u16(0); // bfReserved1
    write_u16(0); // bfReserved2
    write_u32(bf_off_bits); // bfOffBits

    // Write the DIB header (BITMAPINFOHEADER, 40 bytes)
    write_u32(40); // biSize (Windows V3)
    write_u32(w); // biWidth
    write_u32(h); // biHeight (positive = bottom-up)
    write_u16(1); // biPlanes
    write_u16(bi_bit_count); // biBitCount
    write_u32(0); // biCompression = BI_RGB
    write_u32(0); // biSizeImage (not needed for BI_RGB)

    // Resolution: pixels per metre
    let lres: u32 = if res > 0 {
        (0.5 + res as f64 / 0.0254) as u32
    } else {
        2835 // 72 ppi = 2835 pixels/metre
    };
    write_u32(lres); // biXPelsPerMeter
    write_u32(lres); // biYPelsPerMeter
    write_u32(bi_clr_used); // biClrUsed
    write_u32(0); // biClrImportant

    // Write the image data
    if withpalette {
        // 8-bit image: write the palette (256 entries, BGRA format)
        for i in 0..256 {
            let col = palette[i];
            let blue = get_blue(col, bshift);
            let green = get_green(col, 8);
            let red = get_red(col, rshift);
            if !put_byte(blue) || !put_byte(green) || !put_byte(red) || !put_byte(0) {
                return 0;
            }
        }

        // Rows must be padded to 4-byte boundary
        let mut pad: u32 = 0;
        while (w + pad) & 3 != 0 {
            pad += 1;
        }

        // BMP rows are bottom-up
        for i in (0..h).rev() {
            for j in 0..w {
                let col = pixel_at(i, j) & 0xFFFFFF;

                // Binary search the palette (colour must be there)
                let mut low: isize = 0;
                let mut high: isize = ncols as isize - 1;
                let mut mid: isize = 0;
                while low <= high {
                    mid = (low + high) / 2;
                    if col < palette[mid as usize] {
                        high = mid - 1;
                    } else if col > palette[mid as usize] {
                        low = mid + 1;
                    } else {
                        break;
                    }
                }

                if !put_byte(mid as u8) {
                    return 0;
                }
            }
            // Write padding bytes
            for _ in 0..pad {
                if !put_byte(0) {
                    return 0;
                }
            }
        }
    } else {
        // 24-bit image: write null bmiColors entry
        write_u32(0);

        // Row stride must be padded to 4-byte boundary
        let row_bytes = 3 * w;
        let mut pad: u32 = 0;
        while (row_bytes + pad) & 3 != 0 {
            pad += 1;
        }

        // BMP rows are bottom-up
        for i in (0..h).rev() {
            for j in 0..w {
                let col = pixel_at(i, j) & 0xFFFFFF;
                let blue = get_blue(col, bshift);
                let green = get_green(col, 8);
                let red = get_red(col, rshift);
                if !put_byte(blue) || !put_byte(green) || !put_byte(red) {
                    return 0;
                }
            }
            // Write padding bytes
            for _ in 0..pad {
                if !put_byte(0) {
                    return 0;
                }
            }
        }
    }

    1 // success
}

// ── Module-private stubs (no_mangle in winbitmap.rs) ─────────────────
//
// The #[unsafe(no_mangle)] versions live in
//   library/grdevices/winbitmap.rs
// so we keep these as module-private (plain unsafe fn).

/// R_SaveAsPng - save device content as a PNG file.
/// Returns 0 on failure (no libpng linked).
unsafe fn R_SaveAsPng(
    _d: *mut c_void,
    _width: c_int,
    _height: c_int,
    _gp: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int) -> u32>,
    _bgr: c_int,
    _fp: *mut c_void,
    _transparent: u32,
    _res: c_int,
) -> c_int {
    0
}

/// R_SaveAsJpeg - save device content as a JPEG file.
/// Returns 0 on failure (no libjpeg linked).
unsafe fn R_SaveAsJpeg(
    _d: *mut c_void,
    _width: c_int,
    _height: c_int,
    _gp: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int) -> u32>,
    _bgr: c_int,
    _quality: c_int,
    _outfile: *mut c_void,
    _res: c_int,
) -> c_int {
    0
}

/// R_SaveAsTIFF - save device content as a TIFF file.
/// Returns 0 on failure (no libtiff linked).
unsafe fn R_SaveAsTIFF(
    _d: *mut c_void,
    _width: c_int,
    _height: c_int,
    _gp: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int) -> u32>,
    _bgr: c_int,
    _outfile: *const c_char,
    _res: c_int,
    _compression: c_int,
) -> c_int {
    0
}

/// R_SaveAsBmp - save device content as a BMP file.
/// Module-private version that delegates to the real implementation.
unsafe fn R_SaveAsBmp(
    d: *mut c_void,
    width: c_int,
    height: c_int,
    gp: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int) -> u32>,
    bgr: c_int,
    fp: *mut c_void,
    res: c_int,
) -> c_int {
    unsafe { save_as_bmp(d, width, height, gp, bgr, fp, res) }
}
