/* SHA256 implementation.
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2003-2024   The R Core Team.
 *  Based on code released into the Public Domain by
 *  Ulrich Drepper <drepper@redhat.com>.
 *
 *  Ported to Rust for the rmath-rs project.
 *  Based on R's r-source/src/library/tools/src/sha256.c
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

use core::ptr;
use libc::{FILE, c_int, c_void, size_t};
use libc::{ferror, fread};

const BUF_SIZE: usize = 4096;

/// On little-endian (which is what we target), SWAP is identity.
#[inline(always)]
const fn swap(n: u32) -> u32 {
    n
}

/// SHA256 context structure mirroring C's `struct sha256_ctx`.
#[repr(C)]
pub struct Sha256Ctx {
    pub H: [u32; 8],
    pub total: [u32; 2],
    pub buflen: u32,
    pub buffer: [u8; 128],
}

/// Padding bytes (FIPS 180-2:5.1.1).
const FILLBUF: [u8; 64] = {
    let mut buf = [0u8; 64];
    buf[0] = 0x80;
    buf
};

/// Constants for SHA256 from FIPS 180-2:4.2.2.
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

// SHA256 operators defined in FIPS 180-2:4.1.2
#[inline(always)]
fn ch(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (!x & z)
}

#[inline(always)]
fn maj(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (x & z) ^ (y & z)
}

#[inline(always)]
fn s0(x: u32) -> u32 {
    cyclic(x, 2) ^ cyclic(x, 13) ^ cyclic(x, 22)
}

#[inline(always)]
fn s1(x: u32) -> u32 {
    cyclic(x, 6) ^ cyclic(x, 11) ^ cyclic(x, 25)
}

#[inline(always)]
fn r0(x: u32) -> u32 {
    cyclic(x, 7) ^ cyclic(x, 18) ^ (x >> 3)
}

#[inline(always)]
fn r1(x: u32) -> u32 {
    cyclic(x, 17) ^ cyclic(x, 19) ^ (x >> 10)
}

/// Cyclic (right) rotation of a 32-bit word.
#[inline(always)]
fn cyclic(w: u32, s: u32) -> u32 {
    w.rotate_right(s)
}

/// Process LEN bytes of BUFFER, accumulating context into CTX.
/// It is assumed that LEN % 64 == 0.
unsafe fn sha256_process_block(buffer: *const c_void, len: usize, ctx: *mut Sha256Ctx) {
    let mut words = buffer as *const u32;
    let mut nwords = len / core::mem::size_of::<u32>();
    let mut a = (*ctx).H[0];
    let mut b = (*ctx).H[1];
    let mut c = (*ctx).H[2];
    let mut d = (*ctx).H[3];
    let mut e = (*ctx).H[4];
    let mut f = (*ctx).H[5];
    let mut g = (*ctx).H[6];
    let mut h = (*ctx).H[7];

    // First increment the byte count
    (*ctx).total[0] += len as u32;
    if (*ctx).total[0] < len as u32 {
        (*ctx).total[1] += 1;
    }

    // Process all bytes in the buffer with 64 bytes in each round
    while nwords > 0 {
        let mut w: [u32; 64] = [0; 64];
        let a_save = a;
        let b_save = b;
        let c_save = c;
        let d_save = d;
        let e_save = e;
        let f_save = f;
        let g_save = g;
        let h_save = h;

        // Compute the message schedule (FIPS 180-2:6.2.2 step 2)
        let mut t: usize = 0;
        while t < 16 {
            w[t] = swap(*words);
            words = words.add(1);
            t += 1;
        }
        t = 16;
        while t < 64 {
            w[t] = r1(w[t - 2])
                .wrapping_add(w[t - 7])
                .wrapping_add(r0(w[t - 15]))
                .wrapping_add(w[t - 16]);
            t += 1;
        }

        // The actual computation (FIPS 180-2:6.2.2 step 3)
        t = 0;
        while t < 64 {
            let t1 = h
                .wrapping_add(s1(e))
                .wrapping_add(ch(e, f, g))
                .wrapping_add(K[t])
                .wrapping_add(w[t]);
            let t2 = s0(a).wrapping_add(maj(a, b, c));
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
            t += 1;
        }

        // Add the starting values of the context (FIPS 180-2:6.2.2 step 4)
        a = a.wrapping_add(a_save);
        b = b.wrapping_add(b_save);
        c = c.wrapping_add(c_save);
        d = d.wrapping_add(d_save);
        e = e.wrapping_add(e_save);
        f = f.wrapping_add(f_save);
        g = g.wrapping_add(g_save);
        h = h.wrapping_add(h_save);

        nwords -= 16;
    }

    // Put checksum in context
    (*ctx).H[0] = a;
    (*ctx).H[1] = b;
    (*ctx).H[2] = c;
    (*ctx).H[3] = d;
    (*ctx).H[4] = e;
    (*ctx).H[5] = f;
    (*ctx).H[6] = g;
    (*ctx).H[7] = h;
}

/// Initialize the SHA256 computation context. (FIPS 180-2:5.3.2)
pub unsafe fn Rsha256_init_ctx(ctx: *mut Sha256Ctx) {
    (*ctx).H[0] = 0x6a09e667;
    (*ctx).H[1] = 0xbb67ae85;
    (*ctx).H[2] = 0x3c6ef372;
    (*ctx).H[3] = 0xa54ff53a;
    (*ctx).H[4] = 0x510e527f;
    (*ctx).H[5] = 0x9b05688c;
    (*ctx).H[6] = 0x1f83d9ab;
    (*ctx).H[7] = 0x5be0cd19;
    (*ctx).total[0] = 0;
    (*ctx).total[1] = 0;
    (*ctx).buflen = 0;
}

/// Process the remaining bytes in the internal buffer and the usual
/// prolog according to the standard and write the result to RESBUF.
pub unsafe fn Rsha256_finish_ctx(ctx: *mut Sha256Ctx, resbuf: *mut c_void) -> *mut c_void {
    let bytes = (*ctx).buflen;
    let pad: usize = if bytes >= 56 {
        (64 + 56 - bytes) as usize
    } else {
        (56 - bytes) as usize
    };

    // Now count remaining bytes
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
    let buf_ptr = (*ctx).buffer.as_mut_ptr() as *mut u32;
    *buf_ptr.add((bytes as usize + pad + 4) / 4) = swap((*ctx).total[0] << 3);
    *buf_ptr.add((bytes as usize + pad) / 4) =
        swap(((*ctx).total[1] << 3) | ((*ctx).total[0] >> 29));

    // Process last bytes
    sha256_process_block(
        (*ctx).buffer.as_ptr() as *const c_void,
        (bytes as usize) + pad + 8,
        ctx,
    );

    // Put result from CTX in first 32 bytes following RESBUF
    let resbuf32 = resbuf as *mut u32;
    let mut i: usize = 0;
    while i < 8 {
        *resbuf32.add(i) = swap((*ctx).H[i]);
        i += 1;
    }

    resbuf
}

/// Feed arbitrary bytes into the SHA256 computation.
pub unsafe fn Rsha256_process_bytes(buffer: *const c_void, mut len: size_t, ctx: *mut Sha256Ctx) {
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
        (*ctx).buflen += add as u32;

        if (*ctx).buflen > 64 {
            sha256_process_block(
                (*ctx).buffer.as_ptr() as *const c_void,
                (*ctx).buflen as usize & !63,
                ctx,
            );

            (*ctx).buflen &= 63;
            // The regions in the following copy operation cannot overlap
            ptr::copy_nonoverlapping(
                (*ctx).buffer.as_ptr().add((left_over + add) & !63),
                (*ctx).buffer.as_mut_ptr(),
                (*ctx).buflen as usize,
            );
        }

        buf = buf.add(add);
        len -= add;
    }

    // Process available complete blocks
    if len >= 64 {
        // Check alignment
        let aligned = ((buf as usize) % core::mem::size_of::<u32>()) == 0;
        if !aligned {
            while len > 64 {
                ptr::copy_nonoverlapping(buf, (*ctx).buffer.as_mut_ptr(), 64);
                sha256_process_block((*ctx).buffer.as_ptr() as *const c_void, 64, ctx);
                buf = buf.add(64);
                len -= 64;
            }
        } else {
            sha256_process_block(buf as *const c_void, len & !63, ctx);
            buf = buf.add(len & !63);
            len &= 63;
        }
    }

    // Move remaining bytes into internal buffer
    if len > 0 {
        let left_over = (*ctx).buflen as usize;

        ptr::copy_nonoverlapping(buf, (*ctx).buffer.as_mut_ptr().add(left_over), len);
        let mut new_left_over = left_over + len;
        if new_left_over >= 64 {
            sha256_process_block((*ctx).buffer.as_ptr() as *const c_void, 64, ctx);
            new_left_over -= 64;
            ptr::copy_nonoverlapping(
                (*ctx).buffer.as_ptr().add(64),
                (*ctx).buffer.as_mut_ptr(),
                new_left_over,
            );
        }
        (*ctx).buflen = new_left_over as u32;
    }
}

/// Compute SHA256 message digest for bytes read from STREAM.
/// Returns 0 on success, 1 on error.
pub unsafe fn Rsha256_stream(stream: *mut FILE, resblock: *mut c_void) -> c_int {
    let mut ctx = Sha256Ctx {
        H: [0; 8],
        total: [0; 2],
        buflen: 0,
        buffer: [0u8; 128],
    };
    let mut buffer: [u8; BUF_SIZE] = [0; BUF_SIZE];
    let mut sum: usize = 0;

    Rsha256_init_ctx(&mut ctx);

    loop {
        let mut n: size_t = 0;
        // Read next block
        while sum < BUF_SIZE {
            n = fread(
                buffer.as_mut_ptr().add(sum) as *mut c_void,
                1,
                BUF_SIZE - sum,
                stream,
            );
            if n == 0 {
                break;
            }
            sum += n;
        }

        if n == 0 {
            if ferror(stream) != 0 {
                return 1;
            }
            if sum < BUF_SIZE {
                break;
            }
        }

        // Full block
        sha256_process_block(buffer.as_ptr() as *const c_void, BUF_SIZE, &mut ctx);
        sum = 0;
    }

    // Add any remaining bytes
    if sum > 0 {
        Rsha256_process_bytes(buffer.as_ptr() as *const c_void, sum, &mut ctx);
    }

    Rsha256_finish_ctx(&mut ctx, resblock);
    0
}
