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
    instance::with_required_current_instance(|instance| with_env_hash_tables_in(instance, f))
}

fn with_env_hash_tables_in<F, R>(instance: *mut instance::RInstance, f: F) -> R
where
    F: FnOnce(&mut HashMap<usize, HashMap<usize, SEXP>>) -> R,
{
    // P1: the `&mut` field lend below is held only across strictly-local
    // map operations; hashbrown calls never reenter the interpreter.
    unsafe { f(&mut (*instance).env_hash_tables) }
}

/// Check if an environment has an associated hash table.
pub(crate) fn env_has_hash_table(env: SEXP) -> bool {
    instance::with_required_current_instance(|instance| env_has_hash_table_in(instance, env))
}

pub(crate) fn env_has_hash_table_in(instance: *mut instance::RInstance, env: SEXP) -> bool {
    with_env_hash_tables_in(instance, |tables| tables.contains_key(&(env as usize)))
}

/// Look up a symbol in the environment's hash table.
///
/// Returns `None` if no hash table exists or the symbol is not found.
pub(crate) fn hash_get(env: SEXP, symbol: SEXP) -> Option<SEXP> {
    instance::with_required_current_instance(|instance| hash_get_in(instance, env, symbol))
}

pub(crate) fn hash_get_in(
    instance: *mut instance::RInstance,
    env: SEXP,
    symbol: SEXP,
) -> Option<SEXP> {
    with_env_hash_tables_in(instance, |tables| {
        tables
            .get(&(env as usize))?
            .get(&(symbol as usize))
            .copied()
    })
}

/// Insert a binding into the environment's hash table (if one exists).
pub(crate) fn hash_insert(env: SEXP, symbol: SEXP, value: SEXP) {
    instance::with_required_current_instance(|instance| {
        hash_insert_in(instance, env, symbol, value)
    });
}

pub(crate) fn hash_insert_in(
    instance: *mut instance::RInstance,
    env: SEXP,
    symbol: SEXP,
    value: SEXP,
) {
    with_env_hash_tables_in(instance, |tables| {
        if let Some(ht) = tables.get_mut(&(env as usize)) {
            ht.insert(symbol as usize, value);
        }
    })
}

/// Remove a binding from the environment's hash table (if one exists).
pub(crate) fn hash_remove(env: SEXP, symbol: SEXP) {
    instance::with_required_current_instance(|instance| hash_remove_in(instance, env, symbol));
}

pub(crate) fn hash_remove_in(instance: *mut instance::RInstance, env: SEXP, symbol: SEXP) {
    with_env_hash_tables_in(instance, |tables| {
        if let Some(ht) = tables.get_mut(&(env as usize)) {
            ht.remove(&(symbol as usize));
        }
    })
}

/// Promote an environment to use a hash table by bulk-inserting all current bindings.
pub(crate) fn promote_to_hash_table(env: SEXP, bindings: &[(SEXP, SEXP)]) {
    instance::with_required_current_instance(|instance| {
        promote_to_hash_table_in(instance, env, bindings);
    });
}

pub(crate) fn promote_to_hash_table_in(
    instance: *mut instance::RInstance,
    env: SEXP,
    bindings: &[(SEXP, SEXP)],
) {
    with_env_hash_tables_in(instance, |tables| {
        let ht = tables
            .entry(env as usize)
            .or_insert_with(|| HashMap::with_capacity(bindings.len()));
        for (sym, val) in bindings {
            ht.insert(*sym as usize, *val);
        }
    });
}

/// Check whether a pairlist length exceeds the promotion threshold.
pub(crate) fn should_promote(pairlist_length: usize) -> bool {
    pairlist_length >= PROMOTION_THRESHOLD
}

/// Remove an environment's hash table entry (for cleanup).
pub(crate) fn remove_env(env: SEXP) {
    instance::with_required_current_instance(|instance| remove_env_in(instance, env));
}

pub(crate) fn remove_env_in(instance: *mut instance::RInstance, env: SEXP) {
    with_env_hash_tables_in(instance, |tables| {
        tables.remove(&(env as usize));
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
    promote_to_hash_table(env, &[]);
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

fn replay_chains(mut size: i32, names: &[Vec<u8>]) -> (i32, i32, Vec<i32>) {
    let mut inserted: Vec<&[u8]> = Vec::new();
    let mut counts = vec![0i32; size as usize];
    let mut pri = 0i32;
    for name in names {
        if (inserted.len() as f64) > (size as f64) * 0.85 {
            let mut new_size = 1 + (size as f64 * 1.2) as i32;
            if new_size <= size {
                new_size = size + 1;
            }
            loop {
                let (c, p) = place(new_size, &inserted);
                if (inserted.len() as f64) <= (new_size as f64) * 0.85 {
                    size = new_size;
                    counts = c;
                    pri = p;
                    break;
                }
                new_size = 1 + (new_size as f64 * 1.2) as i32;
            }
        }
        let idx = (hashpjw(name) % size as u32) as usize;
        if counts[idx] == 0 {
            pri += 1;
        }
        counts[idx] += 1;
        inserted.push(name);
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
    use std::ptr::addr_of_mut;

    use crate::sexp::instance::RInstance;
    use crate::sexp::session::RSession;

    use super::*;

    #[test]
    fn test_session_env_hash_tables_are_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        let env = 0x1000usize as SEXP;
        let sym = 0x2000usize as SEXP;
        let left_val = 0x3000usize as SEXP;
        let right_val = 0x4000usize as SEXP;

        left.with_active(|| {
            promote_to_hash_table(env, &[(sym, left_val)]);
            assert!(env_has_hash_table(env));
            assert_eq!(hash_get(env, sym), Some(left_val));
        });

        right.with_active(|| {
            assert!(!env_has_hash_table(env));
            assert_eq!(hash_get(env, sym), None);
            promote_to_hash_table(env, &[(sym, right_val)]);
            assert_eq!(hash_get(env, sym), Some(right_val));
        });

        left.with_active(|| {
            assert_eq!(hash_get(env, sym), Some(left_val));
            remove_env(env);
            assert!(!env_has_hash_table(env));
        });
    }

    #[test]
    fn test_env_hash_tables_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        let env = 0x1000usize as SEXP;
        let sym = 0x2000usize as SEXP;
        let left_val = 0x3000usize as SEXP;
        let right_val = 0x4000usize as SEXP;

        promote_to_hash_table_in(addr_of_mut!(left), env, &[(sym, left_val)]);
        assert!(env_has_hash_table_in(addr_of_mut!(left), env));
        assert!(!env_has_hash_table_in(addr_of_mut!(right), env));
        assert_eq!(hash_get_in(addr_of_mut!(left), env, sym), Some(left_val));
        assert_eq!(hash_get_in(addr_of_mut!(right), env, sym), None);

        promote_to_hash_table_in(addr_of_mut!(right), env, &[(sym, right_val)]);
        hash_insert_in(addr_of_mut!(left), env, sym, 0x5000usize as SEXP);
        assert_eq!(
            hash_get_in(addr_of_mut!(left), env, sym),
            Some(0x5000usize as SEXP)
        );
        assert_eq!(hash_get_in(addr_of_mut!(right), env, sym), Some(right_val));

        hash_remove_in(addr_of_mut!(left), env, sym);
        assert_eq!(hash_get_in(addr_of_mut!(left), env, sym), None);
        assert_eq!(hash_get_in(addr_of_mut!(right), env, sym), Some(right_val));

        remove_env_in(addr_of_mut!(right), env);
        assert!(!env_has_hash_table_in(addr_of_mut!(right), env));
    }
}
