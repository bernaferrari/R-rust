/*
 *  R : A Computer Language for Statistical Data Analysis
 *  (C) Copyright 2008-2011 Simon Urbanek
 *      Copyright 2011-2025 R Core Team.
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *  Ported from r-source/src/library/parallel/src/fork.c
 *
 *  interface to system-level tools for spawning copies of the current
 *  process and IPC
 *
 *  Derived from multicore version 0.1-8 by Simon Urbanek
 */

use std::cell::Cell;
use std::os::raw::{c_char, c_double, c_int, c_uint};
use std::ptr;

use crate::attrib_core::setAttrib;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;
use crate::sexp::symbol::Rf_install;

#[cfg(unix)]
use libc::{
    self, EINTR, EPIPE, FD_ISSET, FD_SET, FD_SETSIZE, FD_ZERO, O_WRONLY, SA_RESTART, SIGCHLD,
    SIGUSR1, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO, WEXITSTATUS, WIFEXITED, WIFSIGNALED,
    WNOHANG, WTERMSIG, c_void, close, dup2, fd_set, fork, kill, open, pid_t, pipe, read, sigaction,
    sigaddset, sigemptyset, siginfo_t, signal, sigprocmask, sigset_t, size_t, ssize_t, strdup,
    timeval, usleep, waitpid, write,
};

/// Get errno pointer (macOS uses __error(), Linux/Android use __errno_location).
#[cfg(unix)]
#[inline]
unsafe fn errno_ptr() -> *mut c_int {
    #[cfg(target_os = "macos")]
    {
        libc::__error()
    }
    #[cfg(not(target_os = "macos"))]
    {
        unsafe extern "C" {
            fn __errno_location() -> *mut c_int;
        }
        __errno_location()
    }
}

// read()/write() on pipes may not support arbitrary lengths, so
// this is the largest chunk we'll ever send with one call between
// a child and the parent. On macOS empirically this has to be at
// most a 32-bit number. Current default is 1Gb.
const MC_MAX_CHUNK: size_t = 0x40000000;

// STDIN/STDOUT/STDERR file descriptor constants
const R_STDIN_FILENO: c_int = 0;
const R_STDOUT_FILENO: c_int = 1;
const R_STDERR_FILENO: c_int = 2;

/* A child is created in mc_fork as detached (sEstranged=TRUE, has sifd
   and pfd set to -1) or attached (sifd and pifd connected to pipes). The list
   of children also includes cleanup marks.

   A detached child is not visible to R user code. Upon receiving sigchld,
   a detached child is waited-for from the signal handler and waitedfor is
   set. The child is eventually removed from the list by compact_children.

   An attached child is visible to R user code and always has file descriptors
   sifd and pifd open and >= 0. It becomes detached via readChild() when it
   returns an integer (signalling to user that the child is finishing or has
   failed). An attached child is never waited for in the signal handler.

   A cleanup mark is a child entry that is detached, waited-for, and has a
   pid of -1.
*/
#[cfg(unix)]
#[repr(C)]
struct child_info_t {
    pid: pid_t,
    pfd: c_int,
    sifd: c_int,
    detached: c_int,
    waitedfor: c_int,
    ppid: pid_t,
    next: *mut child_info_t,
}

#[cfg(unix)]
thread_local! { static children: Cell<*mut child_info_t> = Cell::new(ptr::null_mut()); }

#[cfg(unix)]
thread_local! { static master_fd: Cell<c_int> = Cell::new(-1); }

#[cfg(unix)]
thread_local! { static is_master: Cell<c_int> = Cell::new(1); }

#[cfg(unix)]
thread_local! { static child_can_exit: Cell<c_int> = Cell::new(0); }

#[cfg(unix)]
thread_local! { static child_exit_status: Cell<c_int> = Cell::new(-1); }

#[cfg(unix)]
thread_local! { static parent_handler_set: Cell<c_int> = Cell::new(0); }

#[cfg(unix)]
thread_local! { static old_sig_handler: Cell<sigaction> = Cell::new(unsafe { std::mem::zeroed() }); }

#[cfg(unix)]
thread_local! { static R_isForkedChild: Cell<c_int> = Cell::new(0); }

#[cfg(unix)]
thread_local! { static R_ignore_SIGPIPE: Cell<c_int> = Cell::new(0); }

#[cfg(unix)]
thread_local! { static R_Interactive: Cell<c_int> = Cell::new(1); }

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn current_time() -> c_double {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs_f64(),
        Err(_) => 0.0,
    }
}

#[cfg(unix)]
unsafe fn block_sigchld(oldset: *mut sigset_t) {
    let mut ss: sigset_t = std::mem::zeroed();
    sigemptyset(&mut ss);
    sigaddset(&mut ss, SIGCHLD);
    sigprocmask(libc::SIG_BLOCK, &ss, oldset);
}

#[cfg(unix)]
unsafe fn restore_sigchld(oldset: *const sigset_t) {
    sigprocmask(libc::SIG_SETMASK, oldset as *mut sigset_t, ptr::null_mut());
}

#[cfg(unix)]
unsafe fn close_fds_child_ci(ci: *mut child_info_t) {
    if (*ci).pfd >= 0 {
        close((*ci).pfd);
        (*ci).pfd = -1;
    }
    if (*ci).sifd >= 0 {
        close((*ci).sifd);
        (*ci).sifd = -1;
    }
}

#[cfg(unix)]
unsafe fn fd_used_by_children(fd: c_int) -> c_int {
    if fd == -1 {
        return 0;
    }
    let mut ci = children.with(|v| v.get());
    while !ci.is_null() {
        if (*ci).pfd == fd || (*ci).sifd == fd {
            return 1;
        }
        ci = (*ci).next;
    }
    0
}

#[cfg(unix)]
unsafe fn close_non_child_fd(fd: c_int) {
    if fd_used_by_children(fd) != 0 {
        crate::main::errors::Rf_error(
            b"cannot close internal file descriptor\0".as_ptr() as *const c_char
        );
    }
    close(fd);
}

// ---------------------------------------------------------------------------
// Signal handlers
// ---------------------------------------------------------------------------

#[cfg(unix)]
unsafe fn child_sig_handler(sig: c_int) {
    if sig == SIGUSR1 {
        child_can_exit.with(|v| v.set(1));
        if child_exit_status.with(|v| v.get()) >= 0 {
            libc::_exit(child_exit_status.with(|v| v.get()));
        }
    }
}

#[cfg(unix)]
unsafe fn parent_sig_handler(_sig: c_int) {
    let old_errno = *errno_ptr();
    let mut ci = children.with(|v| v.get());
    while !ci.is_null() {
        if (*ci).detached != 0 && (*ci).waitedfor == 0 {
            wait_for_child_ci(ci);
        }
        ci = (*ci).next;
    }
    *errno_ptr() = old_errno;
}

#[cfg(unix)]
unsafe fn wait_for_child_ci(ci: *mut child_info_t) {
    let mut wstat: c_int = 0;
    if waitpid((*ci).pid, &mut wstat, WNOHANG) == (*ci).pid
        && (WIFEXITED(wstat) || WIFSIGNALED(wstat))
    {
        (*ci).waitedfor = 1;
    }
}

#[cfg(unix)]
unsafe fn setup_sig_handler() {
    if parent_handler_set.with(|v| v.get()) == 0 {
        parent_handler_set.with(|v| v.set(1));
        let mut sa: sigaction = std::mem::zeroed();
        sigemptyset(&mut sa.sa_mask);
        sa.sa_sigaction = parent_sig_handler as *const () as usize;
        sa.sa_flags = SA_RESTART | libc::SA_SIGINFO;
        sigaction(
            SIGCHLD,
            &sa,
            &mut old_sig_handler.with(|v| v.replace(std::mem::zeroed())),
        );
    }
}

#[cfg(unix)]
unsafe fn restore_sig_handler() {
    if parent_handler_set.with(|v| v.get()) != 0 {
        parent_handler_set.with(|v| v.set(0));
        let old = old_sig_handler.with(|v| v.replace(std::mem::zeroed()));
        sigaction(SIGCHLD, &old, ptr::null_mut());
    }
}

// ---------------------------------------------------------------------------
// Child management
// ---------------------------------------------------------------------------

#[cfg(unix)]
unsafe fn kill_and_detach_child_ci(ci: *mut child_info_t, sig: c_int) {
    let mut ss: sigset_t = std::mem::zeroed();
    block_sigchld(&mut ss);

    close_fds_child_ci(ci);

    if kill((*ci).pid, sig) == -1 {
        let err_str = libc::strerror(*errno_ptr());
        crate::main::errors::Rf_warning(
            b"unable to terminate child process: %s\0".as_ptr() as *const c_char
        );
        let _ = err_str; // suppress unused warning
    }

    (*ci).detached = 1;
    wait_for_child_ci(ci);
    restore_sigchld(&ss);
}

#[cfg(unix)]
unsafe fn terminate_and_detach_child_ci(ci: *mut child_info_t) {
    kill_and_detach_child_ci(ci, SIGUSR1);
}

#[cfg(unix)]
unsafe fn kill_detached_child_ci(ci: *mut child_info_t, sig: c_int) {
    let mut ss: sigset_t = std::mem::zeroed();
    block_sigchld(&mut ss);

    if (*ci).waitedfor == 0 {
        if kill((*ci).pid, sig) == -1 {
            crate::main::errors::Rf_warning(
                b"unable to terminate child: %s\0".as_ptr() as *const c_char
            );
        }
    }
    restore_sigchld(&ss);
}

#[cfg(unix)]
unsafe fn rm_child(pid: c_int) -> c_int {
    let mut ci = children.with(|v| v.get());
    let ppid = libc::getpid();
    while !ci.is_null() {
        if (*ci).detached == 0 && (*ci).pid == pid && (*ci).ppid == ppid {
            terminate_and_detach_child_ci(ci);
            return 1;
        }
        ci = (*ci).next;
    }
    0
}

#[cfg(unix)]
unsafe fn compact_children() {
    let mut ci = children.with(|v| v.get());
    let mut prev: *mut child_info_t = ptr::null_mut();
    let ppid = libc::getpid();

    let mut ss: sigset_t = std::mem::zeroed();
    block_sigchld(&mut ss);

    while !ci.is_null() {
        if ((*ci).waitedfor != 0 && (*ci).pid >= 0) || (*ci).ppid != ppid {
            if (*ci).ppid != ppid {
                close_fds_child_ci(ci);
            }
            let next = (*ci).next;
            if !prev.is_null() {
                (*prev).next = next;
            } else {
                children.with(|v| v.set(next));
            }
            libc::free(ci as *mut c_void);
            ci = next;
        } else {
            prev = ci;
            ci = (*ci).next;
        }
    }

    restore_sigchld(&ss);
}

// ---------------------------------------------------------------------------
// fd_set utilities
// ---------------------------------------------------------------------------

#[cfg(unix)]
unsafe fn fdcopy(dst: *mut fd_set, src: *const fd_set, nfds: c_int) {
    FD_ZERO(dst);
    if nfds as usize > FD_SETSIZE {
        crate::main::errors::Rf_error(
            b"file descriptor is too large for select()\0".as_ptr() as *const c_char
        );
    }
    for i in 0..nfds {
        if FD_ISSET(i as c_int, src) {
            FD_SET(i as c_int, dst);
        }
    }
}

// ---------------------------------------------------------------------------
// read/write with restart on signal interrupt
// ---------------------------------------------------------------------------

#[cfg(unix)]
unsafe fn readrep(fildes: c_int, buf: *mut c_void, nbyte: size_t) -> ssize_t {
    let mut rbyte: size_t = 0;
    let ptr = buf as *mut u8;
    loop {
        let r = read(fildes, ptr.add(rbyte) as *mut c_void, nbyte - rbyte);
        if r == -1 {
            if *errno_ptr() == EINTR {
                continue;
            } else {
                return -1;
            }
        }
        if r == 0 {
            return rbyte as ssize_t;
        }
        rbyte += r as size_t;
        if rbyte == nbyte {
            return rbyte as ssize_t;
        }
    }
}

#[cfg(unix)]
unsafe fn writerep(fildes: c_int, buf: *const c_void, nbyte: size_t) -> ssize_t {
    let mut wbyte: size_t = 0;
    let ptr = buf as *const u8;
    loop {
        let w = write(fildes, ptr.add(wbyte) as *const c_void, nbyte - wbyte);
        if w == -1 {
            if *errno_ptr() == EINTR {
                continue;
            } else {
                return -1;
            }
        }
        if w == 0 {
            return wbyte as ssize_t;
        }
        wbyte += w as size_t;
        if wbyte == nbyte {
            return wbyte as ssize_t;
        }
    }
}

// ---------------------------------------------------------------------------
// A simple select wrapper (R_SelectEx replacement)
// ---------------------------------------------------------------------------

#[cfg(unix)]
unsafe fn mc_select(
    n: c_int,
    readfds: *mut fd_set,
    writefds: *mut fd_set,
    exceptfds: *mut fd_set,
    timeout: *mut timeval,
) -> c_int {
    // R_SelectEx is essentially select() with input handler support.
    // For the port, we just use libc::select directly.
    libc::select(n, readfds, writefds, exceptfds, timeout)
}

// ---------------------------------------------------------------------------
// Exported functions (Unix only)
// ---------------------------------------------------------------------------

/// Insert a cleanup mark into the children list.
#[cfg(unix)]
pub unsafe fn mc_prepare_cleanup() -> SEXP {
    compact_children();
    let ci = libc::malloc(std::mem::size_of::<child_info_t>()) as *mut child_info_t;
    if ci.is_null() {
        crate::main::errors::Rf_error(b"memory allocation error\0".as_ptr() as *const c_char);
    }
    (*ci).waitedfor = 1;
    (*ci).detached = 1;
    (*ci).pid = -1;
    (*ci).pfd = -1;
    (*ci).sifd = -1;
    (*ci).ppid = libc::getpid();
    (*ci).next = children.with(|v| v.get());
    children.with(|v| v.set(ci));

    R_NilValue()
}

/// Terminate and detach all children up to the first cleanup mark.
#[cfg(unix)]
pub unsafe fn mc_cleanup(sKill: SEXP, sDetach: SEXP, sShutdown: SEXP) -> SEXP {
    use crate::main::coerce::{asInteger, asLogical};

    let mut sig: c_int = -1;
    let kill_type = TYPEOF(sKill);
    if kill_type == SEXPTYPE::LGLSXP {
        let lkill = asLogical(sKill);
        if lkill == 1 {
            sig = libc::SIGTERM;
        } else if lkill == 0 {
            sig = 0;
        }
    } else {
        let ikill = asInteger(sKill);
        if ikill >= 1 && ikill != NA_INTEGER {
            sig = ikill;
        }
    }
    if sig == -1 {
        crate::main::errors::Rf_error(b"invalid 'mc.cleanup' argument\0".as_ptr() as *const c_char);
    }

    let detach = asLogical(sDetach);
    if detach == NA_INTEGER {
        crate::main::errors::Rf_error(b"invalid 'detach' argument\0".as_ptr() as *const c_char);
    }

    let shutdown = asLogical(sShutdown);
    if shutdown == NA_INTEGER {
        crate::main::errors::Rf_error(b"invalid 'shutdown' argument\0".as_ptr() as *const c_char);
    }

    compact_children();

    let mut ci = children.with(|v| v.get());
    let mut nattached: c_int = 0;
    while !ci.is_null() {
        if (*ci).detached != 0 && (*ci).waitedfor != 0 && (*ci).pid == -1 {
            // cleanup mark
            if sig != 0 || shutdown != 0 {
                (*ci).pid = c_int::MAX;
            }
            if shutdown == 0 {
                break;
            }
        }
        if (*ci).detached != 0 && sig != 0 {
            kill_detached_child_ci(ci, sig);
        }
        if (*ci).detached == 0 && detach != 0 {
            let send_sig = if sig != 0 { sig } else { SIGUSR1 };
            kill_and_detach_child_ci(ci, send_sig);
            nattached += 1;
        }
        ci = (*ci).next;
    }

    if nattached > 0 {
        libc::sleep(1);
    }
    compact_children();

    if shutdown != 0 {
        let before = current_time();
        while !children.with(|v| v.get()).is_null() {
            libc::sleep(1);
            compact_children();
            if children.with(|v| v.get()).is_null() {
                break;
            }
            let now = current_time();
            if now - before > 10.0 {
                // Give up after 10 seconds
                restore_sig_handler();
                return R_NilValue();
            }
        }
        restore_sig_handler();
    }
    R_NilValue()
}

/// Fork a child process.
///
/// Returns a length-3 integer vector:
///   [0] = child PID (child returns 0)
///   [1] = file descriptor of the data pipe (child->master)
///   [2] = file descriptor of the child-stdin pipe (master->child)
#[cfg(unix)]
pub unsafe fn mc_fork(sEstranged: SEXP) -> SEXP {
    use crate::main::coerce::asInteger;

    let mut pipefd: [c_int; 2] = [-1, -1]; // write end, read end
    let mut sipfd: [c_int; 2] = [-1, -1];
    let mut pid: pid_t = 0;
    let estranged = asInteger(sEstranged) > 0;
    let res = Rf_allocVector(SEXPTYPE::INTSXP, 3);
    let res_i = INTEGER(res);

    if !estranged {
        if pipe(pipefd.as_mut_ptr()) != 0 {
            crate::main::errors::Rf_error(b"unable to create a pipe\0".as_ptr() as *const c_char);
        }
        if pipe(sipfd.as_mut_ptr()) != 0 {
            close(pipefd[0]);
            close(pipefd[1]);
            crate::main::errors::Rf_error(b"unable to create a pipe\0".as_ptr() as *const c_char);
        }
    }

    setup_sig_handler();

    let mut ss: sigset_t = std::mem::zeroed();
    block_sigchld(&mut ss);

    // Flush stdout before forking
    libc::fflush(std::ptr::null_mut());

    pid = fork();
    if pid == -1 {
        if !estranged {
            close(pipefd[0]);
            close(pipefd[1]);
            close(sipfd[0]);
            close(sipfd[1]);
        }
        crate::main::errors::Rf_error(
            b"unable to fork, possible reason: %s\0".as_ptr() as *const c_char
        );
    }

    *res_i.add(0) = pid as c_int;

    if pid == 0 {
        // Child process
        R_isForkedChild.with(|v| v.set(1));

        // Free children entries inherited from parent
        while !children.with(|v| v.get()).is_null() {
            close_fds_child_ci(children.with(|v| v.get()));
            let next = (*children.with(|v| v.get())).next;
            libc::free(children.with(|v| v.get()) as *mut c_void);
            children.with(|v| v.set(next));
        }

        restore_sigchld(&ss);
        restore_sig_handler();

        if estranged {
            *res_i.add(1) = NA_INTEGER as c_int;
            *res_i.add(2) = NA_INTEGER as c_int;
        } else {
            close(pipefd[0]);
            close(sipfd[1]);
            master_fd.with(|v| v.set(*res_i.add(1)));
            *res_i.add(1) = pipefd[1];
            *res_i.add(2) = NA_INTEGER as c_int;

            dup2(sipfd[0], R_STDIN_FILENO);
            close(sipfd[0]);
        }
        is_master.with(|v| v.set(0));
        child_exit_status.with(|v| v.set(-1));

        if estranged {
            child_can_exit.with(|v| v.set(1));
        } else {
            child_can_exit.with(|v| v.set(0));
            signal(SIGUSR1, child_sig_handler as *const () as usize);
        }
    } else {
        // Master process
        let ci = libc::malloc(std::mem::size_of::<child_info_t>()) as *mut child_info_t;
        if ci.is_null() {
            crate::main::errors::Rf_error(b"memory allocation error\0".as_ptr() as *const c_char);
        }
        (*ci).pid = pid;
        (*ci).ppid = libc::getpid();
        (*ci).waitedfor = 0;

        if estranged {
            (*ci).detached = 1;
            *res_i.add(1) = NA_INTEGER as c_int;
            *res_i.add(2) = NA_INTEGER as c_int;
            (*ci).pfd = -1;
            (*ci).sifd = -1;
        } else {
            (*ci).detached = 0;
            close(pipefd[1]); // close write end of data pipe
            close(sipfd[0]); // close read end of child-stdin pipe
            *res_i.add(1) = pipefd[0];
            *res_i.add(2) = sipfd[1];
            (*ci).pfd = pipefd[0];
            (*ci).sifd = sipfd[1];
        }

        (*ci).next = children.with(|v| v.get());
        children.with(|v| v.set(ci));
        restore_sigchld(&ss);
    }

    res
}

/// Close or redirect stdout.
#[cfg(unix)]
pub unsafe fn mc_close_stdout(toNULL: SEXP) -> SEXP {
    use crate::main::coerce::asLogical;

    if asLogical(toNULL) == 1 {
        let fd = open(b"/dev/null\0".as_ptr() as *const c_char, O_WRONLY);
        if fd != -1 {
            dup2(fd, R_STDOUT_FILENO);
            close(fd);
        } else {
            close_non_child_fd(R_STDOUT_FILENO);
        }
    } else {
        close_non_child_fd(R_STDOUT_FILENO);
    }
    R_NilValue()
}

/// Close or redirect stderr.
#[cfg(unix)]
pub unsafe fn mc_close_stderr(toNULL: SEXP) -> SEXP {
    use crate::main::coerce::asLogical;

    if asLogical(toNULL) == 1 {
        let fd = open(b"/dev/null\0".as_ptr() as *const c_char, O_WRONLY);
        if fd != -1 {
            dup2(fd, R_STDERR_FILENO);
            close(fd);
        } else {
            close_non_child_fd(R_STDERR_FILENO);
        }
    } else {
        close_non_child_fd(R_STDERR_FILENO);
    }
    R_NilValue()
}

/// Close file descriptors.
#[cfg(unix)]
pub unsafe fn mc_close_fds(sFDS: SEXP) -> SEXP {
    if TYPEOF(sFDS) != SEXPTYPE::INTSXP {
        crate::main::errors::Rf_error(b"descriptors must be integers\0".as_ptr() as *const c_char);
    }
    let fds = LENGTH(sFDS);
    let fd = INTEGER(sFDS);
    let mut i: c_int = 0;
    while i < fds {
        close_non_child_fd(*fd.add(i as usize));
        i += 1;
    }
    Rf_ScalarLogical(1)
}

/// Send data from child to master process.
#[cfg(unix)]
pub unsafe fn mc_send_master(what: SEXP) -> SEXP {
    if is_master.with(|v| v.get()) != 0 {
        crate::main::errors::Rf_error(
            b"only children can send data to the master process\0".as_ptr() as *const c_char,
        );
    }
    if master_fd.with(|v| v.get()) == -1 {
        crate::main::errors::Rf_error(
            b"there is no pipe to the master process\0".as_ptr() as *const c_char
        );
    }
    if TYPEOF(what) != SEXPTYPE::RAWSXP {
        crate::main::errors::Rf_error(
            b"content to send must be RAW, use serialize() if needed\0".as_ptr() as *const c_char,
        );
    }

    let len = XLENGTH(what);
    let b = RAW(what);
    let fd = master_fd.with(|v| v.get());

    if writerep(
        fd,
        &len as *const R_xlen_t as *const c_void,
        std::mem::size_of::<R_xlen_t>(),
    ) != std::mem::size_of::<R_xlen_t>() as ssize_t
    {
        close(fd);
        master_fd.with(|v| v.set(-1));
        crate::main::errors::Rf_error(
            b"write error, closing pipe to the master\0".as_ptr() as *const c_char
        );
    }

    let mut i: R_xlen_t = 0;
    while i < len {
        let mut to_send: size_t = (len - i) as size_t;
        if to_send > MC_MAX_CHUNK {
            to_send = MC_MAX_CHUNK;
        }
        let n = writerep(
            master_fd.with(|v| v.get()),
            b.add(i as usize) as *const c_void,
            to_send,
        );
        if n < 1 {
            close(master_fd.with(|v| v.get()));
            master_fd.with(|v| v.set(-1));
            crate::main::errors::Rf_error(
                b"write error, closing pipe to the master\0".as_ptr() as *const c_char
            );
        }
        i += n as R_xlen_t;
    }

    Rf_ScalarLogical(1)
}

/// Send data from master to child process stdin.
#[cfg(unix)]
pub unsafe fn mc_send_child_stdin(sPid: SEXP, what: SEXP) -> SEXP {
    use crate::main::coerce::asInteger;

    let pid = asInteger(sPid);
    if is_master.with(|v| v.get()) == 0 {
        crate::main::errors::Rf_error(
            b"only the master process can send data to a child process\0".as_ptr() as *const c_char,
        );
    }
    if TYPEOF(what) != SEXPTYPE::RAWSXP {
        crate::main::errors::Rf_error(b"what must be a raw vector\0".as_ptr() as *const c_char);
    }

    let mut ci = children.with(|v| v.get());
    let ppid = libc::getpid();
    while !ci.is_null() {
        if (*ci).detached == 0 && (*ci).pid == pid && (*ci).ppid == ppid {
            break;
        }
        ci = (*ci).next;
    }
    if ci.is_null() || (*ci).sifd < 0 {
        crate::main::errors::Rf_error(b"child %d does not exist\0".as_ptr() as *const c_char);
    }

    let len = XLENGTH(what);
    let b = RAW(what);
    let fd = (*ci).sifd as c_uint;
    let mut i: R_xlen_t = 0;
    while i < len {
        let n = writerep(
            fd as c_int,
            b.add(i as usize) as *const c_void,
            (len - i) as size_t,
        );
        if n < 1 {
            crate::main::errors::Rf_error(b"write error\0".as_ptr() as *const c_char);
        }
        i += n as R_xlen_t;
    }

    Rf_ScalarLogical(1)
}

/// Select children with data available to read.
#[cfg(unix)]
pub unsafe fn mc_select_children(sTimeout: SEXP, sWhich: SEXP) -> SEXP {
    use crate::main::coerce::{asInteger, asReal};

    let mut maxfd: c_int = -1;
    let mut sr: c_int = 0;
    let mut wlen: c_uint = 0;
    let mut wcount: c_uint = 0;
    let mut which: *const c_int = ptr::null();
    let mut ci = children.with(|v| v.get());
    let mut fs: fd_set = std::mem::zeroed();
    let mut timeout: c_double = 0.0;
    let ppid = libc::getpid();

    if TYPEOF(sTimeout) == SEXPTYPE::REALSXP && LENGTH(sTimeout) == 1 {
        timeout = asReal(sTimeout);
    }

    if TYPEOF(sWhich) == SEXPTYPE::INTSXP && LENGTH(sWhich) > 0 {
        which = INTEGER(sWhich);
        wlen = LENGTH(sWhich) as c_uint;
    }

    FD_ZERO(&mut fs);
    while !ci.is_null() {
        if (*ci).detached == 0 && (*ci).ppid == ppid {
            if !which.is_null() {
                let mut k: c_uint = 0;
                while k < wlen {
                    if *which.add(k as usize) == (*ci).pid {
                        if (*ci).pfd as usize >= FD_SETSIZE {
                            crate::main::errors::Rf_error(
                                b"file descriptor is too large for select()\0".as_ptr()
                                    as *const c_char,
                            );
                        }
                        FD_SET((*ci).pfd, &mut fs);
                        if (*ci).pfd > maxfd {
                            maxfd = (*ci).pfd;
                        }
                        wcount += 1;
                        break;
                    }
                    k += 1;
                }
            } else {
                if (*ci).pfd as usize >= FD_SETSIZE {
                    crate::main::errors::Rf_error(
                        b"file descriptor is too large for select()\0".as_ptr() as *const c_char,
                    );
                }
                FD_SET((*ci).pfd, &mut fs);
                if (*ci).pfd > maxfd {
                    maxfd = (*ci).pfd;
                }
            }
        }
        ci = (*ci).next;
    }

    if !which.is_null() && wcount < wlen {
        let mut k: c_uint = 0;
        while k < wlen {
            let mut found: c_int = 0;
            let mut ci2 = children.with(|v| v.get());
            while !ci2.is_null() {
                if (*ci2).detached == 0
                    && (*ci2).ppid == ppid
                    && (*ci2).pid == *which.add(k as usize)
                    && FD_ISSET((*ci2).pfd, &fs)
                {
                    found = 1;
                    break;
                }
                ci2 = (*ci2).next;
            }
            if found == 0 {
                crate::main::errors::Rf_warning(
                    b"cannot wait for child %d as it does not exist\0".as_ptr() as *const c_char,
                );
            }
            k += 1;
        }
    }

    if maxfd == -1 {
        return R_NilValue();
    }

    if timeout == 0.0 {
        let mut tv: timeval = std::mem::zeroed();
        tv.tv_sec = 0;
        tv.tv_usec = 0;
        sr = mc_select(
            maxfd + 1,
            &mut fs,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut tv,
        );
    } else {
        let before = current_time();
        let mut remains = timeout;
        let mut tv: timeval = std::mem::zeroed();
        let mut savefs: fd_set = std::mem::zeroed();
        fdcopy(&mut savefs, &fs, maxfd + 1);

        loop {
            // Process events (stub - equivalent to R_ProcessEvents)
            // In the full implementation this would call R_ProcessEvents()
            // to handle GUI events, etc.

            // Re-set tv as it may get updated by select
            // Note: R_wait_usec is a global; we treat it as 0 (no external wait)
            if timeout > 0.0 {
                tv.tv_sec = remains as i64;
                tv.tv_usec = ((remains - (remains as i64) as c_double) * 1_000_000.0) as libc::suseconds_t;
            } else {
                tv.tv_sec = 1; // still allow to process events
                tv.tv_usec = 0;
            }

            sr = mc_select(
                maxfd + 1,
                &mut fs,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut tv,
            );
            if sr > 0 || (sr < 0 && *errno_ptr() != EINTR) {
                break;
            }
            if timeout > 0.0 {
                remains = timeout - (current_time() - before);
                if remains <= 0.0 {
                    break;
                }
            }
            fdcopy(&mut fs, &savefs, maxfd + 1);
        }
    }

    if sr < 0 {
        if *errno_ptr() == EINTR {
            return Rf_ScalarLogical(1);
        }
        crate::main::errors::Rf_warning(b"error '%s' in select\0".as_ptr() as *const c_char);
        return Rf_ScalarLogical(0);
    }
    if sr < 1 {
        return Rf_ScalarLogical(1);
    }

    ci = children.with(|v| v.get());
    let res = Rf_allocVector(SEXPTYPE::INTSXP, sr as c_int);
    let mut res_i = INTEGER(res);
    while !ci.is_null() {
        if (*ci).detached == 0 && (*ci).ppid == ppid && FD_ISSET((*ci).pfd, &fs) {
            *res_i = (*ci).pid as c_int;
            res_i = res_i.add(1);
        }
        ci = (*ci).next;
    }
    res
}

/// Read data from a child process.
#[cfg(unix)]
unsafe fn read_child_ci(ci: *mut child_info_t) -> SEXP {
    if (*ci).detached != 0 {
        return R_NilValue();
    }

    let mut len: R_xlen_t = 0;
    let fd = (*ci).pfd;
    let pid = (*ci).pid;

    let n = readrep(
        fd,
        &mut len as *mut R_xlen_t as *mut c_void,
        std::mem::size_of::<R_xlen_t>(),
    );

    if n != std::mem::size_of::<R_xlen_t>() as ssize_t || len == 0 {
        terminate_and_detach_child_ci(ci);
        return Rf_ScalarInteger(pid as c_int);
    } else {
        let rv = Rf_allocVector(SEXPTYPE::RAWSXP, len as c_int);
        let rvb = RAW(rv);
        let mut i: R_xlen_t = 0;
        while i < len {
            let mut to_read: size_t = (len - i) as size_t;
            if to_read > MC_MAX_CHUNK {
                to_read = MC_MAX_CHUNK;
            }
            let n = readrep(fd, rvb.add(i as usize) as *mut c_void, to_read);
            if n < 1 {
                terminate_and_detach_child_ci(ci);
                return Rf_ScalarInteger(pid as c_int);
            }
            i += n as R_xlen_t;
        }
        Rf_protect(rv);
        let pa = Rf_ScalarInteger(pid as c_int);
        Rf_protect(pa);
        let pid_sym = Rf_install(b"pid\0".as_ptr() as *const c_char);
        setAttrib(rv, pid_sym, pa);
        Rf_unprotect(2);
        rv
    }
}

/// Read data from a specific child.
#[cfg(unix)]
pub unsafe fn mc_read_child(sPid: SEXP) -> SEXP {
    use crate::main::coerce::asInteger;

    let pid = asInteger(sPid);
    let mut ci = children.with(|v| v.get());
    let ppid = libc::getpid();
    while !ci.is_null() {
        if (*ci).detached == 0 && (*ci).pid == pid && (*ci).ppid == ppid {
            break;
        }
        ci = (*ci).next;
    }
    if ci.is_null() {
        return R_NilValue();
    }
    read_child_ci(ci)
}

/// Read data from any available child.
#[cfg(unix)]
pub unsafe fn mc_read_children(sTimeout: SEXP) -> SEXP {
    use crate::main::coerce::asReal;

    let mut maxfd: c_int = 0;
    let mut sr: c_int = 0;
    let mut ci = children.with(|v| v.get());
    let mut fs: fd_set = std::mem::zeroed();
    let mut tv: timeval = std::mem::zeroed();
    tv.tv_sec = 0;
    tv.tv_usec = 0;
    let mut tvp: *mut timeval = &mut tv;

    if TYPEOF(sTimeout) == SEXPTYPE::REALSXP && LENGTH(sTimeout) == 1 {
        let tov = asReal(sTimeout);
        if tov < 0.0 {
            tvp = ptr::null_mut();
        } else {
            tv.tv_sec = tov as i64;
            tv.tv_usec = ((tov - (tov as i64) as c_double) * 1_000_000.0) as libc::suseconds_t;
        }
    }

    // Check for zombies
    let mut wstat: c_int = 0;
    while waitpid(-1, &mut wstat, WNOHANG) > 0 {}

    FD_ZERO(&mut fs);
    let ppid = libc::getpid();
    while !ci.is_null() {
        if (*ci).detached == 0 && (*ci).ppid == ppid {
            if (*ci).pfd > maxfd {
                maxfd = (*ci).pfd;
            }
            if (*ci).pfd >= 0 {
                if (*ci).pfd as usize >= FD_SETSIZE {
                    crate::main::errors::Rf_error(
                        b"file descriptor is too large for select()\0".as_ptr() as *const c_char,
                    );
                }
                FD_SET((*ci).pfd, &mut fs);
            }
        }
        ci = (*ci).next;
    }

    if maxfd == 0 {
        return R_NilValue();
    }

    sr = mc_select(maxfd + 1, &mut fs, ptr::null_mut(), ptr::null_mut(), tvp);

    if sr < 0 {
        crate::main::errors::Rf_warning(b"error '%s' in select\0".as_ptr() as *const c_char);
        return Rf_ScalarLogical(0);
    }
    if sr < 1 {
        return Rf_ScalarLogical(1);
    }

    ci = children.with(|v| v.get());
    while !ci.is_null() {
        if (*ci).detached == 0 && (*ci).ppid == ppid {
            if (*ci).pfd >= 0 && FD_ISSET((*ci).pfd, &fs) {
                break;
            }
        }
        ci = (*ci).next;
    }

    if ci.is_null() {
        Rf_ScalarLogical(1)
    } else {
        read_child_ci(ci)
    }
}

/// Remove a child process.
#[cfg(unix)]
pub unsafe fn mc_rm_child(sPid: SEXP) -> SEXP {
    use crate::main::coerce::asInteger;

    let pid = asInteger(sPid);
    Rf_ScalarLogical(rm_child(pid))
}

/// Get list of attached children PIDs.
#[cfg(unix)]
pub unsafe fn mc_children() -> SEXP {
    let mut ci = children.with(|v| v.get());
    let mut count: c_uint = 0;
    let ppid = libc::getpid();
    while !ci.is_null() {
        if (*ci).detached == 0 && (*ci).ppid == ppid {
            count += 1;
        }
        ci = (*ci).next;
    }

    let res = Rf_allocVector(SEXPTYPE::INTSXP, count as c_int);
    if count > 0 {
        let mut pids = INTEGER(res);
        ci = children.with(|v| v.get());
        while !ci.is_null() {
            if (*ci).detached == 0 && (*ci).ppid == ppid {
                *pids = (*ci).pid as c_int;
                pids = pids.add(1);
            }
            ci = (*ci).next;
        }
    }
    res
}

/// Get file descriptors of attached children.
#[cfg(unix)]
pub unsafe fn mc_fds(sFdi: SEXP) -> SEXP {
    use crate::main::coerce::asInteger;

    let fdi = asInteger(sFdi);
    let mut count: c_uint = 0;
    let mut ci = children.with(|v| v.get());
    let ppid = libc::getpid();
    while !ci.is_null() {
        if (*ci).detached == 0 && (*ci).ppid == ppid {
            count += 1;
        }
        ci = (*ci).next;
    }

    let res = Rf_allocVector(SEXPTYPE::INTSXP, count as c_int);
    if count > 0 {
        let mut fds = INTEGER(res);
        ci = children.with(|v| v.get());
        while !ci.is_null() {
            if (*ci).detached == 0 && (*ci).ppid == ppid {
                *fds = if fdi == 0 { (*ci).pfd } else { (*ci).sifd };
                fds = fds.add(1);
            }
            ci = (*ci).next;
        }
    }
    res
}

/// Get the master file descriptor (child side).
#[cfg(unix)]
pub unsafe fn mc_master_fd() -> SEXP {
    Rf_ScalarInteger(master_fd.with(|v| v.get()))
}

/// Check if current process is a child.
#[cfg(unix)]
pub unsafe fn mc_is_child() -> SEXP {
    Rf_ScalarLogical(if is_master.with(|v| v.get()) != 0 {
        0
    } else {
        1
    })
}

/// Send a signal to a process.
#[cfg(unix)]
pub unsafe fn mc_kill(sPid: SEXP, sSig: SEXP) -> SEXP {
    use crate::main::coerce::asInteger;

    let pid = asInteger(sPid);
    let sig = asInteger(sSig);
    if kill(pid as pid_t, sig) != 0 {
        crate::main::errors::Rf_error(b"'mckill' failed\0".as_ptr() as *const c_char);
    }
    Rf_ScalarLogical(1)
}

/// Exit a child process.
#[cfg(unix)]
pub unsafe fn mc_exit(sRes: SEXP) -> SEXP {
    use crate::main::coerce::asInteger;

    let res = asInteger(sRes);

    if is_master.with(|v| v.get()) != 0 {
        crate::main::errors::Rf_error(
            b"'mcexit' can only be used in a child process\0".as_ptr() as *const c_char
        );
    }

    if master_fd.with(|v| v.get()) != -1 {
        let len: size_t = 0;
        R_ignore_SIGPIPE.with(|v| v.set(1));
        let fd = master_fd.with(|v| v.get());
        let n = writerep(
            fd,
            &len as *const size_t as *const c_void,
            std::mem::size_of::<size_t>(),
        );
        close(fd);
        R_ignore_SIGPIPE.with(|v| v.set(0));
        master_fd.with(|v| v.set(-1));
        if n < 0 && *errno_ptr() != EPIPE {
            crate::main::errors::Rf_error(
                b"write error, closing pipe to the master\0".as_ptr() as *const c_char
            );
        }
    }

    if child_can_exit.with(|v| v.get()) == 0 {
        while child_can_exit.with(|v| v.get()) == 0 {
            libc::sleep(1);
        }
    }

    libc::_exit(res);
    // unreachable
    #[allow(unreachable_code)]
    R_NilValue()
}

/// Get or set the interactive flag.
#[cfg(unix)]
pub unsafe fn mc_interactive(sWhat: SEXP) -> SEXP {
    use crate::main::coerce::asInteger;

    let what = asInteger(sWhat);
    if what != NA_INTEGER {
        R_Interactive.with(|v| v.set(what));
    }
    Rf_ScalarLogical(R_Interactive.with(|v| v.get()))
}

/// Get or set CPU affinity (stub on non-Linux).
#[cfg(unix)]
pub unsafe fn mc_affinity(_req: SEXP) -> SEXP {
    // CPU affinity is Linux-specific with sched_setaffinity/sched_getaffinity.
    // On macOS/BSD, these APIs are not available. Return nil.
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Non-Unix stubs
// ---------------------------------------------------------------------------

#[cfg(not(unix))]
pub unsafe fn mc_prepare_cleanup() -> SEXP {
    R_NilValue()
}

#[cfg(not(unix))]
pub unsafe fn mc_cleanup(_sKill: SEXP, _sDetach: SEXP, _sShutdown: SEXP) -> SEXP {
    R_NilValue()
}

#[cfg(not(unix))]
pub unsafe fn mc_fork(_sEstranged: SEXP) -> SEXP {
    crate::main::errors::Rf_error(
        b"forking is not available on this platform\0".as_ptr() as *const c_char
    );
    R_NilValue()
}

#[cfg(not(unix))]
pub unsafe fn mc_close_stdout(_toNULL: SEXP) -> SEXP {
    R_NilValue()
}

#[cfg(not(unix))]
pub unsafe fn mc_close_stderr(_toNULL: SEXP) -> SEXP {
    R_NilValue()
}

#[cfg(not(unix))]
pub unsafe fn mc_close_fds(_sFDS: SEXP) -> SEXP {
    Rf_ScalarLogical(1)
}

#[cfg(not(unix))]
pub unsafe fn mc_send_master(_what: SEXP) -> SEXP {
    crate::main::errors::Rf_error(
        b"only children can send data to the master process\0".as_ptr() as *const c_char,
    );
    R_NilValue()
}

#[cfg(not(unix))]
pub unsafe fn mc_send_child_stdin(_sPid: SEXP, _what: SEXP) -> SEXP {
    crate::main::errors::Rf_error(
        b"only the master process can send data to a child process\0".as_ptr() as *const c_char,
    );
    R_NilValue()
}

#[cfg(not(unix))]
pub unsafe fn mc_select_children(_sTimeout: SEXP, _sWhich: SEXP) -> SEXP {
    R_NilValue()
}

#[cfg(not(unix))]
pub unsafe fn mc_read_child(_sPid: SEXP) -> SEXP {
    R_NilValue()
}

#[cfg(not(unix))]
pub unsafe fn mc_read_children(_sTimeout: SEXP) -> SEXP {
    R_NilValue()
}

#[cfg(not(unix))]
pub unsafe fn mc_rm_child(_sPid: SEXP) -> SEXP {
    Rf_ScalarLogical(0)
}

#[cfg(not(unix))]
pub unsafe fn mc_children() -> SEXP {
    Rf_allocVector(SEXPTYPE::INTSXP, 0)
}

#[cfg(not(unix))]
pub unsafe fn mc_fds(_sFdi: SEXP) -> SEXP {
    Rf_allocVector(SEXPTYPE::INTSXP, 0)
}

#[cfg(not(unix))]
pub unsafe fn mc_master_fd() -> SEXP {
    Rf_ScalarInteger(-1)
}

#[cfg(not(unix))]
pub unsafe fn mc_is_child() -> SEXP {
    Rf_ScalarLogical(0)
}

#[cfg(not(unix))]
pub unsafe fn mc_kill(_sPid: SEXP, _sSig: SEXP) -> SEXP {
    crate::main::errors::Rf_error(b"'mckill' failed\0".as_ptr() as *const c_char);
    R_NilValue()
}

#[cfg(not(unix))]
pub unsafe fn mc_exit(_sRes: SEXP) -> SEXP {
    crate::main::errors::Rf_error(
        b"'mcexit' can only be used in a child process\0".as_ptr() as *const c_char
    );
    R_NilValue()
}

#[cfg(not(unix))]
pub unsafe fn mc_interactive(_sWhat: SEXP) -> SEXP {
    Rf_ScalarLogical(0)
}

#[cfg(not(unix))]
pub unsafe fn mc_affinity(_req: SEXP) -> SEXP {
    R_NilValue()
}
