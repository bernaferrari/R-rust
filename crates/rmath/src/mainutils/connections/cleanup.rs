#![forbid(unsafe_code)]
//! Cleanup owns original runtime storage and authenticates allocation identity.
use super::{ConnectionToken, close_connection_at_in, connection_table, connection_token, r_error};
use crate::sexp::{
    object::SexpError,
    owner::{OwnerPin, OwnerToken, WeakOwner, with_runtime},
};

pub(crate) struct ConnectionCleanup {
    original: WeakOwner,
    connection: Option<ConnectionToken>,
    remove: bool,
    // Last: cleanup capabilities and tokens disappear before runtime storage.
    pin: OwnerPin,
}
impl ConnectionCleanup {
    pub(crate) fn new(owner: OwnerToken<'_>, remove: bool) -> Self {
        owner
            .require_active()
            .unwrap_or_else(|e| r_error(&e.to_string()));
        let original = owner
            .weak_owner()
            .unwrap_or_else(|| r_error("connection cleanup requires a managed runtime"));
        let pin = original.pin().unwrap_or_else(|e| r_error(&e.to_string()));
        Self {
            original,
            connection: None,
            remove,
            pin,
        }
    }
    fn require_active(&self) -> Result<(), SexpError> {
        self.pin.require_live()?;
        if crate::sexp::instance::current_instance_ptr() == Some(self.pin.as_ptr()) {
            Ok(())
        } else {
            Err(SexpError::OwnerNotActive)
        }
    }
    pub(crate) fn attach(&mut self, index: i32) {
        self.require_active()
            .unwrap_or_else(|e| r_error(&e.to_string()));
        assert!(
            self.connection.is_none(),
            "cleanup already owns a connection"
        );
        self.connection = Some(connection_token(index).unwrap_or_else(|e| r_error(&e)));
    }
    pub(crate) fn check(&self) {
        self.require_active()
            .unwrap_or_else(|e| r_error(&e.to_string()));
        if let Some(connection) = &self.connection {
            connection.require_current().unwrap_or_else(|e| r_error(&e));
        }
    }
    pub(crate) fn close(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        self.require_active()
            .unwrap_or_else(|e| r_error(&e.to_string()));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_runtime(&self.original, |access| {
                access.with_native(|owner| {
                    let index = connection
                        .require_current()
                        .map_err(|message| SexpError::EvaluationFailed { message })?;
                    close_connection_at_in(owner.as_ptr(), index);
                    if self.remove {
                        let index = connection
                            .require_current()
                            .map_err(|message| SexpError::EvaluationFailed { message })?;
                        connection_table()[index as usize] = None;
                    }
                    Ok(())
                })
            })
            .and_then(|result| result)
        }));
        self.require_active()
            .unwrap_or_else(|e| r_error(&e.to_string()));
        match outcome {
            Ok(result) => result.unwrap_or_else(|e| r_error(&e.to_string())),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}
impl Drop for ConnectionCleanup {
    fn drop(&mut self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.close()));
    }
}
