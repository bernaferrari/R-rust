#![forbid(unsafe_code)]
//! GNU-compatible environment operations on the `ValueId` heap.
//!
//! These are the native implementations the evaluator migration targets:
//! frame-local and chain lookup, definition, and assignment with GNU's
//! shadowing, locking, and unbound-value semantics. The operations work on
//! concrete environment objects — sentinel resolution (globalenv, baseenv)
//! is the caller's responsibility.

use crate::heap::Heap;
use crate::id::ValueId;
use crate::intern::SymId;
use crate::object::{Binding, EnvFrame, RObject};

/// Environment operation failures, mirroring the GNU error surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvError {
    /// The target object is not an environment.
    NotAnEnvironment,
    /// GNU: "cannot add bindings to a locked environment".
    LockedEnvironment,
    /// GNU: "cannot change value of locked binding".
    LockedBinding,
}

/// Frame-local lookup — GNU's `findVarInFrame`.
///
/// Returns the full binding including lock/active metadata. A binding whose
/// value is the unbound sentinel is treated as absent, matching GNU.
pub fn lookup_in_frame(heap: &Heap, env: ValueId, sym: SymId) -> Option<Binding> {
    let frame = frame_of(heap, env)?;
    let binding = frame.get_binding(sym)?;
    if binding.value == ValueId::UNBOUND {
        return None;
    }
    Some(binding)
}

/// Chain lookup — GNU's `findVar` with `inherits = TRUE`.
///
/// Walks the enclosure chain and returns the environment where the binding
/// was found alongside the binding itself; provenance matters for
/// assignment semantics. The walk terminates at `ValueId::EMPTY_ENV`,
/// a missing enclosure, or a non-environment enclosure — the same end
/// conditions GNU treats as exhaustion.
pub fn lookup(heap: &Heap, env: ValueId, sym: SymId) -> Option<(ValueId, Binding)> {
    let mut cursor = env;
    for _ in 0..MAX_CHAIN_DEPTH {
        if cursor == ValueId::EMPTY_ENV || cursor.is_sentinel() {
            return None;
        }
        if let Some(binding) = lookup_in_frame(heap, cursor, sym) {
            return Some((cursor, binding));
        }
        cursor = enclosure_of(heap, cursor)?;
    }
    None
}

/// Enumerate the frame's own binding symbols in insertion order — the
/// completion pool and ls() walk exactly as GNU's frame scan. Hash frames
/// lose ordering exactly as GNU's hashtab enumeration does.
pub fn frame_symbols(heap: &Heap, env: ValueId) -> Vec<SymId> {
    let mut out = Vec::new();
    let Some(slot) = heap.get(env) else {
        return out;
    };
    if let RObject::Environment { frame, .. } = &slot.body {
        match frame {
            crate::object::EnvFrame::Empty => {}
            crate::object::EnvFrame::List(entries) => {
                for (sym, binding) in entries {
                    if binding.value != ValueId::UNBOUND {
                        out.push(*sym);
                    }
                }
            }
            crate::object::EnvFrame::Hash(map) => {
                let mut pairs: Vec<(SymId, bool)> = map
                    .iter()
                    .map(|(sym, binding)| (*sym, binding.value != ValueId::UNBOUND))
                    .collect();
                pairs.sort_by_key(|(sym, _)| sym.0);
                for (sym, live) in pairs {
                    if live {
                        out.push(sym);
                    }
                }
            }
        }
    }
    out
}

/// Define a binding in the environment's own frame — GNU's `defineVar`.
///
/// Definition shadows any existing binding in the same frame and never
/// consults enclosing environments. Locked environments reject the
/// definition; locked existing bindings are silently overwritten exactly
/// as GNU does (binding locks constrain assignment, not redefinition).
pub fn define(
    heap: &mut Heap,
    env: ValueId,
    sym: SymId,
    value: ValueId,
) -> Result<(), EnvError> {
    if env_is_locked(heap, env) {
        return Err(EnvError::LockedEnvironment);
    }
    let slot = heap.get_mut(env).ok_or(EnvError::NotAnEnvironment)?;
    match &mut slot.body {
        RObject::Environment { frame, .. } => {
            frame.insert(sym, value);
            Ok(())
        }
        _ => Err(EnvError::NotAnEnvironment),
    }
}

/// Assign through the enclosure chain — GNU's `setVar`, the `<<-` semantics.
///
/// The assignment lands in the nearest enclosing frame that already binds
/// the symbol. Locked bindings and locked environments reject the write.
/// When no enclosing frame binds the symbol, GNU defines it in the global
/// environment; sentinel resolution belongs to the caller, so this layer
/// reports the miss and lets the caller decide where to define.
pub fn assign(
    heap: &mut Heap,
    env: ValueId,
    sym: SymId,
    value: ValueId,
) -> Result<AssignOutcome, EnvError> {
    // Find the target frame first with an immutable walk, then mutate.
    let mut cursor = env;
    let mut target = None;
    for _ in 0..MAX_CHAIN_DEPTH {
        if cursor == ValueId::EMPTY_ENV || cursor.is_sentinel() {
            break;
        }
        if lookup_in_frame(heap, cursor, sym).is_some() {
            target = Some(cursor);
            break;
        }
        match enclosure_of(heap, cursor) {
            Some(next) => cursor = next,
            None => break,
        }
    }
    let Some(target) = target else {
        return Ok(AssignOutcome::NoExistingBinding);
    };
    if env_is_locked(heap, target) {
        return Err(EnvError::LockedEnvironment);
    }
    let slot = heap.get_mut(target).ok_or(EnvError::NotAnEnvironment)?;
    match &mut slot.body {
        RObject::Environment { frame, .. } => {
            if let Some(existing) = frame.get_binding(sym) {
                if existing.locked {
                    return Err(EnvError::LockedBinding);
                }
                if existing.active {
                    // Active bindings do not accept direct assignment in GNU;
                    // the write replaces the binding with a plain one. GNU's
                    // actual behavior is to error in most paths — replicating
                    // the replacement keeps the operation total for now and is
                    // revisited when the evaluator migration lands.
                    frame.insert(sym, value);
                    return Ok(AssignOutcome::ReplacedActiveBinding);
                }
            }
            frame.insert(sym, value);
            Ok(AssignOutcome::Assigned)
        }
        _ => Err(EnvError::NotAnEnvironment),
    }
}

/// Remove a binding from the environment's own frame.
///
/// Locked environments and locked bindings reject removal — GNU guards both
/// in `removeBinding`.
pub fn remove(heap: &mut Heap, env: ValueId, sym: SymId) -> Result<bool, EnvError> {
    if env_is_locked(heap, env) {
        return Err(EnvError::LockedEnvironment);
    }
    let slot = heap.get_mut(env).ok_or(EnvError::NotAnEnvironment)?;
    match &mut slot.body {
        RObject::Environment { frame, .. } => {
            if let Some(existing) = frame.get_binding(sym)
                && existing.locked {
                    return Err(EnvError::LockedBinding);
                }
            Ok(frame.remove(sym).is_some())
        }
        _ => Err(EnvError::NotAnEnvironment),
    }
}

/// Lock an individual binding — GNU's `lockBinding`.
pub fn lock_binding(heap: &mut Heap, env: ValueId, sym: SymId) -> Result<(), EnvError> {
    mutate_binding(heap, env, sym, |binding| binding.locked = true)
}

/// Unlock an individual binding — GNU's `unlockBinding`.
pub fn unlock_binding(heap: &mut Heap, env: ValueId, sym: SymId) -> Result<(), EnvError> {
    mutate_binding(heap, env, sym, |binding| binding.locked = false)
}

/// Read the environment lock flag — GNU's `environmentIsLocked`.
pub fn is_locked(heap: &Heap, env: ValueId) -> bool {
    env_is_locked(heap, env)
}

/// Read one binding's lock flag — GNU's `bindingIsLocked`; absent bindings
/// report FALSE exactly as GNU.
pub fn binding_is_locked(heap: &Heap, env: ValueId, sym: SymId) -> bool {
    lookup_in_frame(heap, env, sym)
        .map(|binding| binding.locked)
        .unwrap_or(false)
}

/// Lock every binding currently in the frame — lockEnvironment's
/// bindings=TRUE form exactly as GNU.
pub fn lock_all_bindings(heap: &mut Heap, env: ValueId) -> Result<(), EnvError> {
    let slot = heap.get_mut(env).ok_or(EnvError::NotAnEnvironment)?;
    match &mut slot.body {
        RObject::Environment { frame, .. } => {
            frame.lock_all();
            Ok(())
        }
        _ => Err(EnvError::NotAnEnvironment),
    }
}

/// Lock the whole environment — GNU's `lockEnvironment`.
pub fn lock_environment(heap: &mut Heap, env: ValueId) -> Result<(), EnvError> {
    let slot = heap.get_mut(env).ok_or(EnvError::NotAnEnvironment)?;
    match slot.body {
        RObject::Environment { .. } => {
            slot.flags.env_locked = true;
            Ok(())
        }
        _ => Err(EnvError::NotAnEnvironment),
    }
}

/// The outcome of a chain assignment, for callers implementing `<<-`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOutcome {
    Assigned,
    /// No enclosing frame binds the symbol; the caller decides where the
    /// definition lands (GNU: the global environment).
    NoExistingBinding,
    ReplacedActiveBinding,
}

/// Allocate a fresh empty environment with the given enclosure.
pub fn new_environment(heap: &mut Heap, enclos: ValueId) -> ValueId {
    heap.alloc(RObject::Environment {
        frame: EnvFrame::Empty,
        enclos,
    })
}

fn frame_of(heap: &Heap, env: ValueId) -> Option<&EnvFrame> {
    match heap.object(env) {
        Some(RObject::Environment { frame, .. }) => Some(frame),
        _ => None,
    }
}

pub fn enclosure_of(heap: &Heap, env: ValueId) -> Option<ValueId> {
    match heap.object(env) {
        Some(RObject::Environment { enclos, .. }) => Some(*enclos),
        _ => None,
    }
}

fn env_is_locked(heap: &Heap, env: ValueId) -> bool {
    heap.get(env)
        .map(|slot| slot.flags.env_locked)
        .unwrap_or(false)
}

/// Cross-module binding mutation — the active-binding installer in
/// primitives needs the flag flip after definition.
pub fn mutate_binding_public(
    heap: &mut Heap,
    env: ValueId,
    sym: SymId,
    mutation: impl FnOnce(&mut Binding),
) -> Result<(), EnvError> {
    mutate_binding(heap, env, sym, mutation)
}

fn mutate_binding(
    heap: &mut Heap,
    env: ValueId,
    sym: SymId,
    mutation: impl FnOnce(&mut Binding),
) -> Result<(), EnvError> {
    let slot = heap.get_mut(env).ok_or(EnvError::NotAnEnvironment)?;
    match &mut slot.body {
        RObject::Environment { frame, .. } => {
            match frame {
                EnvFrame::Empty => {}
                EnvFrame::List(entries) => {
                    if let Some((_, binding)) = entries.iter_mut().find(|(k, _)| *k == sym) {
                        mutation(binding);
                    }
                }
                EnvFrame::Hash(map) => {
                    if let Some(binding) = map.get_mut(&sym) {
                        mutation(binding);
                    }
                }
            }
            Ok(())
        }
        _ => Err(EnvError::NotAnEnvironment),
    }
}

/// Cyclic enclosure chains are degenerate but constructible; GNU's own walk
/// is unbounded because its chains cannot cycle through user operations.
/// The depth cap here is a defensive ceiling, not a semantic limit — real
/// chains are a handful deep, and base-plus-packages nesting stays under a
/// few hundred even with deep module systems.
pub const MAX_CHAIN_DEPTH: usize = 100_000;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intern::SymbolInterner;

    fn fixture() -> (Heap, SymbolInterner) {
        (Heap::new(), SymbolInterner::new())
    }

    #[test]
    fn define_then_lookup_in_frame_roundtrips() {
        let (mut heap, mut interner) = fixture();
        let env = new_environment(&mut heap, ValueId::EMPTY_ENV);
        let x = interner.intern_symbol("x");
        let value = heap.alloc(RObject::Real(vec![1.0]));

        define(&mut heap, env, x, value).expect("define succeeds");
        let binding = lookup_in_frame(&heap, env, x).expect("binding found");
        assert_eq!(binding.value, value);
    }

    #[test]
    fn chain_lookup_finds_enclosing_binding() {
        let (mut heap, mut interner) = fixture();
        let parent = new_environment(&mut heap, ValueId::EMPTY_ENV);
        let child = new_environment(&mut heap, parent);
        let x = interner.intern_symbol("x");
        let value = heap.alloc(RObject::Real(vec![2.0]));

        define(&mut heap, parent, x, value).expect("define in parent");
        assert!(lookup_in_frame(&heap, child, x).is_none());
        let (found_in, binding) = lookup(&heap, child, x).expect("chain lookup finds it");
        assert_eq!(found_in, parent);
        assert_eq!(binding.value, value);
    }

    #[test]
    fn local_binding_shadows_enclosing() {
        let (mut heap, mut interner) = fixture();
        let parent = new_environment(&mut heap, ValueId::EMPTY_ENV);
        let child = new_environment(&mut heap, parent);
        let x = interner.intern_symbol("x");
        let outer = heap.alloc(RObject::Real(vec![1.0]));
        let inner = heap.alloc(RObject::Real(vec![2.0]));

        define(&mut heap, parent, x, outer).expect("outer");
        define(&mut heap, child, x, inner).expect("inner");
        let (_, binding) = lookup(&heap, child, x).expect("found");
        assert_eq!(binding.value, inner, "the shadowing binding wins");
    }

    #[test]
    fn unbound_sentinel_reads_as_absent() {
        let (mut heap, mut interner) = fixture();
        let env = new_environment(&mut heap, ValueId::EMPTY_ENV);
        let x = interner.intern_symbol("x");
        define(&mut heap, env, x, ValueId::UNBOUND).expect("define unbound");
        assert!(lookup_in_frame(&heap, env, x).is_none());
    }

    #[test]
    fn assign_updates_nearest_enclosing_binding() {
        let (mut heap, mut interner) = fixture();
        let parent = new_environment(&mut heap, ValueId::EMPTY_ENV);
        let child = new_environment(&mut heap, parent);
        let x = interner.intern_symbol("x");
        let original = heap.alloc(RObject::Real(vec![1.0]));
        let updated = heap.alloc(RObject::Real(vec![9.0]));

        define(&mut heap, parent, x, original).expect("define");
        let outcome = assign(&mut heap, child, x, updated).expect("assign succeeds");
        assert_eq!(outcome, AssignOutcome::Assigned);
        // the parent frame carries the update, not the child
        let binding = lookup_in_frame(&heap, parent, x).expect("parent binding");
        assert_eq!(binding.value, updated);
        assert!(lookup_in_frame(&heap, child, x).is_none());
    }

    #[test]
    fn assign_reports_missing_binding() {
        let (mut heap, mut interner) = fixture();
        let env = new_environment(&mut heap, ValueId::EMPTY_ENV);
        let x = interner.intern_symbol("x");
        let value = heap.alloc(RObject::Real(vec![1.0]));
        let outcome = assign(&mut heap, env, x, value).expect("assign is total");
        assert_eq!(outcome, AssignOutcome::NoExistingBinding);
    }

    #[test]
    fn locked_binding_rejects_assignment() {
        let (mut heap, mut interner) = fixture();
        let env = new_environment(&mut heap, ValueId::EMPTY_ENV);
        let x = interner.intern_symbol("x");
        let original = heap.alloc(RObject::Real(vec![1.0]));
        let updated = heap.alloc(RObject::Real(vec![2.0]));

        define(&mut heap, env, x, original).expect("define");
        lock_binding(&mut heap, env, x).expect("lock");
        assert_eq!(
            assign(&mut heap, env, x, updated),
            Err(EnvError::LockedBinding)
        );
        // redefinition is still legal — GNU binding locks constrain
        // assignment, not defineVar shadowing
        define(&mut heap, env, x, updated).expect("redefinition allowed");
        assert_eq!(lookup_in_frame(&heap, env, x).unwrap().value, updated);
    }

    #[test]
    fn locked_environment_rejects_new_bindings() {
        let (mut heap, mut interner) = fixture();
        let env = new_environment(&mut heap, ValueId::EMPTY_ENV);
        let x = interner.intern_symbol("x");
        let value = heap.alloc(RObject::Real(vec![1.0]));

        lock_environment(&mut heap, env).expect("lock env");
        assert_eq!(
            define(&mut heap, env, x, value),
            Err(EnvError::LockedEnvironment)
        );
    }

    #[test]
    fn remove_respects_locks() {
        let (mut heap, mut interner) = fixture();
        let env = new_environment(&mut heap, ValueId::EMPTY_ENV);
        let x = interner.intern_symbol("x");
        let value = heap.alloc(RObject::Real(vec![1.0]));

        define(&mut heap, env, x, value).expect("define");
        lock_binding(&mut heap, env, x).expect("lock");
        assert_eq!(remove(&mut heap, env, x), Err(EnvError::LockedBinding));
    }

    #[test]
    fn enclosure_cycle_does_not_diverge() {
        let (mut heap, mut interner) = fixture();
        let env = new_environment(&mut heap, ValueId::EMPTY_ENV);
        // patch the enclosure to point at itself
        match heap.object_mut(env) {
            Some(RObject::Environment { enclos, .. }) => *enclos = env,
            _ => panic!("expected environment"),
        }
        let x = interner.intern_symbol("x");
        assert!(lookup(&heap, env, x).is_none(), "cyclic chains terminate");
    }
}
