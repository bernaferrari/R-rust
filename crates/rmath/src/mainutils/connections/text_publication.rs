//! Publish owned text buffers without connection-table or text-field loans.
use super::text_output::TextOutput;
use super::*;
use crate::sexp::{
    Sexp,
    object::SexpMut,
    owner::{OwnerPin, OwnerToken},
};
use std::cell::RefCell;
use std::rc::Rc;

pub(super) const BUSY: &str = "text connection is busy publishing output";

fn check(owner: OwnerToken<'_>, connection: Option<&ConnectionToken>) {
    owner
        .require_active()
        .unwrap_or_else(|error| r_error(&error.to_string()));
    if let Some(connection) = connection {
        connection
            .require_current()
            .unwrap_or_else(|error| r_error(&error));
    }
}
fn call<T>(
    owner: OwnerToken<'_>,
    connection: Option<&ConnectionToken>,
    f: impl FnOnce() -> T,
) -> T {
    check(owner, connection);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    check(owner, connection);
    match result {
        Ok(value) => value,
        Err(error) => std::panic::resume_unwind(error),
    }
}

/// The original state moves out of the connection. The identity and pin make
/// cleanup independent of callback removal, slot reuse or facade revocation.
pub(super) struct TextWriter<'owner> {
    owner: OwnerToken<'owner>,
    connection: ConnectionToken,
    data: Option<TextOutput>,
    table: Rc<RefCell<ConnectionTable>>,
    _pin: Option<OwnerPin>,
}
impl<'owner> TextWriter<'owner> {
    pub(super) fn take(owner: OwnerToken<'owner>, index: c_int) -> Option<Self> {
        let pin = owner.pin().unwrap_or_else(|e| r_error(&e.to_string()));
        check(owner, None);
        let connection = connection_token(index).unwrap_or_else(|e| r_error(&e));
        let table = connection_table();
        let data = {
            let mut entries = table.borrow_mut();
            let conn = entries[index as usize].as_mut().unwrap();
            if !matches!(conn.kind, ConnKind::TextConnection) {
                return None;
            }
            if conn.text_busy {
                r_error(BUSY);
            }
            let Some(data) = conn.text_output.take() else {
                r_error("not an output text connection");
            };
            conn.text_busy = true;
            data
        };
        Some(Self {
            owner,
            connection,
            data: Some(data),
            table,
            _pin: pin,
        })
    }
    pub(super) fn append(&mut self, bytes: &[u8]) {
        if self.data.as_mut().unwrap().append(bytes) {
            self.publish(false);
        }
    }
    pub(super) fn publish(&mut self, include_partial: bool) {
        publish(
            self.owner,
            Some(&self.connection),
            self.data.as_mut().unwrap(),
            include_partial,
        );
    }
    pub(super) fn close(&mut self) {
        self.publish(true);
        let data = self.data.as_mut().unwrap();
        data.incomplete = false;
        data.environment = None;
    }
    pub(super) fn value(&self) -> Sexp<'owner> {
        let factory = self.owner.node_factory();
        let data = self.data.as_ref().unwrap();
        build(
            self.owner,
            Some(&self.connection),
            &data.lines[..data.complete_len(false)],
            0,
            None,
            &factory,
        )
    }
}
impl Drop for TextWriter<'_> {
    fn drop(&mut self) {
        // No callbacks: the operation pin still retains this physical runtime.
        // Restore only the exact original connection, never a reused slot.
        let mut table = self.table.borrow_mut();
        if let Some(conn) = table
            .get_mut(self.connection.index as usize)
            .and_then(Option::as_mut)
            && self.connection.matches(conn)
        {
            debug_assert!(conn.text_busy && conn.text_output.is_none());
            conn.text_output = self.data.take();
            conn.text_busy = false;
        }
    }
}

pub(super) fn publish(
    owner: OwnerToken<'_>,
    connection: Option<&ConnectionToken>,
    data: &mut TextOutput,
    include_partial: bool,
) {
    let _pin = owner.pin().unwrap_or_else(|e| r_error(&e.to_string()));
    check(owner, connection);
    let Some(name) = data.name.as_deref() else {
        return;
    };
    let Some(environment) = data.environment.as_ref() else {
        return;
    };
    if name.is_empty() || name.contains('\0') {
        return;
    }
    let factory = owner.node_factory();
    let symbol = call(owner, connection, || crate::sexp::symbol::install_str(name));
    let old = call(owner, connection, || {
        crate::sexp::envir::R_findVarInFrame(environment.as_raw(), symbol)
    });
    let old = factory
        .wrap(old)
        .and_then(Sexp::into_owned)
        .unwrap_or_else(|e| r_error(&e.to_string()));
    let complete = data.complete_len(include_partial);
    let reuse = old.typeof_() == SEXPTYPE::STRSXP
        && usize::try_from(old.len()).ok() == Some(data.published)
        && data.published <= complete;
    let answer = build(
        owner,
        connection,
        &data.lines[..complete],
        if reuse { data.published } else { 0 },
        reuse.then_some(&old),
        &factory,
    );
    call(owner, connection, || {
        crate::sexp::envir::defineVar(symbol, answer.as_raw(), environment.as_raw())
    });
    data.published = complete;
}

fn build<'owner>(
    owner: OwnerToken<'owner>,
    connection: Option<&ConnectionToken>,
    lines: &[String],
    start: usize,
    old: Option<&Sexp<'_>>,
    factory: &crate::sexp::object::SessionNodeFactory<'owner>,
) -> Sexp<'owner> {
    let length = R_xlen_t::try_from(lines.len())
        .unwrap_or_else(|_| r_error("too many text connection lines"));
    let answer = if let Some(old) = old {
        let raw = call(owner, connection, || {
            crate::mainutils::builtin::xlengthgets(old.as_raw(), length)
        });
        factory
            .wrap(raw)
            .unwrap_or_else(|e| r_error(&e.to_string()))
    } else {
        call(owner, connection, || {
            factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, length)))
        })
        .unwrap_or_else(|e| r_error(&e.to_string()))
    };
    let mut answer = SexpMut::try_from_checked(answer).unwrap_or_else(|e| r_error(&e.to_string()));
    for (i, line) in lines.iter().enumerate().skip(start) {
        // CString::new rejected interior NUL; keep the previous ""
        // substitution now that the factory copies the text directly.
        let text = if line.contains('\0') {
            ""
        } else {
            line.as_str()
        };
        let character = call(owner, connection, || factory.character(text))
            .unwrap_or_else(|e| r_error(&e.to_string()));
        answer
            .try_set_string_elt(i as R_xlen_t, character)
            .unwrap_or_else(|e| r_error(&e.to_string()));
    }
    answer.freeze()
}
