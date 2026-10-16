use super::*;
use crate::{
    mainutils::connections::{ConnKind, RConn, connection_table, next_connection},
    sexp::{RSession, object::SexpError},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn payload() -> Vec<u8> {
    let mut bytes = b"X\n".to_vec();
    for word in [2_i32, 0x40700, 0x20300, 19, 2, 247, 0, 1, 262153, 5] {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    bytes.extend_from_slice(b"token");
    for word in [13_i32, 1, 99] {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    bytes
}

fn raw_connection(bytes: Vec<u8>) -> Box<RConn> {
    let mut conn = RConn::new(
        "rawConnection",
        "stream fixture",
        "rb",
        ConnKind::RawConnection,
    );
    conn.isopen = true;
    conn.canread = true;
    conn.text = false;
    conn.raw_data = bytes;
    Box::new(conn)
}

fn hook_reentry(action: u8) {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let owner = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let hook = facade
        .borrow_mut()
        .as_mut()
        .unwrap()
        .eval_code_with_output_capture("function(x) { .Internal(gc(FALSE, FALSE, TRUE)); x }")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let index = next_connection();
    connection_table()[index] = Some(raw_connection(payload()));
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let closed = Rc::downgrade(&facade);
    let replacement_runtime = Rc::new(RefCell::new(None));
    let replacement = replacement_runtime.clone();
    let replacement_bytes: Vec<_> = [13_i32, 1, 77]
        .iter()
        .flat_map(|word| word.to_be_bytes())
        .collect();
    let saved_bytes = replacement_bytes.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(observed.get() + 1) != 0 {
            return;
        }
        match action {
            0 => connection_table()[index] = Some(raw_connection(replacement_bytes.clone())),
            1 => connection_table()[index] = None,
            2 => std::panic::panic_any(682_u32),
            3 => {
                drop(closed.upgrade().unwrap().borrow_mut().take());
                *replacement.borrow_mut() = Some(RSession::new_for_gc_tests());
            }
            _ => unreachable!(),
        }
    }));
    let pin = owner.pin().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
            R_unserialize_from_conn(index as c_int, hook.as_raw())
        })
    }));
    assert_eq!(
        calls.get(),
        1,
        "actual restoration closure must collect exactly once"
    );
    let error = result.expect_err("restoration reentry cannot read a closed/replaced stream");
    if action == 2 {
        assert_eq!(*error.downcast::<u32>().unwrap(), 682);
    } else {
        let message = &error
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap()
            .message;
        if action == 3 {
            assert_eq!(message, &SexpError::RootUnavailable.to_string());
        } else {
            assert_eq!(message, "original connection is no longer available");
        }
    }
    if action == 0 {
        facade.borrow().as_ref().unwrap().with_active(|| {
            let table = connection_table();
            let conn = table[index].as_ref().unwrap();
            assert_eq!(conn.raw_pos, 0, "replacement stream must never be consumed");
            assert_eq!(conn.raw_data, saved_bytes);
            assert!(conn.pushback.is_empty());
        });
    }
    if let Some(session) = replacement_runtime.borrow().as_ref() {
        session.with_active(|| assert!(connection_table().iter().skip(3).all(Option::is_none)));
    }
}

#[test]
fn connection_stream_rejects_hook_slot_reuse_without_consuming_replacement() {
    hook_reentry(0);
}
#[test]
fn connection_stream_rejects_hook_closed_stream() {
    hook_reentry(1);
}
#[test]
fn connection_stream_preserves_live_restoration_unwind() {
    hook_reentry(2);
}
#[test]
fn connection_stream_refuses_revoked_original_and_replacement_runtime() {
    hook_reentry(3);
}
