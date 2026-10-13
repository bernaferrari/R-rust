//! bytecode test helpers — extracted verbatim from eval/bytecode.rs

use super::*;
use crate::sexp::session::RSession;

pub(crate) fn private_code(
    session: &RSession,
    words: &[c_int],
    constants: &[Sexp<'_>],
) -> Sexp<'static> {
    session.with_active(|| {
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        crate::sexp::owner::with_runtime(&owner, |access| {
            let domain = access.domain();
            let allocator = access.allocator(&domain).unwrap();
            let pool = allocator
                .allocate(|arena| {
                    let ptr = arena.alloc_vector(SEXPTYPE::VECSXP, constants.len() as i64);
                    for (i, value) in constants.iter().enumerate() {
                        arena.set_reference_element(ptr, i, value.as_raw())?;
                    }
                    Some(ptr)
                })
                .unwrap();
            allocator
                .allocate(|arena| {
                    let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, words.len() as i64);
                    let heap = arena.heap_identity();
                    let node = arena.node_token(ptr)?;
                    let payload = heap.payload_lease(&node)?;
                    for (i, word) in words.iter().enumerate() {
                        payload.set_integer_elt(i, *word)?;
                    }
                    let mut header = heap.node_snapshot(&node)?;
                    header.attrib = domain.link(&pool).ok()?;
                    heap.replace_node(&node, header)?;
                    Some(ptr)
                })
                .unwrap()
                .into_owned()
                .unwrap()
        })
        .unwrap()
    })
}
pub(crate) fn run_private(
    session: &RSession,
    words: &[c_int],
    constants: &[Sexp<'_>],
) -> Result<Sexp<'static>, String> {
    let code = private_code(session, words, constants);
    session.with_active(|| eval_bytecode(code, session.global_env().unwrap().into_owned().unwrap()))
}

pub(crate) fn fixture_integer(session: &RSession, value: c_int) -> Sexp<'static> {
    session.with_active(|| {
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        crate::sexp::owner::with_runtime(&owner, |access| {
            let domain = access.domain();
            access
                .allocator(&domain)
                .unwrap()
                .allocate(|arena| {
                    let pointer = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
                    let heap = arena.heap_identity();
                    let node = arena.node_token(pointer)?;
                    heap.payload_lease(&node)?.set_integer_elt(0, value)?;
                    Some(pointer)
                })
                .unwrap()
        })
        .unwrap()
    })
}
pub(crate) fn collect_and_detach_pool(
    session: &RSession,
    code: &Sexp<'static>,
) -> std::rc::Rc<std::cell::Cell<usize>> {
    let callbacks = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = callbacks.clone();
    let code = code.clone();
    session.with_active(|| {
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            // Remove the only graph edge to the original constants. The
            // executor's actual snapshots and argument cells must own them.
            unsafe {
                crate::sexp::accessors::SET_ATTRIB(
                    code.as_raw(),
                    crate::sexp::globals::R_NilValue(),
                );
            }
            crate::sexp::gengc::full_gc();
            observed.set(observed.get() + 1);
        }));
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
    });
    callbacks
}
