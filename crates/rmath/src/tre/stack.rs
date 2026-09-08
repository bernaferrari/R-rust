/*
  tre/stack.rs - Simple stack implementation

  Ported from tre-stack.c
*/

use std::os::raw::{c_int, c_void};
use std::ptr;

use super::mem;

pub const REG_OK: c_int = 0;
pub const REG_ESPACE: c_int = 12;

#[repr(C)]
union tre_stack_item {
    voidptr_value: *mut c_void,
    int_value: c_int,
}

#[repr(C)]
pub struct tre_stack_rec {
    size: c_int,
    max_size: c_int,
    increment: c_int,
    ptr: c_int,
    stack: *mut tre_stack_item,
}

pub type tre_stack_t = *mut tre_stack_rec;

pub unsafe fn tre_stack_new(size: c_int, max_size: c_int, increment: c_int) -> tre_stack_t {
    unsafe {
        let s: tre_stack_t = mem::xmalloc(std::mem::size_of::<tre_stack_rec>()) as tre_stack_t;
        if s.is_null() {
            return ptr::null_mut();
        }
        (*s).stack = mem::xmalloc(std::mem::size_of::<tre_stack_item>() * size as usize)
            as *mut tre_stack_item;
        if (*s).stack.is_null() {
            mem::xfree(s as *mut c_void);
            return ptr::null_mut();
        }
        (*s).size = size;
        (*s).max_size = max_size;
        (*s).increment = increment;
        (*s).ptr = 0;
        s
    }
}

pub unsafe fn tre_stack_destroy(s: tre_stack_t) {
    unsafe {
        if s.is_null() {
            return;
        }
        if !(*s).stack.is_null() {
            mem::xfree((*s).stack as *mut c_void);
        }
        mem::xfree(s as *mut c_void);
    }
}

pub unsafe fn tre_stack_num_objects(s: tre_stack_t) -> c_int {
    unsafe {
        if s.is_null() {
            return 0;
        }
        (*s).ptr
    }
}

unsafe fn tre_stack_push(s: tre_stack_t, value: tre_stack_item) -> c_int {
    unsafe {
        if (*s).ptr < (*s).size {
            *((*s).stack.add((*s).ptr as usize)) = value;
            (*s).ptr += 1;
        } else {
            if (*s).size >= (*s).max_size {
                return REG_ESPACE;
            } else {
                let new_size = if (*s).size + (*s).increment > (*s).max_size {
                    (*s).max_size
                } else {
                    (*s).size + (*s).increment
                };
                let new_buffer = mem::xrealloc(
                    (*s).stack as *mut c_void,
                    std::mem::size_of::<tre_stack_item>() * new_size as usize,
                ) as *mut tre_stack_item;
                if new_buffer.is_null() {
                    return REG_ESPACE;
                }
                (*s).size = new_size;
                (*s).stack = new_buffer;
                tre_stack_push(s, value);
            }
        }
        REG_OK
    }
}

pub unsafe fn tre_stack_push_voidptr(s: tre_stack_t, value: *mut c_void) -> c_int {
    unsafe {
        let mut item = std::mem::zeroed::<tre_stack_item>();
        item.voidptr_value = value;
        tre_stack_push(s, item)
    }
}

pub unsafe fn tre_stack_push_int(s: tre_stack_t, value: c_int) -> c_int {
    unsafe {
        let mut item = std::mem::zeroed::<tre_stack_item>();
        item.int_value = value;
        tre_stack_push(s, item)
    }
}

pub unsafe fn tre_stack_pop_voidptr(s: tre_stack_t) -> *mut c_void {
    unsafe {
        (*s).ptr -= 1;
        (*((*s).stack.add((*s).ptr as usize))).voidptr_value
    }
}

pub unsafe fn tre_stack_pop_int(s: tre_stack_t) -> c_int {
    unsafe {
        (*s).ptr -= 1;
        (*((*s).stack.add((*s).ptr as usize))).int_value
    }
}
