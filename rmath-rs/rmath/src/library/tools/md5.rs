/* md5.c - Functions to compute MD5 message digest of files or memory blocks
   according to the definition of MD5 in RFC 1321 from April 1992.
   Copyright (C) 1995, 1996, 2001 Free Software Foundation, Inc.

   Ported to Rust for the rmath-rs project.
   Based on R's r-source/src/library/tools/src/md5.c

   This program is free software; you can redistribute it and/or modify it
   under the terms of the GNU General Public License as published by the
   Free Software Foundation; either version 2, or (at your option) any
   later version.
*/

use core::ptr;
use libc::{FILE, c_int, c_void, size_t};
use libc::{ferror, fread};

type md5_uint32 = u32;

const BLOCKSIZE: usize = 4096;

/// Rotate a 32-bit integer left by n bits.
#[inline(always)]
fn rol(x: md5_uint32, n: u32) -> md5_uint32 {
    x.rotate_left(n)
}

/// On little-endian (which is what we target), SWAP is identity.
#[inline(always)]
const fn swap(n: md5_uint32) -> md5_uint32 {
    n
}

/// MD5 context structure mirroring C's `struct md5_ctx`.
#[repr(C)]
pub struct Md5Ctx {
    A: md5_uint32,
    B: md5_uint32,
    C: md5_uint32,
    D: md5_uint32,
    total: [md5_uint32; 2],
    buflen: md5_uint32,
    buffer: [u8; 128],
}

/// Padding bytes for MD5 (RFC 1321, 3.1: Step 1).
const FILLBUF: [u8; 64] = {
    let mut buf = [0u8; 64];
    buf[0] = 0x80;
    buf
};

// MD5 round functions (RFC 1321)
#[inline(always)]
fn ff(b: md5_uint32, c: md5_uint32, d: md5_uint32) -> md5_uint32 {
    d ^ (b & (c ^ d))
}

#[inline(always)]
fn fg(b: md5_uint32, c: md5_uint32, d: md5_uint32) -> md5_uint32 {
    ff(d, b, c)
}

#[inline(always)]
fn fh(b: md5_uint32, c: md5_uint32, d: md5_uint32) -> md5_uint32 {
    b ^ c ^ d
}

#[inline(always)]
fn fi(b: md5_uint32, c: md5_uint32, d: md5_uint32) -> md5_uint32 {
    c ^ (b | !d)
}

/// Initialize the MD5 computation context. (RFC 1321, 3.3: Step 3)
unsafe fn md5_init_ctx(ctx: *mut Md5Ctx) {
    (*ctx).A = 0x67452301;
    (*ctx).B = 0xefcdab89;
    (*ctx).C = 0x98badcfe;
    (*ctx).D = 0x10325476;
    (*ctx).total[0] = 0;
    (*ctx).total[1] = 0;
    (*ctx).buflen = 0;
}

/// Put result from CTX in first 16 bytes following RESBUF.
/// The result is in little endian byte order.
unsafe fn md5_read_ctx(ctx: *const Md5Ctx, resbuf: *mut c_void) -> *mut c_void {
    let resbuf32 = resbuf as *mut md5_uint32;
    *resbuf32.add(0) = swap((*ctx).A);
    *resbuf32.add(1) = swap((*ctx).B);
    *resbuf32.add(2) = swap((*ctx).C);
    *resbuf32.add(3) = swap((*ctx).D);
    resbuf
}

/// Process the remaining bytes in the internal buffer and the usual
/// prolog according to the standard and write the result to RESBUF.
unsafe fn md5_finish_ctx(ctx: *mut Md5Ctx, resbuf: *mut c_void) -> *mut c_void {
    let bytes = (*ctx).buflen;
    let pad: usize = if bytes >= 56 {
        (64 + 56 - bytes) as usize
    } else {
        (56 - bytes) as usize
    };

    // Count remaining bytes
    (*ctx).total[0] += bytes;
    if (*ctx).total[0] < bytes {
        (*ctx).total[1] += 1;
    }

    ptr::copy_nonoverlapping(
        FILLBUF.as_ptr(),
        (*ctx).buffer.as_mut_ptr().add(bytes as usize),
        pad,
    );

    // Put the 64-bit file length in *bits* at the end of the buffer
    let buf_ptr = (*ctx).buffer.as_mut_ptr() as *mut md5_uint32;
    *buf_ptr.add((bytes as usize + pad) / 4) = swap((*ctx).total[0] << 3);
    *buf_ptr.add((bytes as usize + pad + 4) / 4) =
        swap(((*ctx).total[1] << 3) | ((*ctx).total[0] >> 29));

    // Process last bytes
    md5_process_block(
        (*ctx).buffer.as_ptr() as *const c_void,
        (bytes as usize) + pad + 8,
        ctx,
    );

    md5_read_ctx(ctx, resbuf)
}

/// Process LEN bytes of BUFFER, accumulating context into CTX.
/// It is assumed that LEN % 64 == 0.
unsafe fn md5_process_block(buffer: *const c_void, len: usize, ctx: *mut Md5Ctx) {
    let words = buffer as *const md5_uint32;
    let nwords = len / core::mem::size_of::<md5_uint32>();
    let endp = words.add(nwords);
    let mut a = (*ctx).A;
    let mut b = (*ctx).B;
    let mut c = (*ctx).C;
    let mut d = (*ctx).D;

    // First increment the byte count
    (*ctx).total[0] += len as md5_uint32;
    if (*ctx).total[0] < len as md5_uint32 {
        (*ctx).total[1] += 1;
    }

    let mut wp = words;

    while wp < endp {
        let mut correct_words: [md5_uint32; 16] = [0; 16];
        let mut cwp: usize = 0;
        let a_save = a;
        let b_save = b;
        let c_save = c;
        let d_save = d;

        // Round 1: FF function
        // OP(a, b, c, d, s, T): a += FF(b,c,d) + SWAP(*words) + T; a = rol(a,s); a += b
        macro_rules! op1 {
            ($va:expr, $vb:expr, $vc:expr, $vd:expr, $s:expr, $t:expr) => {
                correct_words[cwp] = swap(*wp);
                cwp += 1;
                wp = wp.add(1);
                $va = $va
                    .wrapping_add(ff($vb, $vc, $vd))
                    .wrapping_add(correct_words[cwp - 1])
                    .wrapping_add($t);
                $va = rol($va, $s);
                $va = $va.wrapping_add($vb);
            };
        }

        op1!(a, b, c, d, 7, 0xd76aa478);
        op1!(d, a, b, c, 12, 0xe8c7b756);
        op1!(c, d, a, b, 17, 0x242070db);
        op1!(b, c, d, a, 22, 0xc1bdceee);
        op1!(a, b, c, d, 7, 0xf57c0faf);
        op1!(d, a, b, c, 12, 0x4787c62a);
        op1!(c, d, a, b, 17, 0xa8304613);
        op1!(b, c, d, a, 22, 0xfd469501);
        op1!(a, b, c, d, 7, 0x698098d8);
        op1!(d, a, b, c, 12, 0x8b44f7af);
        op1!(c, d, a, b, 17, 0xffff5bb1);
        op1!(b, c, d, a, 22, 0x895cd7be);
        op1!(a, b, c, d, 7, 0x6b901122);
        op1!(d, a, b, c, 12, 0xfd987193);
        op1!(c, d, a, b, 17, 0xa679438e);
        op1!(b, c, d, a, 22, 0x49b40821);

        // Rounds 2-4: OP(f, a, b, c, d, k, s, T)
        macro_rules! op {
            ($f:expr, $va:expr, $vb:expr, $vc:expr, $vd:expr, $k:expr, $s:expr, $t:expr) => {
                $va = $va
                    .wrapping_add($f($vb, $vc, $vd))
                    .wrapping_add(correct_words[$k])
                    .wrapping_add($t);
                $va = rol($va, $s);
                $va = $va.wrapping_add($vb);
            };
        }

        // Round 2: FG function
        op!(fg, a, b, c, d, 1, 5, 0xf61e2562);
        op!(fg, d, a, b, c, 6, 9, 0xc040b340);
        op!(fg, c, d, a, b, 11, 14, 0x265e5a51);
        op!(fg, b, c, d, a, 0, 20, 0xe9b6c7aa);
        op!(fg, a, b, c, d, 5, 5, 0xd62f105d);
        op!(fg, d, a, b, c, 10, 9, 0x02441453);
        op!(fg, c, d, a, b, 15, 14, 0xd8a1e681);
        op!(fg, b, c, d, a, 4, 20, 0xe7d3fbc8);
        op!(fg, a, b, c, d, 9, 5, 0x21e1cde6);
        op!(fg, d, a, b, c, 14, 9, 0xc33707d6);
        op!(fg, c, d, a, b, 3, 14, 0xf4d50d87);
        op!(fg, b, c, d, a, 8, 20, 0x455a14ed);
        op!(fg, a, b, c, d, 13, 5, 0xa9e3e905);
        op!(fg, d, a, b, c, 2, 9, 0xfcefa3f8);
        op!(fg, c, d, a, b, 7, 14, 0x676f02d9);
        op!(fg, b, c, d, a, 12, 20, 0x8d2a4c8a);

        // Round 3: FH function
        op!(fh, a, b, c, d, 5, 4, 0xfffa3942);
        op!(fh, d, a, b, c, 8, 11, 0x8771f681);
        op!(fh, c, d, a, b, 11, 16, 0x6d9d6122);
        op!(fh, b, c, d, a, 14, 23, 0xfde5380c);
        op!(fh, a, b, c, d, 1, 4, 0xa4beea44);
        op!(fh, d, a, b, c, 4, 11, 0x4bdecfa9);
        op!(fh, c, d, a, b, 7, 16, 0xf6bb4b60);
        op!(fh, b, c, d, a, 10, 23, 0xbebfbc70);
        op!(fh, a, b, c, d, 13, 4, 0x289b7ec6);
        op!(fh, d, a, b, c, 0, 11, 0xeaa127fa);
        op!(fh, c, d, a, b, 3, 16, 0xd4ef3085);
        op!(fh, b, c, d, a, 6, 23, 0x04881d05);
        op!(fh, a, b, c, d, 9, 4, 0xd9d4d039);
        op!(fh, d, a, b, c, 12, 11, 0xe6db99e5);
        op!(fh, c, d, a, b, 15, 16, 0x1fa27cf8);
        op!(fh, b, c, d, a, 2, 23, 0xc4ac5665);

        // Round 4: FI function
        op!(fi, a, b, c, d, 0, 6, 0xf4292244);
        op!(fi, d, a, b, c, 7, 10, 0x432aff97);
        op!(fi, c, d, a, b, 14, 15, 0xab9423a7);
        op!(fi, b, c, d, a, 5, 21, 0xfc93a039);
        op!(fi, a, b, c, d, 12, 6, 0x655b59c3);
        op!(fi, d, a, b, c, 3, 10, 0x8f0ccc92);
        op!(fi, c, d, a, b, 10, 15, 0xffeff47d);
        op!(fi, b, c, d, a, 1, 21, 0x85845dd1);
        op!(fi, a, b, c, d, 8, 6, 0x6fa87e4f);
        op!(fi, d, a, b, c, 15, 10, 0xfe2ce6e0);
        op!(fi, c, d, a, b, 6, 15, 0xa3014314);
        op!(fi, b, c, d, a, 13, 21, 0x4e0811a1);
        op!(fi, a, b, c, d, 4, 6, 0xf7537e82);
        op!(fi, d, a, b, c, 11, 10, 0xbd3af235);
        op!(fi, c, d, a, b, 2, 15, 0x2ad7d2bb);
        op!(fi, b, c, d, a, 9, 21, 0xeb86d391);

        // Add the starting values of the context
        a = a.wrapping_add(a_save);
        b = b.wrapping_add(b_save);
        c = c.wrapping_add(c_save);
        d = d.wrapping_add(d_save);
    }

    // Put checksum in context
    (*ctx).A = a;
    (*ctx).B = b;
    (*ctx).C = c;
    (*ctx).D = d;
}

/// Feed arbitrary bytes into the MD5 computation.
unsafe fn md5_process_bytes(buffer: *const c_void, mut len: size_t, ctx: *mut Md5Ctx) {
    let mut buf = buffer as *const u8;

    // When we already have some bits in our internal buffer, concatenate both inputs first
    if (*ctx).buflen != 0 {
        let left_over = (*ctx).buflen as usize;
        let add = if 128 - left_over > len {
            len
        } else {
            128 - left_over
        };

        ptr::copy_nonoverlapping(buf, (*ctx).buffer.as_mut_ptr().add(left_over), add);
        (*ctx).buflen += add as md5_uint32;

        if left_over + add > 64 {
            md5_process_block(
                (*ctx).buffer.as_ptr() as *const c_void,
                (left_over + add) & !63,
                ctx,
            );
            // The regions in the following copy cannot overlap
            ptr::copy_nonoverlapping(
                (*ctx).buffer.as_ptr().add((left_over + add) & !63),
                (*ctx).buffer.as_mut_ptr(),
                (left_over + add) & 63,
            );
            (*ctx).buflen = ((left_over + add) & 63) as md5_uint32;
        }

        buf = buf.add(add);
        len -= add;
    }

    // Process available complete blocks
    if len > 64 {
        md5_process_block(buf as *const c_void, len & !63, ctx);
        buf = buf.add(len & !63);
        len &= 63;
    }

    // Move remaining bytes into internal buffer
    if len > 0 {
        ptr::copy_nonoverlapping(buf, (*ctx).buffer.as_mut_ptr(), len);
        (*ctx).buflen = len as md5_uint32;
    }
}

/// Compute MD5 message digest for bytes read from STREAM.
/// The resulting message digest will be written into the 16 bytes beginning at RESBLOCK.
/// Returns 0 on success, 1 on error.
pub unsafe fn md5_stream(stream: *mut FILE, resblock: *mut c_void) -> c_int {
    let mut ctx = Md5Ctx {
        A: 0,
        B: 0,
        C: 0,
        D: 0,
        total: [0; 2],
        buflen: 0,
        buffer: [0u8; 128],
    };
    // buffer: BLOCKSIZE + 72
    let mut buffer: [u8; BLOCKSIZE + 72] = [0; BLOCKSIZE + 72];

    md5_init_ctx(&mut ctx);

    let mut sum: usize = 0;
    loop {
        sum = 0;
        let mut n: usize = 0;

        // Read block. Take care for partial reads.
        loop {
            n = fread(
                buffer.as_mut_ptr().add(sum) as *mut c_void,
                1,
                BLOCKSIZE - sum,
                stream,
            );
            sum += n;
            if !(sum < BLOCKSIZE && n != 0) {
                break;
            }
        }

        if n == 0 && ferror(stream) != 0 {
            return 1;
        }

        // If end of file is reached, end the loop
        if n == 0 {
            break;
        }

        // Process buffer with BLOCKSIZE bytes
        md5_process_block(buffer.as_ptr() as *const c_void, BLOCKSIZE, &mut ctx);
    }

    // Add the last bytes if necessary
    if sum > 0 {
        md5_process_bytes(buffer.as_ptr() as *const c_void, sum, &mut ctx);
    }

    // Construct result in desired memory
    md5_finish_ctx(&mut ctx, resblock);
    0
}

/// Compute MD5 message digest for LEN bytes beginning at BUFFER.
/// The result is always in little endian byte order.
/// Returns resblock on success.
pub unsafe fn md5_buffer(buffer: *const u8, len: size_t, resblock: *mut c_void) -> *mut c_void {
    let mut ctx = Md5Ctx {
        A: 0,
        B: 0,
        C: 0,
        D: 0,
        total: [0; 2],
        buflen: 0,
        buffer: [0u8; 128],
    };

    md5_init_ctx(&mut ctx);

    // Process whole buffer
    md5_process_bytes(buffer as *const c_void, len, &mut ctx);

    // Put result in desired memory area
    md5_finish_ctx(&mut ctx, resblock)
}
