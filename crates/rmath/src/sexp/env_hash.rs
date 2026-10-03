#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Nonowning binding-cell index over the original canonical environment frame.
//! Hits read the cell's current CAR. Structural writes invalidate membership;
//! no cached value or index entry grants graph ownership.

use super::{
    ffi::{NodeBody, SEXP, SEXPTYPE, SexprecCore},
    heap::{CheckedNode, HeapIdentity, NodeLink, ResolvedLink},
    instance,
};
use hashbrown::{HashMap, HashSet};
const PROMOTION_THRESHOLD: usize = 100;

#[derive(Default)]
pub(crate) struct BindingTables {
    tables: HashMap<NodeLink, BindingIndex>,
}
struct BindingIndex {
    frame: NodeLink,
    bindings: HashMap<NodeLink, NodeLink>,
    first_names: HashMap<Vec<u8>, NodeLink>,
    members: HashSet<NodeLink>,
    valid: bool,
}
impl BindingIndex {
    fn empty() -> Self {
        Self {
            frame: NodeLink::NULL,
            bindings: HashMap::new(),
            first_names: HashMap::new(),
            members: HashSet::new(),
            valid: false,
        }
    }
}
impl BindingTables {
    pub(crate) fn invalidate_node(&mut self, node: NodeLink, old: &SexprecCore, new: &SexprecCore) {
        let shape_changed = old.sxpinfo.type_of() != new.sxpinfo.type_of()
            || std::mem::discriminant(&old.data) != std::mem::discriminant(&new.data);
        let frame_changed = match (old.data, new.data) {
            (NodeBody::Environment(old), NodeBody::Environment(new)) => old.frame != new.frame,
            _ => shape_changed,
        };
        let chain_changed = match (old.data, new.data) {
            (NodeBody::List(old), NodeBody::List(new)) => {
                old.cdrval != new.cdrval || old.tagval != new.tagval
            }
            _ => shape_changed,
        };
        let symbol_changed = match (old.data, new.data) {
            (NodeBody::Symbol(old), NodeBody::Symbol(new)) => old.pname != new.pname,
            _ => shape_changed,
        };
        for (env, table) in &mut self.tables {
            if (frame_changed && *env == node)
                || (chain_changed && table.members.contains(&node))
                || (symbol_changed && table.bindings.contains_key(&node))
            {
                table.valid = false;
            }
        }
    }
    pub(crate) fn prune(&mut self, heap: &HeapIdentity) {
        self.tables.retain(|env, table| {
            let live = matches!(heap.resolve_link(*env), Some(ResolvedLink::Node { allocation, .. })
                if heap.node_snapshot(&allocation).is_some_and(|core| core.sxpinfo.type_of() == SEXPTYPE::ENVSXP));
            live
        });
    }
    pub(crate) fn clear(&mut self) {
        self.tables.clear();
    }
}

fn symbol_bytes(heap: &HeapIdentity, symbol: &CheckedNode) -> Option<Vec<u8>> {
    let NodeBody::Symbol(body) = heap.node_snapshot(symbol)?.data else {
        return None;
    };
    let (header, lease) = match heap.resolve_link(body.pname)? {
        ResolvedLink::Node { allocation, .. } => (
            heap.node_snapshot(&allocation)?,
            heap.payload_lease(&allocation)?,
        ),
        ResolvedLink::Singleton(lease) => (lease.snapshot(), lease.payload_lease()?),
        ResolvedLink::Null => return None,
    };
    if header.sxpinfo.type_of() != SEXPTYPE::CHARSXP || !lease.is_immutable() {
        return None;
    }
    let length = usize::try_from(header.vecsxp_length()).ok()?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).ok()?;
    // The canonical frame walker compares symbol names as C strings. Keep
    // identical first-binding semantics even for internally constructed names
    // with bytes after a NUL.
    for index in 0..length {
        let byte = lease.byte_elt(index)?;
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    Some(bytes)
}

fn build_index(heap: &HeapIdentity, frame: NodeLink) -> Option<BindingIndex> {
    let mut table = BindingIndex::empty();
    table.frame = frame;
    let mut cursor = frame;
    loop {
        let allocation = match heap.resolve_link(cursor)? {
            ResolvedLink::Null => break,
            ResolvedLink::Singleton(lease)
                if lease.snapshot().sxpinfo.type_of() == SEXPTYPE::NILSXP =>
            {
                break;
            }
            ResolvedLink::Node { allocation, .. } => allocation,
            _ => return None,
        };
        let header = heap.node_snapshot(&allocation)?;
        if header.sxpinfo.type_of() == SEXPTYPE::NILSXP {
            break;
        }
        let NodeBody::List(body) = header.data else {
            return None;
        };
        table.members.try_reserve(1).ok()?;
        if !table.members.insert(cursor) {
            return None;
        }
        if let Some(ResolvedLink::Node {
            allocation: tag, ..
        }) = heap.resolve_link(body.tagval)
        {
            let bytes = symbol_bytes(heap, &tag)?;
            table.first_names.try_reserve(1).ok()?;
            let first = *table.first_names.entry(bytes).or_insert(cursor);
            table.bindings.try_reserve(1).ok()?;
            table.bindings.entry(body.tagval).or_insert(first);
        } else if !body.tagval.is_null() {
            // A canonical nil tag has no binding name; all other malformed
            // tags leave the authoritative walker to report its usual error.
            if !matches!(heap.resolve_link(body.tagval), Some(ResolvedLink::Singleton(lease)) if lease.snapshot().sxpinfo.type_of() == SEXPTYPE::NILSXP)
            {
                return None;
            }
        }
        cursor = body.cdrval;
    }
    table.valid = true;
    Some(table)
}

/// Only a complete index over the exact current frame can establish absence.
/// Unavailable includes unindexed frames, malformed names/cells and failed
/// reservations; those cases retain the bounded authoritative frame walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BindingLookup {
    Unavailable,
    Absent,
    Cell(NodeLink),
}

fn binding_lookup(
    heap: &HeapIdentity,
    env: &CheckedNode,
    symbol: &CheckedNode,
) -> Option<BindingLookup> {
    let env_link = env.link()?;
    let symbol_link = symbol.link()?;
    let NodeBody::Environment(body) = heap.node_snapshot(env)?.data else {
        return None;
    };
    let valid = heap.with_binding_tables(|tables| {
        tables
            .tables
            .get(&env_link)
            .map(|table| table.valid && table.frame == body.frame)
    })??;
    if !valid {
        // Build outside the map borrow. Failed reservations never publish a
        // partial index; the caller falls back to the canonical frame walk.
        let built = build_index(heap, body.frame)?;
        heap.with_binding_tables(|tables| {
            *tables.tables.get_mut(&env_link)? = built;
            Some(())
        })??;
    }
    let exact = heap.with_binding_tables(|tables| {
        let table = tables.tables.get(&env_link)?;
        (table.valid && table.frame == body.frame)
            .then(|| table.bindings.get(&symbol_link).copied())
    })??;
    let cell = if let Some(cell) = exact {
        Some(cell)
    } else {
        // Distinct, noninterned symbols still select the first byte-equal
        // binding. These bounded bytes share the frame walk's NUL semantics.
        let name = symbol_bytes(heap, symbol)?;
        heap.with_binding_tables(|tables| {
            let table = tables.tables.get(&env_link)?;
            (table.valid && table.frame == body.frame)
                .then(|| table.first_names.get(name.as_slice()).copied())
        })??
    };
    let Some(cell) = cell else {
        return Some(BindingLookup::Absent);
    };
    let ResolvedLink::Node { allocation, .. } = heap.resolve_link(cell)? else {
        return None;
    };
    matches!(heap.node_snapshot(&allocation)?.data, NodeBody::List(_))
        .then_some(BindingLookup::Cell(cell))
}

pub(crate) fn hash_binding_lookup(
    env: &super::object::Sexp<'_>,
    symbol: &super::object::Sexp<'_>,
) -> BindingLookup {
    let Ok(env_node) = env.allocation() else {
        return BindingLookup::Unavailable;
    };
    let Ok(symbol_node) = symbol.allocation() else {
        return BindingLookup::Unavailable;
    };
    let heap = env_node.heap_identity();
    if !symbol_node.belongs_to(&heap) {
        return BindingLookup::Unavailable;
    }
    binding_lookup(&heap, env_node, symbol_node).unwrap_or(BindingLookup::Unavailable)
}

// These explicit translated boundaries capture only the original heap field;
// all canonical node and index operations below are safe and callback-free.
unsafe fn instance_heap(instance: *mut instance::RInstance) -> HeapIdentity {
    unsafe { (&*std::ptr::addr_of!((*instance).heap_identity)).clone() }
}
fn env_node(heap: &HeapIdentity, env: SEXP) -> Option<CheckedNode> {
    let (_, node) = super::memory::checked_projection(env)?;
    (node.belongs_to(heap) && heap.node_snapshot(&node)?.sxpinfo.type_of() == SEXPTYPE::ENVSXP)
        .then_some(node)
}

pub(crate) fn env_has_hash_table(env: SEXP) -> bool {
    instance::with_required_current_instance(|instance| unsafe {
        env_has_hash_table_in(instance, env)
    })
}
pub(crate) unsafe fn env_has_hash_table_in(instance: *mut instance::RInstance, env: SEXP) -> bool {
    let heap = unsafe { instance_heap(instance) };
    let Some(node) = env_node(&heap, env) else {
        return false;
    };
    heap.with_binding_tables(|tables| tables.tables.contains_key(&node.link().unwrap()))
        .unwrap_or(false)
}
pub(crate) fn hash_get(env: SEXP, symbol: SEXP) -> Option<SEXP> {
    instance::with_required_current_instance(|instance| unsafe {
        hash_get_in(instance, env, symbol)
    })
}
pub(crate) unsafe fn hash_get_in(
    instance: *mut instance::RInstance,
    env: SEXP,
    symbol: SEXP,
) -> Option<SEXP> {
    let heap = unsafe { instance_heap(instance) };
    let env = env_node(&heap, env)?;
    let (_, symbol) = super::memory::checked_projection(symbol)?;
    if !symbol.belongs_to(&heap) {
        return None;
    }
    let BindingLookup::Cell(cell) = binding_lookup(&heap, &env, &symbol)? else {
        return None;
    };
    let ResolvedLink::Node { allocation, .. } = heap.resolve_link(cell)? else {
        return None;
    };
    heap.projection_of_link(heap.node_snapshot(&allocation)?.data.list().carval)
}

pub(crate) fn promote_to_hash_table(env: SEXP) {
    instance::with_required_current_instance(|instance| unsafe {
        promote_to_hash_table_in(instance, env)
    });
}
pub(crate) unsafe fn promote_to_hash_table_in(instance: *mut instance::RInstance, env: SEXP) {
    let heap = unsafe { instance_heap(instance) };
    let Some(env) = env_node(&heap, env).and_then(|node| node.link()) else {
        return;
    };
    heap.with_binding_tables(|tables| {
        if !tables.tables.contains_key(&env) && tables.tables.try_reserve(1).is_ok() {
            tables.tables.insert(env, BindingIndex::empty());
        }
    });
}

/// Check whether a pairlist length exceeds the promotion threshold.
pub(crate) fn should_promote(pairlist_length: usize) -> bool {
    pairlist_length >= PROMOTION_THRESHOLD
}

/// Remove only the index belonging to this exact environment generation.
pub(crate) fn remove_env(env: SEXP) {
    instance::with_required_current_instance(|instance| unsafe { remove_env_in(instance, env) });
}
pub(crate) unsafe fn remove_env_in(instance: *mut instance::RInstance, env: SEXP) {
    let heap = unsafe { instance_heap(instance) };
    let Some(env) = env_node(&heap, env).and_then(|node| node.link()) else {
        return;
    };
    heap.with_binding_tables(|tables| {
        tables.tables.remove(&env);
    });
}

/// Remember that `env` is a hashed environment created at `size` (GNU `R_NewHashTable`).
///
/// The size lives on the session instance. Collection sweeps reclaimed keys the
/// same way as `locked_environments`, because node addresses are recycled.
pub(crate) fn mark_hashed(env: SEXP, size: i32) {
    if env.is_null() {
        return;
    }
    let size = if size <= 0 { 29 } else { size };
    instance::with_required_current_instance(|inst| unsafe {
        (*inst).env_hash_sizes.insert(env as usize, size);
    });
    promote_to_hash_table(env);
}

pub(crate) fn hashed_size(env: SEXP) -> Option<i32> {
    instance::with_required_current_instance(|inst| unsafe {
        (*inst).env_hash_sizes.get(&(env as usize)).copied()
    })
}

fn hashpjw(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &b in bytes {
        h = h.wrapping_shl(4).wrapping_add(b as u32);
        let g = h & 0xf000_0000;
        if g != 0 {
            h ^= g >> 24;
            h ^= g;
        }
    }
    h
}

pub(crate) fn gnu_chain_profile(initial: i32, names: &[Vec<u8>]) -> (i32, i32, Vec<i32>) {
    let size = if initial <= 0 { 29 } else { initial };
    replay_chains(size, names)
}

/// `1 + (int)(size * 1.2)`. The cast of `size * 1.2` equals `size` when size <= 4.
fn hash_resize_size(size: i32) -> i32 {
    let mut new_size = 1 + (size as f64 * 1.2) as i32;
    if new_size <= size {
        new_size = size.saturating_add(1);
    }
    new_size
}

fn replay_chains(mut size: i32, names: &[Vec<u8>]) -> (i32, i32, Vec<i32>) {
    let mut inserted: Vec<&[u8]> = Vec::new();
    let mut counts = vec![0i32; size as usize];
    // HASHPRI counts each new binding. Resize once when it exceeds 0.85 * size,
    // then rebuild it as the number of non-empty chains.
    let mut pri = 0i32;
    for name in names {
        if !inserted.iter().any(|have| *have == name.as_slice()) {
            let idx = (hashpjw(name) % size as u32) as usize;
            pri += 1;
            counts[idx] += 1;
            inserted.push(name.as_slice());
        }
        if (pri as f64) > (size as f64) * 0.85 {
            size = hash_resize_size(size);
            let (next_counts, next_pri) = place(size, &inserted);
            counts = next_counts;
            pri = next_pri;
        }
    }
    (size, pri, counts)
}

fn place(size: i32, names: &[&[u8]]) -> (Vec<i32>, i32) {
    let mut counts = vec![0i32; size as usize];
    let mut pri = 0i32;
    for name in names {
        let idx = (hashpjw(name) % size as u32) as usize;
        if counts[idx] == 0 {
            pri += 1;
        }
        counts[idx] += 1;
    }
    (counts, pri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{
        object::{SessionNodeFactory, Sexp, SexpMut},
        session::RSession,
    };

    fn symbol<'s>(factory: &SessionNodeFactory<'s>, name: &str) -> Sexp<'s> {
        let name = std::ffi::CString::new(name).unwrap();
        factory
            .wrap(unsafe { crate::sexp::symbol::Rf_install(name.as_ptr()) })
            .unwrap()
    }
    fn integer<'s>(factory: &SessionNodeFactory<'s>, value: i32) -> Sexp<'s> {
        let value_node = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
            .unwrap();
        let mut value_node = SexpMut::try_from_checked(value_node).unwrap();
        value_node.try_set_integer_elt(0, value).unwrap();
        value_node.freeze()
    }
    fn environment<'s>(factory: &SessionNodeFactory<'s>, frame: &Sexp<'s>) -> Sexp<'s> {
        let env = factory
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::ENVSXP)))
            .unwrap();
        unsafe {
            crate::sexp::accessors::SET_FRAME(env.as_raw(), frame.as_raw());
        }
        promote_to_hash_table(env.as_raw());
        env
    }
    fn lookup<'s>(env: &Sexp<'s>, key: &Sexp<'s>) -> Option<Sexp<'s>> {
        unsafe { crate::sexp::envir::find_var_in_frame_result(env.clone(), key.clone()) }.unwrap()
    }

    #[test]
    fn owned_binding_index_reads_current_cells_and_invalidates_detached_live_members() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let first_name = symbol(&factory, "index_first");
        let second_name = symbol(&factory, "index_second");
        let first_value = integer(&factory, 11);
        let second_value = integer(&factory, 22);
        let second = factory
            .pairlist_cell(&second_value, &factory.nil(), &second_name)
            .unwrap();
        let first = factory
            .pairlist_cell(&first_value, &second, &first_name)
            .unwrap();
        let env = environment(&factory, &first);
        assert_eq!(lookup(&env, &second_name).unwrap().integer_elt(0), Some(22));
        let updated = integer(&factory, 33);
        unsafe {
            crate::sexp::accessors::SETCAR(second.as_raw(), updated.as_raw());
        }
        assert_eq!(lookup(&env, &second_name).unwrap().integer_elt(0), Some(33));
        unsafe {
            crate::sexp::accessors::SETCDR(first.as_raw(), factory.nil().as_raw());
        }
        assert!(second.is_live());
        assert!(lookup(&env, &second_name).is_none());
        unsafe {
            crate::sexp::accessors::SETTAG(first.as_raw(), second_name.as_raw());
        }
        assert!(lookup(&env, &first_name).is_none());
        assert_eq!(lookup(&env, &second_name).unwrap().integer_elt(0), Some(11));
        unsafe {
            crate::sexp::accessors::SET_FRAME(env.as_raw(), second.as_raw());
        }
        assert_eq!(lookup(&env, &second_name).unwrap().integer_elt(0), Some(33));
        unsafe {
            crate::sexp::envir::remove_binding_raw(env.as_raw(), second_name.as_raw());
        }
        assert!(lookup(&env, &second_name).is_none());
        unsafe {
            crate::sexp::envir::defineVar(second_name.as_raw(), first_value.as_raw(), env.as_raw());
        }
        assert_eq!(lookup(&env, &second_name).unwrap().integer_elt(0), Some(11));
    }

    #[test]
    fn owned_binding_index_preserves_first_byte_equal_tag_and_noninterned_lookup() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let interned = symbol(&factory, "same_index_name");
        let alias = factory
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::SYMSXP)))
            .unwrap();
        let lookup_alias = factory
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::SYMSXP)))
            .unwrap();
        let name = interned.try_printname().unwrap();
        unsafe {
            crate::sexp::accessors::SET_PRINTNAME(alias.as_raw(), name.as_raw());
            crate::sexp::accessors::SET_PRINTNAME(lookup_alias.as_raw(), name.as_raw());
        }
        let later = factory
            .pairlist_cell(&integer(&factory, 99), &factory.nil(), &interned)
            .unwrap();
        let first = factory
            .pairlist_cell(&integer(&factory, 11), &later, &alias)
            .unwrap();
        let env = environment(&factory, &first);
        for key in [&alias, &interned, &lookup_alias] {
            assert!(matches!(
                hash_binding_lookup(&env, key),
                BindingLookup::Cell(_)
            ));
            assert_eq!(lookup(&env, key).unwrap().integer_elt(0), Some(11));
        }
        let renamed = symbol(&factory, "renamed_index_name")
            .try_printname()
            .unwrap();
        unsafe {
            crate::sexp::accessors::SET_PRINTNAME(alias.as_raw(), renamed.as_raw());
        }
        assert_eq!(lookup(&env, &interned).unwrap().integer_elt(0), Some(99));
    }

    #[test]
    fn owned_binding_index_preserves_first_nul_terminated_name_match() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let make_symbol = |bytes: &[u8]| {
            let name = factory
                .allocate(|arena| Some(arena.alloc_charsxp(bytes)))
                .unwrap();
            let symbol = factory
                .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::SYMSXP)))
                .unwrap();
            unsafe {
                crate::sexp::accessors::SET_PRINTNAME(symbol.as_raw(), name.as_raw());
            }
            symbol
        };
        let first_key = make_symbol(b"a\0x");
        let later_key = make_symbol(b"a\0y");
        let lookup_key = make_symbol(b"a\0z");
        let later = factory
            .pairlist_cell(&integer(&factory, 99), &factory.nil(), &later_key)
            .unwrap();
        let first = factory
            .pairlist_cell(&integer(&factory, 11), &later, &first_key)
            .unwrap();
        let env = environment(&factory, &first);
        for key in [&first_key, &later_key, &lookup_key] {
            assert!(matches!(
                hash_binding_lookup(&env, key),
                BindingLookup::Cell(_)
            ));
            assert_eq!(lookup(&env, key).unwrap().integer_elt(0), Some(11));
        }
    }

    #[test]
    fn owned_binding_index_sealed_names_reject_mutation_and_payload_swaps_across_gc() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let key = symbol(&factory, "sealed_index_name");
        let missing = symbol(&factory, "sealed_index_missing");
        let alias = factory
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::SYMSXP)))
            .unwrap();
        let name = key.try_printname().unwrap();
        unsafe {
            crate::sexp::accessors::SET_PRINTNAME(alias.as_raw(), name.as_raw());
        }
        let cell = factory
            .pairlist_cell(&integer(&factory, 87), &factory.nil(), &key)
            .unwrap();
        let env = environment(&factory, &cell);
        assert_eq!(lookup(&env, &alias).unwrap().integer_elt(0), Some(87));
        assert_eq!(hash_binding_lookup(&env, &missing), BindingLookup::Absent);

        let allocation = name.allocation().unwrap();
        let heap = allocation.heap_identity();
        let original = heap.node_snapshot(allocation).unwrap();
        let lease = heap.payload_lease(allocation).unwrap();
        assert!(lease.is_immutable());
        assert_eq!(lease.set_byte_elt(0, b'x'), None);
        assert_eq!(lease.byte_elt(0), Some(b's'));
        let replacement = factory
            .allocate(|arena| Some(arena.alloc_charsxp(b"mutate_index_name")))
            .unwrap();
        let replacement_lease = heap
            .payload_lease(replacement.allocation().unwrap())
            .unwrap();
        let mut replacement_header = original;
        replacement_header.payload = replacement_lease.link();
        assert!(replacement_lease.matches_header(&replacement_header));
        assert_eq!(
            heap.publish_payload(allocation, original.payload, &replacement_lease),
            None
        );
        // Byte storage accepts raw cells too, but a complete copied header
        // cannot turn a published immutable name into a writable raw vector.
        let mut retyped = original;
        retyped.sxpinfo.set_type(SEXPTYPE::RAWSXP);
        assert_eq!(heap.replace_node(allocation, retyped), None);
        let after = heap.node_snapshot(allocation).unwrap();
        assert_eq!(after.sxpinfo.type_of(), SEXPTYPE::CHARSXP);
        assert_eq!(after.payload, original.payload);
        assert_eq!(after.data, original.data);
        assert!(name.try_char_eq(b"sealed_index_name").unwrap());
        session.owner_token().unwrap().full_gc().unwrap();
        assert!(matches!(
            hash_binding_lookup(&env, &alias),
            BindingLookup::Cell(_)
        ));
        assert_eq!(lookup(&env, &alias).unwrap().integer_elt(0), Some(87));
        assert_eq!(hash_binding_lookup(&env, &missing), BindingLookup::Absent);
    }

    #[test]
    fn owned_binding_index_warm_absence_tracks_insert_tag_name_frame_and_gc() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let present = symbol(&factory, "warm_present");
        let missing = symbol(&factory, "warm_missing");
        let renamed = symbol(&factory, "warm_renamed");
        let alias = factory
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::SYMSXP)))
            .unwrap();
        unsafe {
            crate::sexp::accessors::SET_PRINTNAME(
                alias.as_raw(),
                missing.try_printname().unwrap().as_raw(),
            );
        }
        let value = integer(&factory, 23);
        let cell = factory
            .pairlist_cell(&value, &factory.nil(), &present)
            .unwrap();
        let env = environment(&factory, &cell);
        for key in [&missing, &alias] {
            assert_eq!(hash_binding_lookup(&env, key), BindingLookup::Absent);
            assert!(lookup(&env, key).is_none());
        }
        // Changing a query symbol which is not a member must use its current
        // name, rather than caching an earlier negative result by symbol ID.
        unsafe {
            crate::sexp::accessors::SET_PRINTNAME(
                alias.as_raw(),
                present.try_printname().unwrap().as_raw(),
            );
        }
        assert_eq!(
            hash_binding_lookup(&env, &alias),
            BindingLookup::Cell(cell.allocation().unwrap().link().unwrap())
        );
        assert_eq!(lookup(&env, &alias).unwrap().integer_elt(0), Some(23));
        unsafe {
            crate::sexp::envir::defineVar(missing.as_raw(), value.as_raw(), env.as_raw());
        }
        assert_eq!(lookup(&env, &missing).unwrap().integer_elt(0), Some(23));
        assert!(matches!(
            hash_binding_lookup(&env, &missing),
            BindingLookup::Cell(_)
        ));
        unsafe {
            crate::sexp::envir::remove_binding_raw(env.as_raw(), missing.as_raw());
            crate::sexp::accessors::SETTAG(cell.as_raw(), alias.as_raw());
            crate::sexp::accessors::SET_PRINTNAME(
                alias.as_raw(),
                missing.try_printname().unwrap().as_raw(),
            );
        }
        assert_eq!(hash_binding_lookup(&env, &present), BindingLookup::Absent);
        assert_eq!(lookup(&env, &missing).unwrap().integer_elt(0), Some(23));
        unsafe {
            crate::sexp::accessors::SET_PRINTNAME(
                alias.as_raw(),
                renamed.try_printname().unwrap().as_raw(),
            );
        }
        assert_eq!(hash_binding_lookup(&env, &missing), BindingLookup::Absent);
        assert_eq!(lookup(&env, &renamed).unwrap().integer_elt(0), Some(23));
        unsafe {
            crate::sexp::accessors::SET_FRAME(env.as_raw(), factory.nil().as_raw());
        }
        assert_eq!(hash_binding_lookup(&env, &renamed), BindingLookup::Absent);
        let replacement = factory
            .pairlist_cell(&integer(&factory, 61), &factory.nil(), &missing)
            .unwrap();
        unsafe {
            crate::sexp::accessors::SET_FRAME(env.as_raw(), replacement.as_raw());
        }
        assert_eq!(lookup(&env, &missing).unwrap().integer_elt(0), Some(61));
        session.owner_token().unwrap().full_gc().unwrap();
        assert_eq!(hash_binding_lookup(&env, &renamed), BindingLookup::Absent);
        assert_eq!(lookup(&env, &missing).unwrap().integer_elt(0), Some(61));
    }

    #[test]
    fn owned_binding_index_shared_frames_invalidate_all_name_and_membership_results() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let first = symbol(&factory, "shared_first");
        let missing = symbol(&factory, "shared_missing");
        let later = symbol(&factory, "shared_later");
        let alias = factory
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::SYMSXP)))
            .unwrap();
        unsafe {
            crate::sexp::accessors::SET_PRINTNAME(
                alias.as_raw(),
                first.try_printname().unwrap().as_raw(),
            );
        }
        let cell = factory
            .pairlist_cell(&integer(&factory, 31), &factory.nil(), &alias)
            .unwrap();
        let left = environment(&factory, &cell);
        let right = environment(&factory, &cell);
        for env in [&left, &right] {
            assert_eq!(lookup(env, &first).unwrap().integer_elt(0), Some(31));
            assert_eq!(hash_binding_lookup(env, &missing), BindingLookup::Absent);
        }
        unsafe {
            crate::sexp::accessors::SET_PRINTNAME(
                alias.as_raw(),
                missing.try_printname().unwrap().as_raw(),
            );
        }
        for env in [&left, &right] {
            assert_eq!(hash_binding_lookup(env, &first), BindingLookup::Absent);
            assert_eq!(lookup(env, &missing).unwrap().integer_elt(0), Some(31));
        }
        let tail = factory
            .pairlist_cell(&integer(&factory, 42), &factory.nil(), &later)
            .unwrap();
        unsafe {
            crate::sexp::accessors::SETCDR(cell.as_raw(), tail.as_raw());
        }
        for env in [&left, &right] {
            assert_eq!(lookup(env, &later).unwrap().integer_elt(0), Some(42));
        }
        unsafe {
            crate::sexp::accessors::SETTAG(tail.as_raw(), first.as_raw());
            crate::sexp::accessors::SET_FRAME(left.as_raw(), tail.as_raw());
        }
        assert_eq!(hash_binding_lookup(&left, &missing), BindingLookup::Absent);
        assert_eq!(lookup(&right, &missing).unwrap().integer_elt(0), Some(31));
        session.owner_token().unwrap().full_gc().unwrap();
        for env in [&left, &right] {
            assert_eq!(hash_binding_lookup(env, &later), BindingLookup::Absent);
            assert_eq!(lookup(env, &first).unwrap().integer_elt(0), Some(42));
        }
        unsafe {
            crate::sexp::accessors::SETCDR(cell.as_raw(), factory.nil().as_raw());
        }
        // The detached tail remains owned by left; right must not keep a hit
        // merely because that exact allocation is still live elsewhere.
        assert_eq!(lookup(&left, &first).unwrap().integer_elt(0), Some(42));
        assert_eq!(hash_binding_lookup(&right, &first), BindingLookup::Absent);
    }

    #[test]
    fn owned_binding_index_rejects_cyclic_frame_without_hanging() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let key = symbol(&factory, "cyclic_key");
        let missing = symbol(&factory, "not_in_cycle");
        let cell = factory
            .pairlist_cell(&integer(&factory, 1), &factory.nil(), &key)
            .unwrap();
        let env = environment(&factory, &cell);
        assert!(lookup(&env, &key).is_some());
        unsafe {
            crate::sexp::accessors::SETCDR(cell.as_raw(), cell.as_raw());
        }
        assert_eq!(
            hash_binding_lookup(&env, &missing),
            BindingLookup::Unavailable
        );
        let error =
            unsafe { crate::sexp::envir::find_var_in_frame_result(env, missing) }.unwrap_err();
        assert!(error.contains("cyclic binding chain"), "{error}");
    }

    #[test]
    fn owned_binding_index_does_not_root_environment_or_reuse_retired_cells() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let key = symbol(&factory, "retired_index_key");
        let cell = factory
            .pairlist_cell(&integer(&factory, 77), &factory.nil(), &key)
            .unwrap();
        let env = environment(&factory, &cell);
        let allocation = env.allocation().unwrap().clone();
        let cell_allocation = cell.allocation().unwrap().clone();
        let heap = allocation.heap_identity();
        let original = allocation.link().unwrap();
        assert!(lookup(&env, &key).is_some());
        drop(cell);
        drop(env);
        session.owner_token().unwrap().full_gc().unwrap();
        assert!(!allocation.is_live());
        assert!(!cell_allocation.is_live());
        assert!(
            !heap
                .with_binding_tables(|tables| tables.tables.contains_key(&original))
                .unwrap()
        );
        let replacement = environment(&factory, &factory.nil());
        assert!(lookup(&replacement, &key).is_none());
    }

    #[test]
    fn owned_binding_indexes_target_original_instance_and_reject_foreign_heap() {
        let left = RSession::new_for_gc_tests();
        let (env, key, value) = left.with_active(|| {
            let factory = SessionNodeFactory::new(left.owner_token().unwrap());
            let key = symbol(&factory, "left_index_key");
            let value = integer(&factory, 41);
            let cell = factory.pairlist_cell(&value, &factory.nil(), &key).unwrap();
            (environment(&factory, &cell), key, value)
        });
        let right = RSession::new_for_gc_tests();
        assert!(!env_has_hash_table(env.as_raw()));
        assert_eq!(hash_get(env.as_raw(), key.as_raw()), None);
        left.with_active_in(|instance| unsafe {
            assert!(env_has_hash_table_in(instance, env.as_raw()));
            assert_eq!(
                hash_get_in(instance, env.as_raw(), key.as_raw()),
                Some(value.as_raw())
            );
        });
        right.with_active_in(|instance| unsafe {
            assert!(!env_has_hash_table_in(instance, env.as_raw()));
            assert_eq!(hash_get_in(instance, env.as_raw(), key.as_raw()), None);
        });
        left.with_active(|| {
            remove_env(env.as_raw());
            assert!(!env_has_hash_table(env.as_raw()));
        });
    }

    #[test]
    fn owned_binding_index_large_frame_preserves_hot_function_and_variable_lookup_across_gc() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let key = symbol(&factory, "hot_value");
        let value = integer(&factory, 123);
        let mut frame = factory.pairlist_cell(&value, &factory.nil(), &key).unwrap();
        for index in 0..200 {
            let tag = symbol(&factory, &format!("cold_{index}"));
            frame = factory
                .pairlist_cell(&integer(&factory, index), &frame, &tag)
                .unwrap();
        }
        let function_name = symbol(&factory, "hot_function");
        let expr = session
            .owner_token()
            .unwrap()
            .with_arena(|arena| {
                crate::eval::parser::parse("function() { gc(); 47L }", arena, factory.clone())
            })
            .unwrap()
            .unwrap();
        let function = factory
            .wrap(unsafe {
                crate::eval::eval::Rf_eval(expr.as_raw(), session.global_env().unwrap().as_raw())
            })
            .unwrap();
        frame = factory
            .pairlist_cell(&function, &frame, &function_name)
            .unwrap();
        let env = environment(&factory, &frame);
        for _ in 0..3 {
            assert_eq!(lookup(&env, &key).unwrap().integer_elt(0), Some(123));
            let selected =
                unsafe { crate::sexp::envir::find_fun_result(function_name.clone(), env.clone()) }
                    .unwrap()
                    .unwrap();
            assert_eq!(selected, function);
            session.owner_token().unwrap().full_gc().unwrap();
            assert!(env_has_hash_table(env.as_raw()));
        }
    }

    #[test]
    fn owned_binding_index_active_callback_collects_and_retries_after_replacement() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let key = symbol(&factory, "active_index_key");
        let expr = session
            .owner_token()
            .unwrap()
            .with_arena(|arena| {
                crate::eval::parser::parse("function() { gc(); 77L }", arena, factory.clone())
            })
            .unwrap()
            .unwrap();
        let handler = factory
            .wrap(unsafe {
                crate::eval::eval::Rf_eval(expr.as_raw(), session.global_env().unwrap().as_raw())
            })
            .unwrap();
        let env = environment(&factory, &factory.nil());
        unsafe {
            crate::sexp::envir::make_active_binding_raw(
                env.as_raw(),
                key.as_raw(),
                handler.as_raw(),
            );
        }
        assert_eq!(lookup(&env, &key).unwrap().integer_elt(0), Some(77));
        unsafe {
            crate::sexp::envir::remove_binding_raw(env.as_raw(), key.as_raw());
        }
        assert!(lookup(&env, &key).is_none());
        unsafe {
            crate::sexp::envir::make_active_binding_raw(
                env.as_raw(),
                key.as_raw(),
                handler.as_raw(),
            );
        }
        assert_eq!(lookup(&env, &key).unwrap().integer_elt(0), Some(77));
    }
}
