//! Binary search tree (red-black tree) implementation.
//!
//! Ported from `tsearch.c` in the GNU gettext `intl/` library (originally
//! from the GNU C Library).  Implements `tsearch`, `tfind`, `tdelete`, and
//! `twalk` for managing binary search trees with red-black balancing.

#![allow(non_snake_case, non_camel_case_types)]

use std::alloc::{self, Layout};
#[cfg(test)]
use std::cell::Cell;
use std::os::raw::{c_int, c_void};
use std::ptr;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Node in the red-black tree.
///
/// In the C implementation the first field is `key` (a `const void *`).
/// Callers expect this to be the first element, so we preserve that layout.
#[repr(C)]
struct node_t {
    key: *const c_void,
    left: *mut node_t,
    right: *mut node_t,
    red: u32, // bitfield: 1 bit, stored as u32 for alignment
}

/// Visit order for `twalk` callbacks.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VISIT {
    preorder = 0,
    postorder = 1,
    endorder = 2,
    leaf = 3,
}

/// Action function type for tree walking.
type action_fn_t = unsafe extern "C" fn(*const c_void, VISIT, c_int);

// ---------------------------------------------------------------------------
// Node layout helper
// ---------------------------------------------------------------------------

const NODE_LAYOUT: Layout = Layout::new::<node_t>();

// ---------------------------------------------------------------------------
// Internal: split / rebalance for insertion
// ---------------------------------------------------------------------------

/// Possibly split a node with two red successors, and/or fix up two red
/// edges in a row.
///
/// This is the core rebalancing routine used during insertion.
unsafe fn maybe_split_for_insert(
    rootp: *mut *mut node_t,
    parentp: *mut *mut node_t,
    gparentp: *mut *mut node_t,
    p_r: c_int,
    gp_r: c_int,
    mode: c_int,
) {
    unsafe {
        let root = *rootp;
        let rp: *mut *mut node_t = &mut (*(*rootp)).right;
        let lp: *mut *mut node_t = &mut (*(*rootp)).left;

        // See if we have to split this node (both successors red).
        if mode == 1 || (!(*rp).is_null() && !(*lp).is_null() && (**rp).red != 0 && (**lp).red != 0)
        {
            // This node becomes red, its successors black.
            (*root).red = 1;
            if !(*rp).is_null() {
                (**rp).red = 0;
            }
            if !(*lp).is_null() {
                (**lp).red = 0;
            }

            // If the parent is also red, do rotations.
            if !parentp.is_null() && (**parentp).red != 0 {
                let gp = *gparentp;
                let p = *parentp;

                if (p_r > 0) != (gp_r > 0) {
                    // Case 1: the edge types of the two red edges differ.
                    // Put the child at the top of the tree.
                    (*p).red = 1;
                    (*gp).red = 1;
                    (*root).red = 0;
                    if p_r < 0 {
                        // Child is left of parent.
                        (*p).left = *rp;
                        *rp = p;
                        (*gp).right = *lp;
                        *lp = gp;
                    } else {
                        // Child is right of parent.
                        (*p).right = *lp;
                        *lp = p;
                        (*gp).left = *rp;
                        *rp = gp;
                    }
                    *gparentp = root;
                } else {
                    // Case 2: both red edges are of the same type.
                    *gparentp = *parentp;
                    (*p).red = 0;
                    (*gp).red = 1;
                    if p_r < 0 {
                        // Left edges.
                        (*gp).left = (*p).right;
                        (*p).right = gp;
                    } else {
                        // Right edges.
                        (*gp).right = (*p).left;
                        (*p).left = gp;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// tsearch: find or insert
// ---------------------------------------------------------------------------

/// Find or insert a datum into a search tree.
///
/// If the key already exists in the tree, returns a pointer to the matching
/// node.  Otherwise, inserts a new node for the key and returns it.
/// Returns `null` if `vrootp` is null or memory allocation fails.
pub unsafe fn tsearch(
    key: *const c_void,
    vrootp: *mut c_void,
    compar: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> c_int>,
) -> *mut c_void {
    unsafe {
        if vrootp.is_null() {
            return ptr::null_mut();
        }
        let compar = match compar {
            Some(f) => f,
            None => return ptr::null_mut(),
        };

        let mut rootp = vrootp as *mut *mut node_t;
        let mut parentp: *mut *mut node_t = ptr::null_mut();
        let mut gparentp: *mut *mut node_t = ptr::null_mut();
        let mut nextp: *mut *mut node_t;
        let mut r: c_int = 0;
        let mut p_r: c_int = 0;
        let mut gp_r: c_int = 0;

        // Ensure root is black.
        if !(*rootp).is_null() {
            (**rootp).red = 0;
        }

        nextp = rootp;
        while !(*nextp).is_null() {
            let root = *rootp;
            r = compar(key, (*root).key);
            if r == 0 {
                return root as *mut c_void;
            }

            maybe_split_for_insert(rootp, parentp, gparentp, p_r, gp_r, 0);

            nextp = if r < 0 {
                &mut (*root).left
            } else {
                &mut (*root).right
            };
            if (*nextp).is_null() {
                break;
            }

            gparentp = parentp;
            parentp = rootp;
            rootp = nextp;

            gp_r = p_r;
            p_r = r;
        }

        // Allocate a new node.
        let q = alloc::alloc(NODE_LAYOUT) as *mut node_t;
        if q.is_null() {
            return ptr::null_mut();
        }

        *nextp = q;
        (*q).key = key;
        (*q).red = 1;
        (*q).left = ptr::null_mut();
        (*q).right = ptr::null_mut();

        if nextp != rootp {
            // There may be two red edges in a row now.
            maybe_split_for_insert(nextp, rootp, parentp, r, p_r, 1);
        }

        q as *mut c_void
    }
}

// ---------------------------------------------------------------------------
// tfind: find
// ---------------------------------------------------------------------------

/// Find a datum in a search tree.
///
/// Returns a pointer to the matching node, or `null` if not found.
pub unsafe fn tfind(
    key: *const c_void,
    vrootp: *const c_void,
    compar: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> c_int>,
) -> *mut c_void {
    unsafe {
        if vrootp.is_null() {
            return ptr::null_mut();
        }
        let compar = match compar {
            Some(f) => f,
            None => return ptr::null_mut(),
        };

        let mut rootp = vrootp as *const *mut node_t;

        while !(*rootp).is_null() {
            let root = *rootp;
            let r = compar(key, (*root).key);
            if r == 0 {
                return root as *mut c_void;
            }
            rootp = if r < 0 {
                &(*root).left as *const *mut node_t
            } else {
                &(*root).right as *const *mut node_t
            };
        }
        ptr::null_mut()
    }
}

// ---------------------------------------------------------------------------
// tdelete: delete
// ---------------------------------------------------------------------------

/// Delete a node with the given key from the search tree.
///
/// Returns a pointer to the parent of the deleted node, or `null` if the
/// key was not found.
pub unsafe fn tdelete(
    key: *const c_void,
    vrootp: *mut c_void,
    compar: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> c_int>,
) -> *mut c_void {
    unsafe {
        if vrootp.is_null() {
            return ptr::null_mut();
        }
        let compar = match compar {
            Some(f) => f,
            None => return ptr::null_mut(),
        };

        let mut rootp = vrootp as *mut *mut node_t;
        let mut p = *rootp;
        if p.is_null() {
            return ptr::null_mut();
        }

        let retval: *mut node_t;
        let mut r: *mut node_t;
        let mut q: *mut node_t;

        // Stack of parent pointers (O(log n) deep).
        const STACKSIZE: usize = 100;
        let mut nodestack: [*mut *mut node_t; STACKSIZE] = [ptr::null_mut(); STACKSIZE];
        let mut sp: usize = 0;

        // Search for the node to delete.
        let mut cmp = compar(key, (*p).key);
        while cmp != 0 {
            if sp == STACKSIZE {
                // Stack overflow (tree too deep) - abort like C implementation.
                std::process::abort();
            }
            nodestack[sp] = rootp;
            sp += 1;
            p = *rootp;
            rootp = if cmp < 0 {
                &mut (*p).left
            } else {
                &mut (*p).right
            };
            if (*rootp).is_null() {
                return ptr::null_mut();
            }
            cmp = compar(key, (*(*rootp)).key);
        }

        retval = *rootp;

        // Determine which node to actually unchain.
        let root = *rootp;
        r = (*root).right;
        q = (*root).left;

        let unchained = if q.is_null() || r.is_null() {
            root
        } else {
            // Find the in-order successor (smallest key larger than root).
            let mut parent = rootp;
            let mut up = &mut (*root).right as *mut *mut node_t;
            loop {
                if sp == STACKSIZE {
                    std::process::abort();
                }
                nodestack[sp] = parent;
                sp += 1;
                parent = up;
                if (*(*up)).left.is_null() {
                    break;
                }
                up = &mut (*(*up)).left;
            }
            *up
        };

        // Unchain the node: its only non-null child (if any) takes its place.
        r = (*unchained).left;
        if r.is_null() {
            r = (*unchained).right;
        }
        if sp == 0 {
            *rootp = r;
        } else {
            q = *nodestack[sp - 1];
            if unchained == (*q).right {
                (*q).right = r;
            } else {
                (*q).left = r;
            }
        }

        // If we removed a different node (successor), copy its key.
        if unchained != root {
            (*root).key = (*unchained).key;
        }

        // Rebalance if we removed a black node.
        if (*unchained).red == 0 {
            // We lost a black edge; rebalance the tree.
            while sp > 0 && (r.is_null() || (*r).red == 0) {
                let mut pp = nodestack[sp - 1];
                p = *pp;

                if r == (*p).left {
                    // R is the left child.
                    q = (*p).right;
                    if (*q).red != 0 {
                        // Q is red: rotate left.
                        (*q).red = 0;
                        (*p).red = 1;
                        (*p).right = (*q).left;
                        (*q).left = p;
                        *pp = q;
                        nodestack[sp] = pp;
                        sp += 1;
                        pp = &mut (*q).left;
                        q = (*p).right;
                    }
                    // Q is black.
                    if ((*q).left.is_null() || (*(*q).left).red == 0)
                        && ((*q).right.is_null() || (*(*q).right).red == 0)
                    {
                        // Both children of Q are black: color Q red.
                        (*q).red = 1;
                        r = p;
                    } else {
                        if (*q).right.is_null() || (*(*q).right).red == 0 {
                            // Left child of Q is red.
                            let q2 = (*q).left;
                            (*q2).red = (*p).red;
                            (*p).right = (*q2).left;
                            (*q).left = (*q2).right;
                            (*q2).right = q;
                            (*q2).left = p;
                            *pp = q2;
                            (*p).red = 0;
                        } else {
                            // Right child of Q is red.
                            (*q).red = (*p).red;
                            (*p).red = 0;
                            (*(*q).right).red = 0;
                            // Left rotate p.
                            (*p).right = (*q).left;
                            (*q).left = p;
                            *pp = q;
                        }
                        // Done.
                        sp = 1;
                        r = ptr::null_mut();
                    }
                } else {
                    // R is the right child (mirror of above).
                    q = (*p).left;
                    if (*q).red != 0 {
                        (*q).red = 0;
                        (*p).red = 1;
                        (*p).left = (*q).right;
                        (*q).right = p;
                        *pp = q;
                        nodestack[sp] = pp;
                        sp += 1;
                        pp = &mut (*q).right;
                        q = (*p).left;
                    }
                    if ((*q).right.is_null() || (*(*q).right).red == 0)
                        && ((*q).left.is_null() || (*(*q).left).red == 0)
                    {
                        (*q).red = 1;
                        r = p;
                    } else {
                        if (*q).left.is_null() || (*(*q).left).red == 0 {
                            let q2 = (*q).right;
                            (*q2).red = (*p).red;
                            (*p).left = (*q2).right;
                            (*q).right = (*q2).left;
                            (*q2).left = q;
                            (*q2).right = p;
                            *pp = q2;
                            (*p).red = 0;
                        } else {
                            (*q).red = (*p).red;
                            (*p).red = 0;
                            (*(*q).left).red = 0;
                            (*p).left = (*q).right;
                            (*q).right = p;
                            *pp = q;
                        }
                        sp = 1;
                        r = ptr::null_mut();
                    }
                }
                sp -= 1;
            }
            if !r.is_null() {
                (*r).red = 0;
            }
        }

        // Free the unchained node.
        alloc::dealloc(unchained as *mut u8, NODE_LAYOUT);

        retval as *mut c_void
    }
}

// ---------------------------------------------------------------------------
// twalk: tree walking
// ---------------------------------------------------------------------------

/// Recursive tree walker.
unsafe fn trecurse(vroot: *const c_void, action: action_fn_t, level: c_int) {
    unsafe {
        if vroot.is_null() {
            return;
        }

        let root = vroot as *const node_t;

        if (*root).left.is_null() && (*root).right.is_null() {
            action(root as *const c_void, VISIT::leaf, level);
        } else {
            action(root as *const c_void, VISIT::preorder, level);
            if !(*root).left.is_null() {
                trecurse((*root).left as *const c_void, action, level + 1);
            }
            action(root as *const c_void, VISIT::postorder, level);
            if !(*root).right.is_null() {
                trecurse((*root).right as *const c_void, action, level + 1);
            }
            action(root as *const c_void, VISIT::endorder, level);
        }
    }
}

/// Walk the nodes of a tree in depth-first, left-to-right order.
///
/// For non-leaf nodes, `action` is called three times (preorder, postorder,
/// endorder).  For leaf nodes, `action` is called once (leaf).
pub unsafe fn twalk(
    vroot: *const c_void,
    action: Option<unsafe extern "C" fn(*const c_void, VISIT, c_int)>,
) {
    unsafe {
        if vroot.is_null() {
            return;
        }
        let action = match action {
            Some(f) => f,
            None => return,
        };
        trecurse(vroot, action, 0);
    }
}

// ---------------------------------------------------------------------------
// tdestroy: destroy tree (extension beyond POSIX, from glibc)
// ---------------------------------------------------------------------------

type free_fn_t = unsafe extern "C" fn(*mut c_void);

/// Recursive helper to destroy a tree.
unsafe fn tdestroy_recurse(root: *mut node_t, freefct: free_fn_t) {
    unsafe {
        if root.is_null() {
            return;
        }
        if !(*root).left.is_null() {
            tdestroy_recurse((*root).left, freefct);
        }
        if !(*root).right.is_null() {
            tdestroy_recurse((*root).right, freefct);
        }
        freefct((*root).key as *mut c_void);
        alloc::dealloc(root as *mut u8, NODE_LAYOUT);
    }
}

/// Destroy an entire tree, calling `freefct` on each node's key and then
/// freeing the node itself.
pub unsafe fn tdestroy(vroot: *mut c_void, freefct: Option<unsafe extern "C" fn(*mut c_void)>) {
    unsafe {
        if vroot.is_null() {
            return;
        }
        let freefct = match freefct {
            Some(f) => f,
            None => return,
        };
        tdestroy_recurse(vroot as *mut node_t, freefct);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn int_compar(a: *const c_void, b: *const c_void) -> c_int {
        let ia = unsafe { *(a as *const i32) };
        let ib = unsafe { *(b as *const i32) };
        ia - ib
    }

    /// Helper to create a tree root pointer on the stack.
    fn make_root() -> *mut *mut node_t {
        let root: *mut node_t = ptr::null_mut();
        Box::into_raw(Box::new(root))
    }

    unsafe fn drop_root(p: *mut *mut node_t) {
        drop(unsafe { Box::from_raw(p) });
    }

    #[test]
    fn test_tsearch_null_rootp() {
        unsafe {
            let val: i32 = 42;
            let result = tsearch(
                &val as *const i32 as *const c_void,
                ptr::null_mut(),
                Some(int_compar),
            );
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_tsearch_insert_and_find() {
        unsafe {
            let rootp = make_root();
            let v1: Box<i32> = Box::new(10);
            let v2: Box<i32> = Box::new(20);
            let v3: Box<i32> = Box::new(5);

            // Insert three values.
            let r1 = tsearch(
                Box::into_raw(v1) as *const c_void,
                rootp as *mut c_void,
                Some(int_compar),
            );
            assert!(!r1.is_null());

            let r2 = tsearch(
                Box::into_raw(v2) as *const c_void,
                rootp as *mut c_void,
                Some(int_compar),
            );
            assert!(!r2.is_null());

            let r3 = tsearch(
                Box::into_raw(v3) as *const c_void,
                rootp as *mut c_void,
                Some(int_compar),
            );
            assert!(!r3.is_null());

            // Find existing.
            let find_val: i32 = 20;
            let found = tfind(
                &find_val as *const i32 as *const c_void,
                rootp as *const c_void,
                Some(int_compar),
            );
            assert!(!found.is_null());

            // Find non-existing.
            let miss_val: i32 = 99;
            let missed = tfind(
                &miss_val as *const i32 as *const c_void,
                rootp as *const c_void,
                Some(int_compar),
            );
            assert!(missed.is_null());

            // Cleanup.
            tdestroy(*rootp as *mut c_void, Some(free_int_key));
            drop_root(rootp);
        }
    }

    unsafe extern "C" fn free_int_key(p: *mut c_void) {
        if !p.is_null() {
            drop(unsafe { Box::from_raw(p as *mut i32) });
        }
    }

    #[test]
    fn test_tdelete() {
        unsafe {
            let rootp = make_root();

            // Insert values: 10, 20, 30. After red-black balancing,
            // the tree will have 20 as root (black), 10 and 30 as children.
            let v1: Box<i32> = Box::new(10);
            let v2: Box<i32> = Box::new(20);
            let v3: Box<i32> = Box::new(30);

            let k1 = Box::into_raw(v1) as *const c_void;
            let k2 = Box::into_raw(v2) as *const c_void;
            let k3 = Box::into_raw(v3) as *const c_void;

            tsearch(k1, rootp as *mut c_void, Some(int_compar));
            tsearch(k2, rootp as *mut c_void, Some(int_compar));
            tsearch(k3, rootp as *mut c_void, Some(int_compar));

            // Delete a leaf node (30, which is the right child).
            let del_val: i32 = 30;
            let result = tdelete(
                &del_val as *const i32 as *const c_void,
                rootp as *mut c_void,
                Some(int_compar),
            );
            // Note: tdelete of a leaf node returns the parent node.
            // The key from the deleted node (k3) is freed, so we must not
            // use it afterwards. We manually free it since tdestroy won't
            // see the deleted node.
            drop(Box::from_raw(k3 as *mut i32));
            let _ = result;

            // Delete another leaf (10, which is the left child).
            let del_val2: i32 = 10;
            tdelete(
                &del_val2 as *const i32 as *const c_void,
                rootp as *mut c_void,
                Some(int_compar),
            );
            drop(Box::from_raw(k1 as *mut i32));

            // Cleanup: only k2 (20) remains in the tree.
            tdestroy(*rootp as *mut c_void, Some(free_int_key));
            drop_root(rootp);
        }
    }

    #[test]
    fn test_twalk() {
        unsafe {
            let rootp = make_root();

            let v1: Box<i32> = Box::new(30);
            let v2: Box<i32> = Box::new(10);
            let v3: Box<i32> = Box::new(20);

            tsearch(
                Box::into_raw(v1) as *const c_void,
                rootp as *mut c_void,
                Some(int_compar),
            );
            tsearch(
                Box::into_raw(v2) as *const c_void,
                rootp as *mut c_void,
                Some(int_compar),
            );
            tsearch(
                Box::into_raw(v3) as *const c_void,
                rootp as *mut c_void,
                Some(int_compar),
            );

            // Walk the tree - just ensure it doesn't crash.
            thread_local! { static VISIT_COUNT: Cell<c_int> = Cell::new(0); }
            unsafe extern "C" fn count_visits(_node: *const c_void, _visit: VISIT, _level: c_int) {
                VISIT_COUNT.with(|v| v.set(v.get() + 1));
            }
            VISIT_COUNT.with(|v| v.set(0));
            twalk(*rootp as *const c_void, Some(count_visits));
            assert_eq!(VISIT_COUNT.with(|v| v.get()), 5);

            // Cleanup.
            tdestroy(*rootp as *mut c_void, Some(free_int_key));
            drop_root(rootp);
        }
    }

    #[test]
    fn test_tfind_null_rootp() {
        unsafe {
            let val: i32 = 42;
            let result = tfind(
                &val as *const i32 as *const c_void,
                ptr::null(),
                Some(int_compar),
            );
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_tdelete_nonexistent() {
        unsafe {
            let rootp = make_root();
            let val: i32 = 999;
            let result = tdelete(
                &val as *const i32 as *const c_void,
                rootp as *mut c_void,
                Some(int_compar),
            );
            assert!(result.is_null());
            drop_root(rootp);
        }
    }
}
