#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Font management for GraphApp.
//!
//! Ported from fonts.c - font creation and measurement.

use std::cell::Cell;
use std::os::raw::c_int;
use std::ptr;

use super::objects;
use super::strings;
use super::types::*;

thread_local! { pub static FixedFont: Cell<font> = Cell::new(ptr::null_mut()); }
thread_local! { pub static SystemFont: Cell<font> = Cell::new(ptr::null_mut()); }
thread_local! { pub static Times: Cell<font> = Cell::new(ptr::null_mut()); }
thread_local! { pub static Helvetica: Cell<font> = Cell::new(ptr::null_mut()); }
thread_local! { pub static Courier: Cell<font> = Cell::new(ptr::null_mut()); }

unsafe fn alloc_font(name: &str, style: c_int, size: c_int) -> font {
    let obj = unsafe { objects::new_object(FontObject, ptr::null_mut(), ptr::null_mut()) };
    if obj.is_null() {
        return ptr::null_mut();
    }

    let mut bytes = Vec::with_capacity(name.len() + 1);
    bytes.extend_from_slice(name.as_bytes());
    bytes.push(0);

    unsafe {
        (*obj).text = strings::new_string(bytes.as_ptr() as *const libc::c_char);
        (*obj).state = style as _;
        (*obj).value = size;
        (*obj).size = size;
        (*obj).xsize = (size / 2).max(1);
        (*obj).max = (size * 4) / 5;
    }
    obj
}

fn ensure_font(slot: &Cell<font>, name: &str, style: c_int, size: c_int) {
    if slot.get().is_null() {
        slot.set(unsafe { alloc_font(name, style, size) });
    }
}

pub fn init_fonts() {
    FixedFont.with(|slot| ensure_font(slot, "Fixed", FixedWidth, 10));
    SystemFont.with(|slot| ensure_font(slot, "System", Plain, 10));
    Times.with(|slot| ensure_font(slot, "Times", Plain, 12));
    Helvetica.with(|slot| ensure_font(slot, "Helvetica", SansSerif, 12));
    Courier.with(|slot| ensure_font(slot, "Courier", FixedWidth, 10));
}

pub unsafe fn getSysFontSize() -> c_int {
    10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_fonts_populates_default_handles() {
        unsafe {
            objects::init_objects();
        }
        init_fonts();
        SystemFont.with(|slot| assert!(!slot.get().is_null()));
        FixedFont.with(|slot| assert!(!slot.get().is_null()));
    }
}
