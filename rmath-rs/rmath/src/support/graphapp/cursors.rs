#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Cursor management for GraphApp.

use std::cell::Cell;
use std::ptr;

use super::objects;
use super::types::*;

thread_local! { pub static ArrowCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }
thread_local! { pub static BlankCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }
thread_local! { pub static WatchCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }
thread_local! { pub static CaretCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }
thread_local! { pub static TextCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }
thread_local! { pub static HandCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }
thread_local! { pub static CrossCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }

unsafe fn alloc_cursor() -> cursor {
    unsafe { objects::new_object(CursorObject, ptr::null_mut(), ptr::null_mut()) }
}

fn ensure_cursor(slot: &Cell<cursor>) {
    if slot.get().is_null() {
        slot.set(unsafe { alloc_cursor() });
    }
}

pub fn init_cursors() {
    ArrowCursor.with(ensure_cursor);
    BlankCursor.with(ensure_cursor);
    WatchCursor.with(ensure_cursor);
    CaretCursor.with(ensure_cursor);
    TextCursor.with(ensure_cursor);
    HandCursor.with(ensure_cursor);
    CrossCursor.with(ensure_cursor);
}
