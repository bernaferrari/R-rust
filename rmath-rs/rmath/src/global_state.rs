use std::cell::UnsafeCell;

pub struct Global<T> {
    value: UnsafeCell<T>,
}

// SAFETY: R is single-threaded. No concurrent access is possible.
unsafe impl<T> Sync for Global<T> {}

impl<T> Global<T> {
    pub const fn new(value: T) -> Self {
        Global {
            value: UnsafeCell::new(value),
        }
    }

    pub fn get(&self) -> *mut T {
        self.value.get()
    }

    pub fn read(&self) -> T
    where
        T: Copy,
    {
        unsafe { *self.value.get() }
    }

    pub fn write(&self, val: T) {
        unsafe { *self.value.get() = val };
    }
}
