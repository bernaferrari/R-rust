use alloc::string::String;
use core::fmt;

#[derive(Debug, Clone)]
pub struct RError {
    message: String,
}

impl RError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for RError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

pub type RResult<T> = Result<T, RError>;
