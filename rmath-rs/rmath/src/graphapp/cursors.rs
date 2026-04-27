#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Cursor management for GraphApp.

use std::ptr;

use super::objects;
use super::runtime::{CursorState, with_graphapp_runtime};
use super::types::*;

unsafe fn alloc_cursor() -> cursor {
    unsafe { objects::new_object(CursorObject, ptr::null_mut(), ptr::null_mut()) }
}

fn ensure_cursor(slot: &mut cursor) {
    if slot.is_null() {
        *slot = unsafe { alloc_cursor() };
    }
}

pub fn init_cursors() {
    with_graphapp_runtime(|runtime| {
        let CursorState {
            arrow,
            blank,
            watch,
            caret,
            text,
            hand,
            cross,
        } = &mut runtime.cursors;
        ensure_cursor(arrow);
        ensure_cursor(blank);
        ensure_cursor(watch);
        ensure_cursor(caret);
        ensure_cursor(text);
        ensure_cursor(hand);
        ensure_cursor(cross);
    });
}
