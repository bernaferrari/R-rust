#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Drawing context management for GraphApp.
//!
//! Ported from context.c - internal functions for manipulating device contexts.

use std::os::raw::c_void;
use std::ptr;

use super::runtime::ContextEntry;
use super::runtime::with_graphapp_runtime;
use super::types::*;

pub fn init_contexts() {
    with_graphapp_runtime(|runtime| runtime.contexts.clear());
}

pub fn finish_contexts() {
    del_all_contexts();
}

pub unsafe fn add_context(obj: object, dc: *mut c_void, old: *mut c_void) {
    if obj.is_null() {
        return;
    }
    with_graphapp_runtime(|runtime| {
        if let Some(entry) = runtime.contexts.iter_mut().find(|entry| entry.obj == obj) {
            entry.dc = dc;
            entry.old = old;
        } else {
            runtime.contexts.push(ContextEntry { obj, dc, old });
        }
    });
}

pub unsafe fn get_context(obj: object) -> *mut c_void {
    if obj.is_null() {
        return ptr::null_mut();
    }
    with_graphapp_runtime(|runtime| {
        runtime
            .contexts
            .iter()
            .find(|entry| entry.obj == obj)
            .map_or(ptr::null_mut(), |entry| entry.dc)
    })
}

pub unsafe fn remove_context(obj: object) {
    if obj.is_null() {
        return;
    }
    with_graphapp_runtime(|runtime| {
        if let Some(entry) = runtime.contexts.iter_mut().find(|entry| entry.obj == obj) {
            entry.dc = entry.old;
            entry.old = ptr::null_mut();
        }
    });
}

pub unsafe fn del_context(obj: object) {
    if obj.is_null() {
        return;
    }
    with_graphapp_runtime(|runtime| runtime.contexts.retain(|entry| entry.obj != obj));
}

pub fn del_all_contexts() {
    with_graphapp_runtime(|runtime| runtime.contexts.clear());
}

pub unsafe fn fix_brush(dc: *mut c_void, obj: drawing, brush: *mut c_void) {
    if obj.is_null() || dc.is_null() {
        return;
    }
    unsafe {
        if get_context(obj).is_null() {
            add_context(obj, dc, brush);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_lifecycle_restores_old_context_before_delete() {
        let _session = crate::sexp::session::RSession::new();
        let obj = 1usize as object;
        let dc = 2usize as *mut c_void;
        let old = 3usize as *mut c_void;

        unsafe {
            init_contexts();
            add_context(obj, dc, old);
            assert_eq!(get_context(obj), dc);

            remove_context(obj);
            assert_eq!(get_context(obj), old);

            del_context(obj);
            assert!(get_context(obj).is_null());
        }
    }

    #[test]
    fn fix_brush_registers_missing_context_once() {
        let _session = crate::sexp::session::RSession::new();
        let obj = 4usize as object;
        let dc = 5usize as *mut c_void;
        let brush = 6usize as *mut c_void;

        unsafe {
            init_contexts();
            fix_brush(dc, obj, brush);
            assert_eq!(get_context(obj), dc);

            remove_context(obj);
            assert_eq!(get_context(obj), brush);

            finish_contexts();
            assert!(get_context(obj).is_null());
        }
    }
}
