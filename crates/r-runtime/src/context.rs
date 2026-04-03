use alloc::boxed::Box;

pub struct Context {
    jmpbuf: Option<Box<()>>,
}

impl Context {
    pub fn new() -> Self {
        Self { jmpbuf: None }
    }

    pub fn begin(&mut self) {
        // Stub implementation
    }

    pub fn end(&mut self) {
        // Stub implementation
    }
}
