/// Stub session implementation - full GC and session management pending
pub struct Session;

impl Session {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
