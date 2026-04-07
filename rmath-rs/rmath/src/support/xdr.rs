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

use std::ffi::{CStr, c_int, c_uint, c_void};
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

    fn getbytes(&mut self, addr: *mut u8, len: c_uint) -> c_int {
        match self.backend {
            XdrBackend::Mem => mem_getbytes(self, addr, len),
            XdrBackend::Stdio => stdio_getbytes(self, addr, len),
        }
    }

    fn putbytes(&mut self, addr: *const u8, len: c_uint) -> c_int {
        match self.backend {
            XdrBackend::Mem => mem_putbytes(self, addr, len),
            XdrBackend::Stdio => stdio_putbytes(self, addr, len),
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

    fn inline_op(&mut self, len: c_uint) -> *mut c_void {
        match self.backend {
            XdrBackend::Mem => mem_inline(self, len),
            XdrBackend::Stdio => ptr::null_mut(),
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
fn mem_getbytes(xdrs: &mut Xdr, addr: *mut u8, len: c_uint) -> c_int {
    let len = len as usize;
    if len == 0 {
        return TRUE;
    }
    if (xdrs.x_handy as usize) < len {
        return FALSE;
    }
    xdrs.x_handy -= len as c_int;
    unsafe {
        ptr::copy_nonoverlapping(xdrs.x_private, addr, len);
    }
    xdrs.x_private = xdrs.x_private.wrapping_add(len);
    TRUE
}

/// Memory-based putbytes: copy bytes into buffer.
fn mem_putbytes(xdrs: &mut Xdr, addr: *const u8, len: c_uint) -> c_int {
    let len = len as usize;
    if len == 0 {
        return TRUE;
    }
    if (xdrs.x_handy as usize) < len {
        return FALSE;
    }
    xdrs.x_handy -= len as c_int;
    unsafe {
        ptr::copy_nonoverlapping(addr, xdrs.x_private as *mut u8, len);
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

/// Memory-based inline: return direct pointer to buffered data.
fn mem_inline(xdrs: &mut Xdr, len: c_uint) -> *mut c_void {
    let len = len as usize;
    if (xdrs.x_handy as usize) >= len {
        xdrs.x_handy -= len as c_int;
        let buf = xdrs.x_private as *mut c_void;
        xdrs.x_private = xdrs.x_private.wrapping_add(len);
        buf
    } else {
        ptr::null_mut()
    }
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
fn stdio_getbytes(xdrs: &mut Xdr, addr: *mut u8, len: c_uint) -> c_int {
    if len == 0 {
        return TRUE;
    }
    let file = unsafe { &mut *(xdrs.x_private as *mut File) };
    let slice = unsafe { std::slice::from_raw_parts_mut(addr, len as usize) };
    match file.read_exact(slice) {
        Ok(()) => TRUE,
        Err(_) => FALSE,
    }
}

/// Stdio-based putbytes: write raw bytes to file.
fn stdio_putbytes(xdrs: &mut Xdr, addr: *const u8, len: c_uint) -> c_int {
    if len == 0 {
        return TRUE;
    }
    let file = unsafe { &mut *(xdrs.x_private as *mut File) };
    let slice = unsafe { std::slice::from_raw_parts(addr, len as usize) };
    match file.write_all(slice) {
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
/// struct lives on the heap, and `XdrC` wraps a pointer to it.
///
/// # Safety
/// This struct must only be created by the `*_create` functions and freed
/// by calling `xdr_destroy` to avoid memory leaks.
#[repr(C)]
pub struct XdrC {
    /// Opaque pointer to the Rust-native Xdr.
    handle: *mut Xdr,
}

// We implement Send + Sync since XdrC can be passed across FFI.
// The inner Xdr uses raw pointers, so we declare these unsafely.
unsafe impl Send for XdrC {}
unsafe impl Sync for XdrC {}

impl XdrC {
    /// Get a mutable reference to the inner `Xdr`.
    ///
    /// # Safety
    /// `self.handle` must be a valid, non-null pointer.
    unsafe fn inner(&mut self) -> &mut Xdr {
        unsafe { &mut *self.handle }
    }

    /// Get a reference to the inner `Xdr`.
    ///
    /// # Safety
    /// `self.handle` must be a valid, non-null pointer.
    unsafe fn inner_ref(&self) -> &Xdr {
        unsafe { &*self.handle }
    }
}

// ---------------------------------------------------------------------------
// Constructor: xdrmem_create
// ---------------------------------------------------------------------------

/// Create an XDR stream backed by a memory buffer.
///
/// Ported from C's `xdrmem_create()`.
///
/// # Safety
/// - `addr` must point to a valid memory region of at least `size` bytes.
/// - The memory pointed to by `addr` must remain valid for the lifetime of
///   the returned `XdrC`.
/// - The returned `XdrC` must eventually be freed by calling
///   `xdr_destroy` to avoid memory leaks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xdrmem_create(
    xdrs: *mut XdrC,
    addr: *mut c_void,
    size: c_uint,
    op: c_int,
) {
    unsafe {
        let addr = addr as *mut u8;
        let size = size as c_int;
        let x_op = match op {
            0 => XdrOp::Encode,
            1 => XdrOp::Decode,
            2 => XdrOp::Free,
            _ => XdrOp::Free,
        };

        let xdr = Box::new(Xdr {
            x_op,
            backend: XdrBackend::Mem,
            x_base: addr,
            x_private: addr,
            x_handy: size,
        });

        ptr::write(
            xdrs,
            XdrC {
                handle: Box::into_raw(xdr),
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Constructor: xdrstdio_create
// ---------------------------------------------------------------------------

/// Create an XDR stream backed by a `std::fs::File`.
///
/// Ported from C's `xdrstdio_create()`.
///
/// # Safety
/// - `file` must be a valid pointer to a heap-allocated `std::fs::File`.
/// - The caller is responsible for ensuring the `File` outlives the XDR stream,
///   or for closing the file separately after destroying the XDR stream.
/// - The returned `XdrC` must eventually be freed by calling `xdr_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xdrstdio_create(xdrs: *mut XdrC, file: *mut File, op: c_int) {
    unsafe {
        let x_op = match op {
            0 => XdrOp::Encode,
            1 => XdrOp::Decode,
            2 => XdrOp::Free,
            _ => XdrOp::Free,
        };

        let xdr = Box::new(Xdr {
            x_op,
            backend: XdrBackend::Stdio,
            x_base: ptr::null_mut(),
            x_private: file as *mut u8,
            x_handy: 0,
        });

        ptr::write(
            xdrs,
            XdrC {
                handle: Box::into_raw(xdr),
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Destructor: xdr_destroy
// ---------------------------------------------------------------------------

/// Destroy an XDR stream and free its resources.
///
/// # Safety
/// - `xdrs` must be a valid pointer to an `XdrC` that was created by
///   `xdrmem_create` or `xdrstdio_create`.
/// - After this call, `xdrs` must not be used.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xdr_destroy(xdrs: *mut XdrC) {
    unsafe {
        if xdrs.is_null() {
            return;
        }
        let xdr_c = &mut *xdrs;
        if !xdr_c.handle.is_null() {
            let mut xdr = Box::from_raw(xdr_c.handle);
            xdr.destroy();
            // Box is dropped here, freeing the Xdr
            xdr_c.handle = ptr::null_mut();
        }
    }
}

// ---------------------------------------------------------------------------
// Generic XDR routines (ported from xdr.c)
// ---------------------------------------------------------------------------

/// XDR a 32-bit integer.
///
/// Ported from C's `xdr_int()`.
///
/// # Safety
/// - `xdrs` must be a valid pointer to an `XdrC`.
/// - `ip` must be a valid pointer to an `i32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xdr_int(xdrs: *mut XdrC, ip: *mut i32) -> c_int {
    unsafe {
        if xdrs.is_null() || ip.is_null() {
            return FALSE;
        }
        let xdr = (*xdrs).inner();
        match xdr.x_op {
            XdrOp::Decode => xdr.getlong(&mut *ip),
            XdrOp::Encode => xdr.putlong(&*ip),
            XdrOp::Free => TRUE,
        }
    }
}

/// XDR an unsigned 32-bit integer.
///
/// Ported from C's `xdr_u_int()`.
///
/// # Safety
/// - `xdrs` must be a valid pointer to an `XdrC`.
/// - `up` must be a valid pointer to a `u32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xdr_u_int(xdrs: *mut XdrC, up: *mut u32) -> c_int {
    unsafe {
        if xdrs.is_null() || up.is_null() {
            return FALSE;
        }
        let xdr = (*xdrs).inner();
        match xdr.x_op {
            XdrOp::Decode => xdr.getlong(&mut *(up as *mut i32)),
            XdrOp::Encode => xdr.putlong(&*(up as *const i32)),
            XdrOp::Free => TRUE,
        }
    }
}

/// XDR opaque data of a fixed size.
///
/// Ported from C's `xdr_opaque()`.
/// `cp` points to the opaque object and `cnt` gives the byte length.
/// The count is rounded up to a multiple of 4 (BYTES_PER_XDR_UNIT).
///
/// # Safety
/// - `xdrs` must be a valid pointer to an `XdrC`.
/// - If `cnt > 0`, `cp` must be a valid pointer to at least `cnt` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xdr_opaque(xdrs: *mut XdrC, cp: *mut c_void, cnt: c_uint) -> c_int {
    unsafe {
        if xdrs.is_null() {
            return FALSE;
        }
        let xdr = (*xdrs).inner();

        if cnt == 0 {
            return TRUE;
        }

        // Round byte count up to full XDR units
        let rndup = if cnt % BYTES_PER_XDR_UNIT as u32 != 0 {
            BYTES_PER_XDR_UNIT as u32 - (cnt % BYTES_PER_XDR_UNIT as u32)
        } else {
            0
        };

        match xdr.x_op {
            XdrOp::Decode => {
                if xdr.getbytes(cp as *mut u8, cnt) == FALSE {
                    return FALSE;
                }
                if rndup == 0 {
                    return TRUE;
                }
                // Read and discard padding bytes
                let mut crud = [0u8; BYTES_PER_XDR_UNIT];
                xdr.getbytes(crud.as_mut_ptr(), rndup)
            }
            XdrOp::Encode => {
                if xdr.putbytes(cp as *const u8, cnt) == FALSE {
                    return FALSE;
                }
                if rndup == 0 {
                    return TRUE;
                }
                xdr.putbytes(XDR_ZERO.as_ptr(), rndup)
            }
            XdrOp::Free => TRUE,
        }
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
/// - `cpp` must be a valid pointer to a `*mut c_char`.
/// - `sizep` must be a valid pointer to a `u32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xdr_bytes(
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
        if xdr_u_int(xdrs, sizep) == FALSE {
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
                    let layout = std::alloc::Layout::from_size_align(nodesize as usize, 1).expect("unwrap on None/Err");
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
                xdr_opaque(xdrs, sp as *mut c_void, nodesize)
            }
            XdrOp::Encode => xdr_opaque(xdrs, sp as *mut c_void, nodesize),
            XdrOp::Free => {
                if !sp.is_null() {
                    // Free the allocated memory
                    let layout = std::alloc::Layout::from_size_align(nodesize as usize, 1).expect("unwrap on None/Err");
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
/// - `cpp` must be a valid pointer to a `*mut c_char`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xdr_string(xdrs: *mut XdrC, cpp: *mut *mut u8, maxsize: c_uint) -> c_int {
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
                    let cstr = CStr::from_ptr(sp as *const i8);
                    size = cstr.to_bytes().len() as c_uint;
                }
            }
            XdrOp::Decode => {
                // size will be read from the stream below
            }
        }

        // Read/write the length
        if xdr_u_int(xdrs, &mut size) == FALSE {
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
                    let layout = std::alloc::Layout::from_size_align(nodesize as usize, 1).expect("unwrap on None/Err");
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
                xdr_opaque(xdrs, sp as *mut c_void, size)
            }
            XdrOp::Encode => xdr_opaque(xdrs, sp as *mut c_void, size),
            XdrOp::Free => {
                if !sp.is_null() {
                    let layout = std::alloc::Layout::from_size_align(nodesize as usize, 1).expect("unwrap on None/Err");
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
/// On little-endian systems, the two 32-bit halves of the double are
/// transmitted in reversed order (matching the original C behavior with
/// WORDS_BIGENDIAN / ntohl).
///
/// # Safety
/// - `xdrs` must be a valid pointer to an `XdrC`.
/// - `dp` must be a valid pointer to a `f64`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xdr_double(xdrs: *mut XdrC, dp: *mut f64) -> c_int {
    unsafe {
        if xdrs.is_null() || dp.is_null() {
            return FALSE;
        }
        let xdr = (*xdrs).inner();

        // Treat the double as two consecutive i32 values
        let lp = dp as *mut i32;

        match xdr.x_op {
            XdrOp::Encode => {
                #[cfg(target_endian = "little")]
                {
                    // On little-endian: put lp+1 first, then lp (swap halves)
                    // Mirrors: return (XDR_PUTLONG(xdrs, lp+1) && XDR_PUTLONG(xdrs, lp));
                    let second = xdr.putlong(&*lp.add(1));
                    let first = xdr.putlong(&*lp);
                    if second == TRUE && first == TRUE {
                        TRUE
                    } else {
                        FALSE
                    }
                }
                #[cfg(target_endian = "big")]
                {
                    // On big-endian: put lp first, then lp+1 (natural order)
                    // Mirrors: return (XDR_PUTLONG(xdrs, lp++) && XDR_PUTLONG(xdrs, lp));
                    let first = xdr.putlong(&*lp);
                    let second = xdr.putlong(&*lp.add(1));
                    if first == TRUE && second == TRUE {
                        TRUE
                    } else {
                        FALSE
                    }
                }
            }
            XdrOp::Decode => {
                #[cfg(target_endian = "little")]
                {
                    // On little-endian: get into lp+1 first, then lp (swap halves)
                    // Mirrors: return (XDR_GETLONG(xdrs, lp+1) && XDR_GETLONG(xdrs, lp));
                    let second = xdr.getlong(&mut *lp.add(1));
                    let first = xdr.getlong(&mut *lp);
                    if second == TRUE && first == TRUE {
                        TRUE
                    } else {
                        FALSE
                    }
                }
                #[cfg(target_endian = "big")]
                {
                    // On big-endian: get into lp first, then lp+1 (natural order)
                    // Mirrors: return (XDR_GETLONG(xdrs, lp++) && XDR_GETLONG(xdrs, lp));
                    let first = xdr.getlong(&mut *lp);
                    let second = xdr.getlong(&mut *lp.add(1));
                    if first == TRUE && second == TRUE {
                        TRUE
                    } else {
                        FALSE
                    }
                }
            }
            XdrOp::Free => TRUE,
        }
    }
}

// ---------------------------------------------------------------------------
// Additional C API helpers (getpos / setpos)
// ---------------------------------------------------------------------------

/// Get the current position in the XDR stream.
///
/// # Safety
/// - `xdrs` must be a valid pointer to an `XdrC`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xdr_getpos(xdrs: *mut XdrC) -> c_uint {
    unsafe {
        if xdrs.is_null() {
            return 0;
        }
        (*xdrs).inner_ref().getpos()
    }
}

/// Set the current position in the XDR stream.
///
/// # Safety
/// - `xdrs` must be a valid pointer to an `XdrC`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xdr_setpos(xdrs: *mut XdrC, pos: c_uint) -> c_int {
    unsafe {
        if xdrs.is_null() {
            return FALSE;
        }
        (*xdrs).inner().setpos(pos)
    }
}

// ---------------------------------------------------------------------------
// Rust-native API for testing / internal use
// ---------------------------------------------------------------------------

/// Create a memory-backed XDR stream (Rust-native API).
///
/// Returns a boxed `Xdr` that can be used directly from Rust code.
pub(crate) fn xdrmem_create_rust(addr: &mut [u8], op: XdrOp) -> Box<Xdr> {
    let addr_ptr = addr.as_mut_ptr();
    let size = addr.len() as c_int;
    Box::new(Xdr {
        x_op: op,
        backend: XdrBackend::Mem,
        x_base: addr_ptr,
        x_private: addr_ptr,
        x_handy: size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ntohl_swap() {
        // On little-endian, ntohl should swap bytes
        let val: u32 = 0x12345678;
        #[cfg(target_endian = "little")]
        assert_eq!(ntohl(val), 0x78563412);
        #[cfg(target_endian = "big")]
        assert_eq!(ntohl(val), 0x12345678);
    }

    #[test]
    fn test_xdr_int_encode_decode_roundtrip() {
        let mut buf = [0u8; 32];
        let mut xdr = xdrmem_create_rust(&mut buf, XdrOp::Encode);

        // Encode
        let val: i32 = 0x12345678;
        assert_eq!(xdr.putlong(&val), TRUE);
        assert_eq!(xdr.getpos(), 4);

        drop(xdr);

        // Decode
        let mut xdr2 = xdrmem_create_rust(&mut buf, XdrOp::Decode);
        let mut result: i32 = 0;
        assert_eq!(xdr2.getlong(&mut result), TRUE);
        assert_eq!(result, 0x12345678);
    }

    #[test]
    fn test_xdr_u_int_roundtrip() {
        let mut buf = [0u8; 32];
        let mut xdr = xdrmem_create_rust(&mut buf, XdrOp::Encode);

        let val: u32 = 0xDEADBEEF;
        assert_eq!(xdr.putlong(&(val as i32)), TRUE);

        drop(xdr);

        let mut xdr2 = xdrmem_create_rust(&mut buf, XdrOp::Decode);
        let mut result: i32 = 0;
        assert_eq!(xdr2.getlong(&mut result), TRUE);
        assert_eq!(result as u32, 0xDEADBEEF);
    }

    #[test]
    fn test_xdr_double_roundtrip() {
        let mut buf = [0u8; 32];
        let mut xdr = xdrmem_create_rust(&mut buf, XdrOp::Encode);

        let mut val: f64 = 3.141592653589793;
        unsafe {
            let mut xdr_c = XdrC { handle: &mut *xdr };
            assert_eq!(xdr_double(&mut xdr_c, &mut val), TRUE);
        }
        assert_eq!(xdr.getpos(), 8);

        drop(xdr);

        // Decode
        let mut xdr2 = xdrmem_create_rust(&mut buf, XdrOp::Decode);
        let mut result: f64 = 0.0;
        unsafe {
            let mut xdr_c2 = XdrC { handle: &mut *xdr2 };
            assert_eq!(xdr_double(&mut xdr_c2, &mut result), TRUE);
        }
        assert!((result - 3.141592653589793).abs() < f64::EPSILON);
    }

    #[test]
    fn test_xdr_double_special_values() {
        // Test zero
        let mut buf = [0u8; 32];
        let mut xdr = xdrmem_create_rust(&mut buf, XdrOp::Encode);
        let mut val: f64 = 0.0;
        unsafe {
            let mut xdr_c = XdrC { handle: &mut *xdr };
            assert_eq!(xdr_double(&mut xdr_c, &mut val), TRUE);
        }
        drop(xdr);

        let mut xdr2 = xdrmem_create_rust(&mut buf, XdrOp::Decode);
        let mut result: f64 = 1.0;
        unsafe {
            let mut xdr_c2 = XdrC { handle: &mut *xdr2 };
            assert_eq!(xdr_double(&mut xdr_c2, &mut result), TRUE);
        }
        assert_eq!(result, 0.0);

        // Test negative
        let mut buf2 = [0u8; 32];
        let mut xdr3 = xdrmem_create_rust(&mut buf2, XdrOp::Encode);
        let mut val2: f64 = -42.5;
        unsafe {
            let mut xdr_c3 = XdrC { handle: &mut *xdr3 };
            assert_eq!(xdr_double(&mut xdr_c3, &mut val2), TRUE);
        }
        drop(xdr3);

        let mut xdr4 = xdrmem_create_rust(&mut buf2, XdrOp::Decode);
        let mut result2: f64 = 0.0;
        unsafe {
            let mut xdr_c4 = XdrC { handle: &mut *xdr4 };
            assert_eq!(xdr_double(&mut xdr_c4, &mut result2), TRUE);
        }
        assert!((result2 - (-42.5)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_xdr_opaque_roundtrip() {
        let mut buf = [0u8; 32];
        let mut xdr = xdrmem_create_rust(&mut buf, XdrOp::Encode);

        let data: &mut [u8] = &mut [0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE];
        unsafe {
            let mut xdr_c = XdrC { handle: &mut *xdr };
            assert_eq!(
                xdr_opaque(&mut xdr_c, data.as_mut_ptr() as *mut c_void, 6),
                TRUE
            );
        }
        // 6 bytes + 2 padding = 8 bytes
        assert_eq!(xdr.getpos(), 8);

        drop(xdr);

        // Decode
        let mut xdr2 = xdrmem_create_rust(&mut buf, XdrOp::Decode);
        let mut result = [0u8; 6];
        unsafe {
            let mut xdr_c2 = XdrC { handle: &mut *xdr2 };
            assert_eq!(
                xdr_opaque(&mut xdr_c2, result.as_mut_ptr() as *mut c_void, 6),
                TRUE
            );
        }
        assert_eq!(result, [0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE]);
    }

    #[test]
    fn test_xdr_opaque_exact_multiple() {
        // 4 bytes -- exact multiple of BYTES_PER_XDR_UNIT, no padding
        let mut buf = [0u8; 32];
        let mut xdr = xdrmem_create_rust(&mut buf, XdrOp::Encode);
        let data: &mut [u8] = &mut [1, 2, 3, 4];
        unsafe {
            let mut xdr_c = XdrC { handle: &mut *xdr };
            assert_eq!(
                xdr_opaque(&mut xdr_c, data.as_mut_ptr() as *mut c_void, 4),
                TRUE
            );
        }
        assert_eq!(xdr.getpos(), 4); // no padding
    }

    #[test]
    fn test_xdr_setpos() {
        let mut buf = [0u8; 32];
        let mut xdr = xdrmem_create_rust(&mut buf, XdrOp::Encode);

        let val: i32 = 42;
        xdr.putlong(&val);
        assert_eq!(xdr.getpos(), 4);

        // Rewind and write again
        assert_eq!(xdr.setpos(0), TRUE);
        let val2: i32 = 99;
        xdr.putlong(&val2);
        assert_eq!(xdr.getpos(), 4);

        drop(xdr);

        // Verify the second value was written
        let mut xdr2 = xdrmem_create_rust(&mut buf, XdrOp::Decode);
        let mut result: i32 = 0;
        xdr2.getlong(&mut result);
        assert_eq!(result, 99);
    }

    #[test]
    fn test_xdr_int_overflow_buffer() {
        let mut buf = [0u8; 2]; // Too small for a 4-byte int
        let mut xdr = xdrmem_create_rust(&mut buf, XdrOp::Encode);

        let val: i32 = 42;
        assert_eq!(xdr.putlong(&val), FALSE);
    }

    #[test]
    fn test_xdr_int_free() {
        let mut buf = [0u8; 32];
        let mut xdr = xdrmem_create_rust(&mut buf, XdrOp::Free);

        let mut val: i32 = 42;
        // Free should always return TRUE
        assert_eq!(xdr.getlong(&mut val), TRUE);
        assert_eq!(xdr.putlong(&val), TRUE);
    }

    #[test]
    fn test_xdr_getbytes_putbytes() {
        let mut buf = [0u8; 32];
        let mut xdr = xdrmem_create_rust(&mut buf, XdrOp::Encode);

        let data = [0x01, 0x02, 0x03];
        assert_eq!(xdr.putbytes(data.as_ptr(), 3), TRUE);
        assert_eq!(xdr.getpos(), 3);

        drop(xdr);

        let mut xdr2 = xdrmem_create_rust(&mut buf, XdrOp::Decode);
        let mut result = [0u8; 3];
        assert_eq!(xdr2.getbytes(result.as_mut_ptr(), 3), TRUE);
        assert_eq!(result, [0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_xdr_setpos_out_of_bounds() {
        let mut buf = [0u8; 8];
        let mut xdr = xdrmem_create_rust(&mut buf, XdrOp::Decode);
        // Position 9 is beyond buffer
        assert_eq!(xdr.setpos(9), FALSE);
        // Position 8 is exactly at the end (allowed)
        assert_eq!(xdr.setpos(8), TRUE);
    }
}
