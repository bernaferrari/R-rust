#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Side table for environment hash lookups.
//!
//! Uses the active `RInstance` to provide O(1) lookups for environment bindings
//! without modifying the `#[repr(C)]` Envsxp struct or the GC's pointer-remapping logic.
//! The pairlist remains the authoritative storage; this hash table is an optimization layer.

use hashbrown::HashMap;

use super::ffi::SEXP;
use super::instance;

/// Number of bindings in a frame before auto-promotion to hash table.
const PROMOTION_THRESHOLD: usize = 100;

fn with_env_hash_tables<F, R>(f: F) -> R
where
    F: FnOnce(&mut HashMap<usize, HashMap<usize, SEXP>>) -> R,
{
    instance::with_required_current_instance(|instance| f(&mut instance.env_hash_tables))
}

/// Check if an environment has an associated hash table.
pub fn env_has_hash_table(env: SEXP) -> bool {
    with_env_hash_tables(|tables| tables.contains_key(&(env as usize)))
}

/// Look up a symbol in the environment's hash table.
///
/// Returns `None` if no hash table exists or the symbol is not found.
pub fn hash_get(env: SEXP, symbol: SEXP) -> Option<SEXP> {
    with_env_hash_tables(|tables| {
        tables
            .get(&(env as usize))?
            .get(&(symbol as usize))
            .copied()
    })
}

/// Insert a binding into the environment's hash table (if one exists).
pub fn hash_insert(env: SEXP, symbol: SEXP, value: SEXP) {
    with_env_hash_tables(|tables| {
        if let Some(ht) = tables.get_mut(&(env as usize)) {
            ht.insert(symbol as usize, value);
        }
    })
}

/// Remove a binding from the environment's hash table (if one exists).
pub fn hash_remove(env: SEXP, symbol: SEXP) {
    with_env_hash_tables(|tables| {
        if let Some(ht) = tables.get_mut(&(env as usize)) {
            ht.remove(&(symbol as usize));
        }
    })
}

/// Promote an environment to use a hash table by bulk-inserting all current bindings.
pub fn promote_to_hash_table(env: SEXP, bindings: &[(SEXP, SEXP)]) {
    with_env_hash_tables(|tables| {
        let ht = tables
            .entry(env as usize)
            .or_insert_with(|| HashMap::with_capacity(bindings.len()));
        for (sym, val) in bindings {
            ht.insert(*sym as usize, *val);
        }
    });
}

/// Check whether a pairlist length exceeds the promotion threshold.
pub fn should_promote(pairlist_length: usize) -> bool {
    pairlist_length >= PROMOTION_THRESHOLD
}

/// Remove an environment's hash table entry (for cleanup).
pub fn remove_env(env: SEXP) {
    with_env_hash_tables(|tables| {
        tables.remove(&(env as usize));
    });
}

#[cfg(test)]
mod tests {
    use crate::sexp::session::RSession;

    use super::*;

    #[test]
    fn test_session_env_hash_tables_are_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        let env = 0x1000usize as SEXP;
        let sym = 0x2000usize as SEXP;
        let left_val = 0x3000usize as SEXP;
        let right_val = 0x4000usize as SEXP;

        left.with_arena(|_| {
            promote_to_hash_table(env, &[(sym, left_val)]);
            assert!(env_has_hash_table(env));
            assert_eq!(hash_get(env, sym), Some(left_val));
        })
        .unwrap();

        right
            .with_arena(|_| {
                assert!(!env_has_hash_table(env));
                assert_eq!(hash_get(env, sym), None);
                promote_to_hash_table(env, &[(sym, right_val)]);
                assert_eq!(hash_get(env, sym), Some(right_val));
            })
            .unwrap();

        left.with_arena(|_| {
            assert_eq!(hash_get(env, sym), Some(left_val));
            remove_env(env);
            assert!(!env_has_hash_table(env));
        })
        .unwrap();
    }
}
