use super::*;
use crate::sexp::{RSession, memory::ArenaBudget, object::SexpMut};
use std::{cell::Cell, rc::Rc};

fn integer_list_stream(length: i32) -> Vec<u8> {
    let mut bytes = b"X\n".to_vec();
    for word in [2, 0x40700, 0x20300, SEXPTYPE::VECSXP.as_c_int(), length] {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    for value in 0..length {
        for word in [SEXPTYPE::INTSXP.as_c_int(), 1, value] {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
    }
    bytes
}

#[test]
fn managed_decoder_collects_unreachable_nodes_before_bounded_admission() {
    let mut session = RSession::new_for_gc_tests();
    let bytes = integer_list_stream(16);
    let input = session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let input = factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t))).unwrap();
        let mut input = SexpMut::try_from_checked(input).unwrap();
        for (index, byte) in bytes.iter().copied().enumerate() {
            input.try_set_raw_elt(index as R_xlen_t, byte).unwrap();
        }
        input.freeze().into_owned().unwrap()
    });
    let active = session.with_arena(|arena| {
        for _ in 0..48 {
            assert!(!arena.alloc_node(SEXPTYPE::LISTSXP).is_null());
        }
        arena.node_count()
    }).unwrap();
    let budget = ArenaBudget::new(0, active + 8);
    session.set_arena_budget(budget);
    let collections = Rc::new(Cell::new(0));
    session.with_active(|| {
        let seen = collections.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| seen.set(seen.get() + 1)));
        let factory = session.owner_token().unwrap().node_factory();
        let output = factory.wrap(unsafe { R_unserialize(input.as_raw(), R_NilValue()) }).unwrap();
        assert_eq!(output.len(), 16);
        for index in 0..16 {
            assert_eq!(output.try_vector_elt(index).unwrap().try_integer_elt(0).unwrap(), index as i32);
        }
    });
    assert!(collections.get() > 0, "actual original-owner collection must occur before admission fails");
    assert_eq!(session.with_arena(|arena| arena.budget().max_nodes).unwrap(), budget.max_nodes);
}
