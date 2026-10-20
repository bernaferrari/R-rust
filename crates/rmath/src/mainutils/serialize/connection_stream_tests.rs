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

#[test]
fn connection_stream_owns_hook_before_collecting_connection_operand_provider() {
    use crate::sexp::{
        altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
        object::{PairlistBuilder, SexpResult},
    };
    struct IndexProvider {
        head: Rc<Cell<SEXP>>,
        index: i32,
        calls: Rc<Cell<usize>>,
    }
    impl AltrepClass for IndexProvider {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::INTSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(1)
        }
        fn element<'s>(&self, _: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
            self.calls.set(self.calls.get() + 1);
            let head = self.head.replace(ptr::null_mut());
            if !head.is_null() {
                unsafe {
                    SETCDR(head, R_NilValue());
                }
            }
            crate::sexp::gengc::full_gc();
            Ok(AltrepElement::Integer(self.index))
        }
    }
    let mut session = RSession::new_for_gc_tests();
    let hook = session
        .eval_code_with_output_capture("function(x) x")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let f = session.owner_token().unwrap().node_factory();
    let index = next_connection();
    connection_table()[index] = Some(raw_connection(payload()));
    let head: Rc<Cell<SEXP>> = Rc::new(Cell::new(ptr::null_mut()));
    let calls = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "stream.connection.index",
            IndexProvider {
                head: head.clone(),
                index: index as i32,
                calls: calls.clone(),
            },
        )
        .unwrap();
    let conn = AltrepBuilder::new(class).build().unwrap();
    let classes = f.strings(&["rawConnection", "connection"]).unwrap();
    unsafe {
        setAttrib(
            conn.as_raw(),
            crate::eval::attrib_core::R_ClassSymbol(),
            classes.as_raw(),
        );
    }
    let mut args = PairlistBuilder::from_factory(f.clone());
    args.push(conn, None).unwrap();
    args.push(hook, None).unwrap();
    let args = args.finish().unwrap();
    head.set(args.as_raw());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        do_unserializeFromConn(
            f.nil().as_raw(),
            f.nil().as_raw(),
            args.as_raw(),
            f.nil().as_raw(),
        )
    }));
    let raw = outcome.unwrap_or_else(|payload| {
        eprintln!(
            "provider-detached stream hook failed: {:?}",
            payload
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|e| &e.message)
        );
        std::panic::resume_unwind(payload)
    });
    let value = f.wrap(raw).unwrap();
    assert_eq!(calls.get(), 1);
    assert!(args.try_cdr().unwrap().is_nil());
    assert_eq!(
        value
            .try_vector_elt(0)
            .unwrap()
            .try_string_value_elt(0)
            .unwrap()
            .as_deref(),
        Some("token")
    );
    assert_eq!(
        value.try_vector_elt(1).unwrap().try_integer_elt(0).unwrap(),
        99
    );
}

#[test]
fn connection_stream_truncated_huge_collection_never_allocates_its_declared_extent() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let before = owner
        .with_arena(|arena| (arena.node_count(), arena.workspace_bytes()))
        .unwrap();
    for count in [-1_i32, i32::MAX] {
        let mut bytes = b"X\n".to_vec();
        for word in [2_i32, 0x40700, 0x20300, 19, count, 254] {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        let index = next_connection();
        connection_table()[index] = Some(raw_connection(bytes));
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            R_unserialize_from_conn(index as c_int, R_NilValue())
        }))
        .expect_err("a truncated stream cannot authenticate its declared collection");
        assert!(
            error
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("read error")
        );
        assert_eq!(
            owner
                .with_arena(|arena| (arena.node_count(), arena.workspace_bytes()))
                .unwrap(),
            before
        );
        connection_table()[index] = None;
    }
}

#[test]
fn connection_stream_staging_admission_releases_on_denial_and_recovers() {
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let baseline = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
    let allocated = owner
        .with_arena(|arena| arena.total_bytes_allocated())
        .unwrap();
    let nil = owner.node_factory().nil().into_owned().unwrap();
    let reservation = owner
        .with_arena(|arena| arena.reserve_workspace(0))
        .unwrap()
        .unwrap();
    owner
        .with_arena(|arena| arena.set_budget(ArenaBudget::new(allocated + 1, 0)))
        .unwrap();
    let mut values = super::super::stream_values::StreamValues::new(reservation);
    assert_eq!(
        values.push(nil.clone()).unwrap_err(),
        "unserialize exceeds session memory limit"
    );
    drop(values);
    assert_eq!(
        owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
        baseline
    );
    owner
        .with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))
        .unwrap();
    let reservation = owner
        .with_arena(|arena| arena.reserve_workspace(0))
        .unwrap()
        .unwrap();
    let mut values = super::super::stream_values::StreamValues::new(reservation);
    values.push(nil).unwrap();
    assert!(owner.with_arena(|arena| arena.workspace_bytes()).unwrap() > baseline);
    drop(values);
    assert_eq!(
        owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
        baseline
    );
}

#[test]
fn connection_stream_compiled_constants_keep_both_gnu_hook_cursors_through_gc() {
    let mut session = RSession::new_for_gc_tests();
    let hook = session
        .eval_code_with_output_capture("function(x) { .Internal(gc(FALSE, FALSE, TRUE)); x }")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let owner = session.owner_token().unwrap();
    let index = next_connection();
    let bytes =
        include_bytes!("../../../../r-embed/tests/fixtures/gnu-bytecode-stream-hooks-v2.rds");
    connection_table()[index] = Some(raw_connection(bytes.to_vec()));
    let observed = Rc::new(RefCell::new(Vec::new()));
    let cursors = observed.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        let position = connection_table()[index].as_ref().unwrap().raw_pos;
        cursors.borrow_mut().push(position);
    }));
    let value = unsafe { R_unserialize_from_conn(index as c_int, hook.as_raw()) };
    let value = owner.sexp(value).unwrap();
    assert_eq!(value.typeof_(), SEXPTYPE::CLOSXP);
    assert_eq!(*observed.borrow(), [1324, 3761]);
    assert_eq!(
        connection_table()[index].as_ref().unwrap().raw_pos,
        bytes.len()
    );
}
