#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Cursor management for GraphApp.

use std::cell::Cell;
use std::ptr;

use super::types::*;

thread_local! { pub static ArrowCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }
thread_local! { pub static BlankCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }
thread_local! { pub static WatchCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }
thread_local! { pub static CaretCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }
thread_local! { pub static TextCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }
thread_local! { pub static HandCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }
thread_local! { pub static CrossCursor: Cell<cursor> = Cell::new(ptr::null_mut()); }

pub unsafe fn init_cursors() { /* TODO */
}
