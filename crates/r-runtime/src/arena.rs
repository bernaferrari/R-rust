use alloc::alloc::{alloc, dealloc, Layout};
use core::mem;
use core::ptr;

pub struct Arena {
    layout: Layout,
    ptr: *mut u8,
    used: usize,
    capacity: usize,
}

impl Arena {
    pub fn new(capacity: usize) -> Self {
        let layout = Layout::from_size_align(capacity, 8).unwrap();
        let ptr = unsafe { alloc(layout) };
        Self {
            layout,
            ptr,
            used: 0,
            capacity,
        }
    }

    pub fn alloc<T>(&mut self, value: T) -> *mut T {
        let size = mem::size_of::<T>();
        let align = mem::align_of::<T>();

        let aligned_used = (self.used + align - 1) & !(align - 1);
        if aligned_used + size > self.capacity {
            panic!("Arena out of memory");
        }

        let ptr = unsafe { self.ptr.add(aligned_used) as *mut T };
        unsafe { ptr::write(ptr, value) };
        self.used = aligned_used + size;
        ptr
    }

    pub fn reset(&mut self) {
        self.used = 0;
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        unsafe { dealloc(self.ptr, self.layout) };
    }
}
