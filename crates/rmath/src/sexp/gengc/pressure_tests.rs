use super::*;
use crate::sexp::{memory::ArenaBudget, session::RSession};

#[test]
fn pressure_safe_point_collects_old_garbage_and_keeps_the_owned_value() {
    let mut session = RSession::new_for_gc_tests();
    let (nodes, mut roots) = session
        .with_arena(|arena| {
            let nodes: Vec<_> = (0..10)
                .map(|_| {
                    let value = arena.alloc_node(SEXPTYPE::LISTSXP);
                    arena.node_token(value).unwrap()
                })
                .collect();
            let roots: Vec<_> = nodes
                .iter()
                .map(|node| node.root_lease().unwrap())
                .collect();
            arena.set_budget(ArenaBudget::new(usize::MAX, arena.node_count()));
            (nodes, roots)
        })
        .unwrap();
    session.with_active(|| {
        minor_gc();
    });
    assert!(nodes.iter().all(|node| node.is_live()));
    let retained = roots.pop().unwrap();
    drop(roots);
    session.with_active(|| {
        instance::with_required_current_instance(|owner| unsafe {
            (*owner).gc_state.gc_pending = true;
        });
        maybe_collect_at_eval_safe_point();
    });
    assert!(nodes[..9].iter().all(|node| !node.is_live()));
    assert!(nodes[9].is_live());
    drop(retained);
}
