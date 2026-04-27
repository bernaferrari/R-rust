#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Cursor management for GraphApp.

use std::ptr;

use super::objects;
use super::runtime::with_graphapp_runtime;
use super::types::*;

unsafe fn alloc_cursor() -> cursor {
    unsafe { objects::new_object(CursorObject, ptr::null_mut(), ptr::null_mut()) }
}

macro_rules! ensure_cursor {
    ($field:ident) => {
        if with_graphapp_runtime(|runtime| runtime.cursors.$field.is_null()) {
            let cursor = unsafe { alloc_cursor() };
            with_graphapp_runtime(|runtime| {
                if runtime.cursors.$field.is_null() {
                    runtime.cursors.$field = cursor;
                }
            });
        }
    };
}

pub fn init_cursors() {
    ensure_cursor!(arrow);
    ensure_cursor!(blank);
    ensure_cursor!(watch);
    ensure_cursor!(caret);
    ensure_cursor!(text);
    ensure_cursor!(hand);
    ensure_cursor!(cross);
}
