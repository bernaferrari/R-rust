/*!
 * xdr.rs -- Rust port of R's XDR (External Data Representation) library.
 *
 * Ported from the Oracle/Sun RPC XDR implementation (BSD license),
 * as used in R's src/extra/xdr/.
 *
 * Original files:
 *   xdr.c       - Generic XDR routines
 *   xdr_float.c - xdr_double
 *   xdr_mem.c   - Memory-based XDR stream
 *   xdr_stdio.c - FILE-based XDR stream
 *   rpc/types.h - Type definitions
 *   rpc/xdr.h   - XDR struct and function declarations
 */

use std::ffi::{CStr, c_int, c_uint};
use std::fs::File;
use std::io::{Read as IoRead, Seek as IoSeek, SeekFrom, Write as IoWrite};
use std::ptr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// C-compatible boolean: TRUE
const TRUE: c_int = 1;
/// C-compatible boolean: FALSE
const FALSE: c_int = 0;

/// Bytes per XDR unit (always 4).
const BYTES_PER_XDR_UNIT: usize = 4;

/// Static zero bytes used for XDR padding during encode.
const XDR_ZERO: [u8; BYTES_PER_XDR_UNIT] = [0, 0, 0, 0];

// ---------------------------------------------------------------------------
// Byte-swapping (network <-> host order)
// ---------------------------------------------------------------------------

/// Swap bytes of a 32-bit value (ntohl / htonl on little-endian).
#[cfg(target_endian = "little")]
#[inline]
fn ntohl(x: u32) -> u32 {
    (x << 24) | ((x & 0xff00) << 8) | ((x & 0xff0000) >> 8) | (x >> 24)
}

/// On big-endian, ntohl is identity.
#[cfg(target_endian = "big")]
#[inline]
fn ntohl(x: u32) -> u32 {
    x
}

/// htonl is the same as ntohl for our purposes.
#[inline]
fn htonl(x: u32) -> u32 {
    ntohl(x)
}

// ---------------------------------------------------------------------------
// XdrOp enum
// ---------------------------------------------------------------------------

/// XDR operation type, matching C's `enum xdr_op`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XdrOp {
    Encode = 0,
    Decode = 1,
    Free = 2,
}

// ---------------------------------------------------------------------------
// Backend type
// ---------------------------------------------------------------------------

/// Identifies which backend (memory or stdio) an XDR stream uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XdrBackend {
    Mem,
    Stdio,
}

// ---------------------------------------------------------------------------
// Xdr struct
// ---------------------------------------------------------------------------

/// The XDR handle, analogous to C's `struct XDR`.
///
/// Contains the operation being applied, the backend type, and private
/// fields used by the particular backend implementation.
pub(crate) struct Xdr {
    /// The current operation (encode, decode, or free).
    x_op: XdrOp,

    /// Which backend implementation is in use.
    backend: XdrBackend,

    /// Pointer to the start of the buffer (memory backend).
    x_base: *mut u8,

    /// Private pointer: current position in buffer (memory) or FILE* (stdio).
    x_private: *mut u8,

    /// Extra private word: remaining bytes (memory) or 0 (stdio).
    x_handy: c_int,
}

impl Xdr {
    // -----------------------------------------------------------------------
    // Dispatch methods -- call the correct backend implementation
    // -----------------------------------------------------------------------

    fn getlong(&mut self, lp: &mut i32) -> c_int {
        match self.backend {
            XdrBackend::Mem => mem_getlong(self, lp),
            XdrBackend::Stdio => stdio_getlong(self, lp),
        }
    }

    fn putlong(&mut self, lp: &i32) -> c_int {
        match self.backend {
            XdrBackend::Mem => mem_putlong(self, lp),
            XdrBackend::Stdio => stdio_putlong(self, lp),
        }
    }

    fn getbytes(&mut self, addr: &mut [u8]) -> c_int {
        match self.backend {
            XdrBackend::Mem => mem_getbytes(self, addr),
            XdrBackend::Stdio => stdio_getbytes(self, addr),
        }
    }

    fn putbytes(&mut self, addr: &[u8]) -> c_int {
        match self.backend {
            XdrBackend::Mem => mem_putbytes(self, addr),
            XdrBackend::Stdio => stdio_putbytes(self, addr),
        }
    }

    fn getpos(&self) -> c_uint {
        match self.backend {
            XdrBackend::Mem => mem_getpos(self),
            XdrBackend::Stdio => stdio_getpos(self),
        }
    }

    fn setpos(&mut self, pos: c_uint) -> c_int {
        match self.backend {
            XdrBackend::Mem => mem_setpos(self, pos),
            XdrBackend::Stdio => stdio_setpos(self, pos),
        }
    }

    fn destroy(&mut self) {
        match self.backend {
            XdrBackend::Mem => {
                // Nothing to free for memory backend
            }
            XdrBackend::Stdio => {
                if self.x_op == XdrOp::Encode {
                    let file = unsafe { &mut *(self.x_private as *mut File) };
                    let _ = file.flush();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// XdrMem backend implementation (from xdr_mem.c)
// ---------------------------------------------------------------------------

/// Memory-based getlong: read 4 bytes, byte-swap, advance position.
fn mem_getlong(xdrs: &mut Xdr, lp: &mut i32) -> c_int {
    if xdrs.x_handy < 4 {
        return FALSE;
    }
    xdrs.x_handy -= 4;
    // Read 4 bytes from x_private and byte-swap
    let src = xdrs.x_private as *const i32;
    let val = unsafe { ptr::read(src) };
    *lp = ntohl(val as u32) as i32;
    xdrs.x_private = xdrs.x_private.wrapping_add(4);
    TRUE
}

/// Memory-based putlong: byte-swap, write 4 bytes, advance position.
fn mem_putlong(xdrs: &mut Xdr, lp: &i32) -> c_int {
    if xdrs.x_handy < 4 {
        return FALSE;
    }
    xdrs.x_handy -= 4;
    let dst = xdrs.x_private as *mut i32;
    let swapped = htonl(*lp as u32) as i32;
    unsafe {
        ptr::write(dst, swapped);
    }
    xdrs.x_private = xdrs.x_private.wrapping_add(4);
    TRUE
}

/// Memory-based getbytes: copy bytes from buffer.
fn mem_getbytes(xdrs: &mut Xdr, addr: &mut [u8]) -> c_int {
    let len = addr.len();
    if len == 0 {
        return TRUE;
    }
    if (xdrs.x_handy as usize) < len {
        return FALSE;
    }
    xdrs.x_handy -= len as c_int;
    unsafe {
        ptr::copy_nonoverlapping(xdrs.x_private, addr.as_mut_ptr(), len);
    }
    xdrs.x_private = xdrs.x_private.wrapping_add(len);
    TRUE
}

/// Memory-based putbytes: copy bytes into buffer.
fn mem_putbytes(xdrs: &mut Xdr, addr: &[u8]) -> c_int {
    let len = addr.len();
    if len == 0 {
        return TRUE;
    }
    if (xdrs.x_handy as usize) < len {
        return FALSE;
    }
    xdrs.x_handy -= len as c_int;
    unsafe {
        ptr::copy_nonoverlapping(addr.as_ptr(), xdrs.x_private as *mut u8, len);
    }
    xdrs.x_private = xdrs.x_private.wrapping_add(len);
    TRUE
}

/// Memory-based getpos: return offset from base.
fn mem_getpos(xdrs: &Xdr) -> c_uint {
    (xdrs.x_private as usize - xdrs.x_base as usize) as c_uint
}

/// Memory-based setpos: reposition within buffer.
fn mem_setpos(xdrs: &mut Xdr, pos: c_uint) -> c_int {
    let pos = pos as usize;
    let newaddr = xdrs.x_base.wrapping_add(pos);
    let lastaddr = xdrs.x_private.wrapping_add(xdrs.x_handy as usize);
    if newaddr as usize > lastaddr as usize {
        return FALSE;
    }
    xdrs.x_private = newaddr;
    xdrs.x_handy = (lastaddr as usize - newaddr as usize) as c_int;
    TRUE
}

// ---------------------------------------------------------------------------
// XdrStdio backend implementation (from xdr_stdio.c)
// ---------------------------------------------------------------------------

/// Stdio-based getlong: read 4 bytes in big-endian order.
fn stdio_getlong(xdrs: &mut Xdr, lp: &mut i32) -> c_int {
    let file = unsafe { &mut *(xdrs.x_private as *mut File) };
    let mut buf = [0u8; 4];
    match file.read_exact(&mut buf) {
        Ok(()) => {
            let val = u32::from_be_bytes(buf);
            *lp = val as i32;
            TRUE
        }
        Err(_) => FALSE,
    }
}

/// Stdio-based putlong: write 4 bytes in big-endian order.
fn stdio_putlong(xdrs: &mut Xdr, lp: &i32) -> c_int {
    let file = unsafe { &mut *(xdrs.x_private as *mut File) };
    let swapped = htonl(*lp as u32);
    match file.write_all(&swapped.to_be_bytes()) {
        Ok(()) => TRUE,
        Err(_) => FALSE,
    }
}

/// Stdio-based getbytes: read raw bytes from file.
fn stdio_getbytes(xdrs: &mut Xdr, addr: &mut [u8]) -> c_int {
    if addr.is_empty() {
        return TRUE;
    }
    let file = unsafe { &mut *(xdrs.x_private as *mut File) };
    match file.read_exact(addr) {
        Ok(()) => TRUE,
        Err(_) => FALSE,
    }
}

/// Stdio-based putbytes: write raw bytes to file.
fn stdio_putbytes(xdrs: &mut Xdr, addr: &[u8]) -> c_int {
    if addr.is_empty() {
        return TRUE;
    }
    let file = unsafe { &mut *(xdrs.x_private as *mut File) };
    match file.write_all(addr) {
        Ok(()) => TRUE,
        Err(_) => FALSE,
    }
}

/// Stdio-based getpos: return current file position.
fn stdio_getpos(xdrs: &Xdr) -> c_uint {
    // stream_position() requires &mut self on the File, but we access
    // through a raw pointer so we can obtain &mut even from &Xdr.
    let file = xdrs.x_private as *mut File;
    unsafe {
        match (*file).stream_position() {
            Ok(pos) => pos as c_uint,
            Err(_) => 0,
        }
    }
}

/// Stdio-based setpos: seek to given position.
fn stdio_setpos(xdrs: &mut Xdr, pos: c_uint) -> c_int {
    let file = unsafe { &mut *(xdrs.x_private as *mut File) };
    match file.seek(SeekFrom::Start(pos as u64)) {
        Ok(_) => TRUE,
        Err(_) => FALSE,
    }
}

// ---------------------------------------------------------------------------
// C-compatible XDR struct for FFI
// ---------------------------------------------------------------------------

/// A C-compatible wrapper around a heap-allocated `Xdr`.
///
/// This is what C code sees when it receives an `XDR*`. The Rust `Xdr`
/// struct lives on the heap inside an owning box that `xdr_destroy`
/// releases.
#[repr(C)]
pub struct XdrC {
    /// Opaque heap-owned `Xdr`; `None` after `xdr_destroy`.
    handle: Option<Box<Xdr>>,
}

// We implement Send + Sync since XdrC can be passed across FFI.
// The inner Xdr uses raw pointers, so we declare these unsafely.
unsafe impl Send for XdrC {}
unsafe impl Sync for XdrC {}

impl XdrC {
    /// Get a mutable reference to the inner `Xdr`.
    ///
    /// Panics if the stream was already destroyed.
    fn inner(&mut self) -> &mut Xdr {
        self.handle
            .as_deref_mut()
            .expect("XDR stream already destroyed")
    }

    /// Get a reference to the inner `Xdr`.
    ///
    /// Panics if the stream was already destroyed.
    fn inner_ref(&self) -> &Xdr {
        self.handle
            .as_deref()
            .expect("XDR stream already destroyed")
    }
}

// ---------------------------------------------------------------------------
// Constructor: xdrmem_create
// ---------------------------------------------------------------------------

/// Create an XDR stream backed by a memory buffer.
///
/// Ported from C's `xdrmem_create()`.
///
/// The buffer must remain valid for the lifetime of the returned `XdrC`;
/// the stream only retains a raw pointer into it. Pass the `XdrC` to
/// `xdr_destroy` when done.
pub fn xdrmem_create(addr: &mut [u8], op: c_int) -> XdrC {
    let x_op = match op {
        0 => XdrOp::Encode,
        1 => XdrOp::Decode,
        2 => XdrOp::Free,
        _ => XdrOp::Free,
    };

    let addr_ptr = addr.as_mut_ptr();
    XdrC {
        handle: Some(Box::new(Xdr {
            x_op,
            backend: XdrBackend::Mem,
            x_base: addr_ptr,
            x_private: addr_ptr,
            x_handy: addr.len() as c_int,
        })),
    }
}

// ---------------------------------------------------------------------------
// Constructor: xdrstdio_create
// ---------------------------------------------------------------------------

/// Create an XDR stream backed by a `std::fs::File`.
///
/// Ported from C's `xdrstdio_create()`.
///
/// The `File` must outlive the returned `XdrC`; the stream only retains a
/// raw pointer to it. Pass the `XdrC` to `xdr_destroy` when done.
pub fn xdrstdio_create(file: &mut File, op: c_int) -> XdrC {
    let x_op = match op {
        0 => XdrOp::Encode,
        1 => XdrOp::Decode,
        2 => XdrOp::Free,
        _ => XdrOp::Free,
    };

    XdrC {
        handle: Some(Box::new(Xdr {
            x_op,
            backend: XdrBackend::Stdio,
            x_base: ptr::null_mut(),
            x_private: file as *mut File as *mut u8,
            x_handy: 0,
        })),
    }
}

// ---------------------------------------------------------------------------
// Destructor: xdr_destroy
// ---------------------------------------------------------------------------

/// Destroy an XDR stream and free its resources.
///
/// After this call, `xdrs` must not be used.
pub fn xdr_destroy(xdrs: &mut XdrC) {
    if let Some(mut xdr) = xdrs.handle.take() {
        xdr.destroy();
        // Box is dropped here, freeing the Xdr
    }
}

// ---------------------------------------------------------------------------
// Generic XDR routines (ported from xdr.c)
// ---------------------------------------------------------------------------

/// XDR a 32-bit integer.
///
/// Ported from C's `xdr_int()`.
pub fn xdr_int(xdrs: &mut XdrC, ip: &mut i32) -> c_int {
    let xdr = xdrs.inner();
    match xdr.x_op {
        XdrOp::Decode => xdr.getlong(ip),
        XdrOp::Encode => xdr.putlong(ip),
        XdrOp::Free => TRUE,
    }
}

/// XDR an unsigned 32-bit integer.
///
/// Ported from C's `xdr_u_int()`.
pub fn xdr_u_int(xdrs: &mut XdrC, up: &mut u32) -> c_int {
    let xdr = xdrs.inner();
    match xdr.x_op {
        XdrOp::Decode => {
            let mut v = 0i32;
            let result = xdr.getlong(&mut v);
            if result == TRUE {
                *up = v as u32;
            }
            result
        }
        XdrOp::Encode => xdr.putlong(&(*up as i32)),
        XdrOp::Free => TRUE,
    }
}

/// XDR opaque data of a fixed size.
///
/// Ported from C's `xdr_opaque()`.
/// `cp` is the opaque byte buffer; its length is rounded up to a multiple
/// of 4 (BYTES_PER_XDR_UNIT).
pub fn xdr_opaque(xdrs: &mut XdrC, cp: &mut [u8]) -> c_int {
    let cnt = cp.len() as c_uint;
    if cnt == 0 {
        return TRUE;
    }

    // Round byte count up to full XDR units
    #[allow(clippy::manual_is_multiple_of)]
    let rndup = if cnt % BYTES_PER_XDR_UNIT as u32 != 0 {
        BYTES_PER_XDR_UNIT as usize - (cnt as usize % BYTES_PER_XDR_UNIT)
    } else {
        0
    };

    let xdr = xdrs.inner();
    match xdr.x_op {
        XdrOp::Decode => {
            if xdr.getbytes(cp) == FALSE {
                return FALSE;
            }
            if rndup == 0 {
                return TRUE;
            }
            // Read and discard padding bytes
            let mut crud = [0u8; BYTES_PER_XDR_UNIT];
            xdr.getbytes(&mut crud[..rndup])
        }
        XdrOp::Encode => {
            if xdr.putbytes(cp) == FALSE {
                return FALSE;
            }
            if rndup == 0 {
                return TRUE;
            }
            xdr.putbytes(&XDR_ZERO[..rndup])
        }
        XdrOp::Free => TRUE,
    }
}

/// XDR counted bytes.
///
/// Ported from C's `xdr_bytes()`.
/// `cpp` is a pointer to the byte pointer, `sizep` is a pointer to the count.
/// If `*cpp` is NULL during decode, memory is allocated.
///
/// # Safety
/// - `xdrs` must be a valid pointer to an `XdrC`.
/// - `cpp` must be a valid pointer to a `*mut u8`; when `*cpp` is non-null it
///   must point to at least `*sizep` bytes.
/// - `sizep` must be a valid pointer to a `u32`.
pub unsafe fn xdr_bytes(
    xdrs: *mut XdrC,
    cpp: *mut *mut u8,
    sizep: *mut c_uint,
    maxsize: c_uint,
) -> c_int {
    unsafe {
        if xdrs.is_null() || cpp.is_null() || sizep.is_null() {
            return FALSE;
        }
        let xdr = (*xdrs).inner();

        // First deal with the length
        if xdr_u_int(&mut *xdrs, &mut *sizep) == FALSE {
            return FALSE;
        }
        let nodesize = *sizep;
        if nodesize > maxsize && xdr.x_op != XdrOp::Free {
            return FALSE;
        }

        let mut sp = *cpp;

        match xdr.x_op {
            XdrOp::Decode => {
                if nodesize == 0 {
                    return TRUE;
                }
                if sp.is_null() {
                    // Allocate memory (mirrors C's mem_alloc / malloc)
                    let layout = std::alloc::Layout::from_size_align(nodesize as usize, 1)
                        .unwrap_or_else(|_| std::alloc::Layout::new::<u8>());
                    sp = std::alloc::alloc(layout) as *mut u8;
                    if sp.is_null() {
                        return FALSE;
                    }
                    *cpp = sp;
                }
                if sp.is_null() {
                    return FALSE;
                }
                // Fall through to opaque handling
                let buf = std::slice::from_raw_parts_mut(sp, nodesize as usize);
                xdr_opaque(&mut *xdrs, buf)
            }
            XdrOp::Encode => {
                if nodesize == 0 {
                    return TRUE;
                }
                let buf = std::slice::from_raw_parts_mut(sp, nodesize as usize);
                xdr_opaque(&mut *xdrs, buf)
            }
            XdrOp::Free => {
                if !sp.is_null() {
                    // Free the allocated memory
                    let layout = std::alloc::Layout::from_size_align(nodesize as usize, 1)
                        .unwrap_or_else(|_| std::alloc::Layout::new::<u8>());
                    std::alloc::dealloc(sp, layout);
                    *cpp = ptr::null_mut();
                }
                TRUE
            }
        }
    }
}

/// XDR a null-terminated ASCII string.
///
/// Ported from C's `xdr_string()`.
/// `cpp` references a pointer to the string. If the pointer is null during
/// decode, storage is allocated. `maxsize` is the maximum allowed length.
///
/// # Safety
/// - `xdrs` must be a valid pointer to an `XdrC`.
/// - `cpp` must be a valid pointer to a `*mut u8`; when `*cpp` is non-null it
///   must point to a NUL-terminated string buffer of at least `size + 1`
///   bytes.
pub unsafe fn xdr_string(xdrs: *mut XdrC, cpp: *mut *mut u8, maxsize: c_uint) -> c_int {
    unsafe {
        if xdrs.is_null() || cpp.is_null() {
            return FALSE;
        }
        let xdr = (*xdrs).inner();

        let mut sp = *cpp;
        let mut size: c_uint = 0;

        // Compute string length for encode/free
        match xdr.x_op {
            XdrOp::Free => {
                if sp.is_null() {
                    return TRUE; // already free
                }
                // fall through to compute size for free
            }
            XdrOp::Encode => {
                if !sp.is_null() {
                    let cstr = CStr::from_ptr(sp as *const core::ffi::c_char);
                    size = cstr.to_bytes().len() as c_uint;
                }
            }
            XdrOp::Decode => {
                // size will be read from the stream below
            }
        }

        // Read/write the length
        if xdr_u_int(&mut *xdrs, &mut size) == FALSE {
            return FALSE;
        }
        if size > maxsize {
            return FALSE;
        }
        let nodesize = size + 1; // include null terminator

        match xdr.x_op {
            XdrOp::Decode => {
                if nodesize == 0 {
                    return TRUE;
                }
                if sp.is_null() {
                    let layout = std::alloc::Layout::from_size_align(nodesize as usize, 1)
                        .unwrap_or_else(|_| std::alloc::Layout::new::<u8>());
                    sp = std::alloc::alloc(layout) as *mut u8;
                    if sp.is_null() {
                        return FALSE;
                    }
                    *cpp = sp;
                }
                if sp.is_null() {
                    return FALSE;
                }
                // Null-terminate the buffer
                *sp.add(size as usize) = 0;
                // Fall through to opaque
                let buf = std::slice::from_raw_parts_mut(sp, size as usize);
                xdr_opaque(&mut *xdrs, buf)
            }
            XdrOp::Encode => {
                if size == 0 {
                    return TRUE;
                }
                let buf = std::slice::from_raw_parts_mut(sp, size as usize);
                xdr_opaque(&mut *xdrs, buf)
            }
            XdrOp::Free => {
                if !sp.is_null() {
                    let layout = std::alloc::Layout::from_size_align(nodesize as usize, 1)
                        .unwrap_or_else(|_| std::alloc::Layout::new::<u8>());
                    std::alloc::dealloc(sp, layout);
                    *cpp = ptr::null_mut();
                }
                TRUE
            }
        }
    }
}

/// XDR a double-precision floating point number.
///
/// Ported from C's `xdr_double()` (from xdr_float.c).
///
/// The two 32-bit halves of the double are transmitted high word first
/// (matching the original C behavior on both endiannesses: little-endian
/// wrote `lp+1` then `lp`, big-endian `lp` then `lp+1`).
pub fn xdr_double(xdrs: &mut XdrC, dp: &mut f64) -> c_int {
    let xdr = xdrs.inner();

    match xdr.x_op {
        XdrOp::Encode => {
            // High 32 bits first, then low 32 bits.
            let bits = dp.to_bits();
            let second = xdr.putlong(&((bits >> 32) as u32 as i32));
            let first = xdr.putlong(&(bits as u32 as i32));
            if second == TRUE && first == TRUE {
                TRUE
            } else {
                FALSE
            }
        }
        XdrOp::Decode => {
            let mut hi = 0i32;
            let mut lo = 0i32;
            let second = xdr.getlong(&mut hi);
            let first = xdr.getlong(&mut lo);
            if second == TRUE && first == TRUE {
                *dp = f64::from_bits(((hi as u32 as u64) << 32) | (lo as u32 as u64));
                TRUE
            } else {
                FALSE
            }
        }
        XdrOp::Free => TRUE,
    }
}

// ---------------------------------------------------------------------------
// Additional C API helpers (getpos / setpos)
// ---------------------------------------------------------------------------

/// Get the current position in the XDR stream.
pub fn xdr_getpos(xdrs: &XdrC) -> c_uint {
    xdrs.inner_ref().getpos()
}

/// Set the current position in the XDR stream.
pub fn xdr_setpos(xdrs: &mut XdrC, pos: c_uint) -> c_int {
    xdrs.inner().setpos(pos)
}
