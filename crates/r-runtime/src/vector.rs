/// Stub vector implementation
pub struct Vector<T> {
    _marker: core::marker::PhantomData<T>,
}

impl<T> Vector<T> {
    pub fn new() -> Self {
        Self {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<T> Default for Vector<T> {
    fn default() -> Self {
        Self::new()
    }
}
