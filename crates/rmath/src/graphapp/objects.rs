#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Object management for GraphApp.
//!
//! Ported from objects.c - maintains internal info about graphical objects
//! using a linked-list hierarchy with reference counting.

use std::os::raw::{c_int, c_void};
use std::ptr;

use super::memory;
use super::runtime::{DelNode, with_graphapp_runtime};
use super::types::*;

/// Initialise the base object and set the list to be empty.
pub fn init_objects() {
    if !with_graphapp_runtime(|runtime| runtime.objects.base_object).is_null() {
        return;
    }
    let obj = memory::memalloc_bytes(std::mem::size_of::<ObjInfo>()) as object;
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let Some(base) = (unsafe { obj_mut(obj) }) else {
        return;
    };
    *base = ObjInfo {
        kind: BaseObject,
        next: obj,
        prev: obj,
        ..Default::default()
    };
    with_graphapp_runtime(|runtime| runtime.objects.base_object = obj);
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn add_object(obj: object, parent: object) {
    let parent = if parent.is_null() {
        with_graphapp_runtime(|runtime| runtime.objects.base_object)
    } else {
        parent
    };

    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let child = unsafe { obj_ref(parent) }.map_or(ptr::null_mut(), |p| p.child);
    if !child.is_null() {
        // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
        let prev = unsafe { obj_ref(child) }.map_or(ptr::null_mut(), |c| c.prev);
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(o) = unsafe { obj_mut(obj) } {
            o.prev = prev;
            o.next = child;
        }
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(prev) = unsafe { obj_mut(prev) } {
            prev.next = obj;
        }
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(child) = unsafe { obj_mut(child) } {
            child.prev = obj;
        }
    } else {
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(o) = unsafe { obj_mut(obj) } {
            o.next = obj;
            o.prev = obj;
        }
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(parent) = unsafe { obj_mut(parent) } {
            parent.child = obj;
        }
    }
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(o) = unsafe { obj_mut(obj) } {
        o.parent = parent;
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn remove_object(obj: object) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some((next, prev, parent)) = unsafe { obj_ref(obj) }.map(|o| (o.next, o.prev, o.parent))
    else {
        return;
    };
    let final_next = if !next.is_null() && next != obj {
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(prev) = unsafe { obj_mut(prev) } {
            prev.next = next;
        }
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(next) = unsafe { obj_mut(next) } {
            next.prev = prev;
        }
        next
    } else {
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(o) = unsafe { obj_mut(obj) } {
            o.next = ptr::null_mut();
            o.prev = ptr::null_mut();
        }
        ptr::null_mut()
    };
    if !parent.is_null() {
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(parent) = unsafe { obj_mut(parent) } {
            if parent.child == obj {
                parent.child = final_next;
            }
        }
    }
}

/// Bring an object to the front of its sibling list.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn move_to_front(obj: object) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(parent) = unsafe { obj_ref(obj) }.map(|o| o.parent) else {
        return;
    };
    // SAFETY: forwarded raw pointers satisfy `remove_object`'s contract per this function's own `# Safety` requirements.
    unsafe { remove_object(obj) };
    // SAFETY: forwarded raw pointers satisfy `add_object`'s contract per this function's own `# Safety` requirements.
    unsafe { add_object(obj, parent) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(parent) = unsafe { obj_mut(parent) } {
        parent.child = obj;
    }
}

/// Call a function on all objects in a sibling list.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn apply_to_list(first: object, fn_: actionfn) {
    let Some(fn_) = fn_ else { return };
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(start) = unsafe { obj_ref(first) }
        // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
        .and_then(|f| unsafe { obj_ref(f.parent) })
        .map(|p| p.child)
    else {
        return;
    };
    let mut obj = first;
    loop {
        fn_(obj);
        // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
        let Some(next) = unsafe { obj_ref(obj) }.map(|o| o.next) else {
            return;
        };
        obj = next;
        if obj == start {
            break;
        }
    }
}

/// Decrease the reference count of an object.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn decrease_refcount(obj: object) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(o) = (unsafe { obj_mut(obj) }) else {
        return;
    };
    if o.refcount <= 0 {
        return;
    }
    o.refcount -= 1;
    if o.refcount != 0 {
        return;
    }

    // Add to deletion list
    let new_node = memory::memalloc_bytes(std::mem::size_of::<DelNode>()) as *mut DelNode;
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    let Some(node) = (unsafe { ptr_mut(new_node) }) else {
        return;
    };
    node.obj = obj;
    node.next = new_node;
    node.prev = new_node;

    let del_base = with_graphapp_runtime(|runtime| runtime.objects.deletion_base);
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(base) = unsafe { ptr_mut(del_base) } {
        node.prev = base.prev;
        node.next = del_base;
        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(prev) = unsafe { ptr_mut(node.prev) } {
            prev.next = new_node;
        }
        base.prev = new_node;
    } else {
        with_graphapp_runtime(|runtime| runtime.objects.deletion_base = new_node);
    }
}

/// Increase the reference count of an object.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn increase_refcount(obj: object) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(o) = unsafe { obj_mut(obj) } {
        if o.refcount >= 0 {
            o.refcount += 1;
        }
    }
}

/// Protect an object from deletion by setting refcount to -1.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn protect_object(obj: object) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(o) = unsafe { obj_mut(obj) } {
        o.refcount = -1;
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn remove_delnode(n: *mut DelNode) {
    // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
    let Some((next, prev)) = unsafe { ptr_ref(n) }.map(|n| (n.next, n.prev)) else {
        return;
    };
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(prev) = unsafe { ptr_mut(prev) } {
        prev.next = next;
    }
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(next) = unsafe { ptr_mut(next) } {
        next.prev = prev;
    }
    if n == next {
        with_graphapp_runtime(|runtime| runtime.objects.deletion_base = ptr::null_mut());
    } else if n == with_graphapp_runtime(|runtime| runtime.objects.deletion_base) {
        with_graphapp_runtime(|runtime| runtime.objects.deletion_base = next);
    }
    // SAFETY: forwarded raw pointers satisfy `memfree`'s contract per this function's own `# Safety` requirements.
    unsafe { memory::memfree(n as *mut u8) };
}

fn remove_deleted_object(obj: object) {
    let del_base = with_graphapp_runtime(|runtime| runtime.objects.deletion_base);
    if del_base.is_null() {
        return;
    }
    let mut next = del_base;
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let Some(last) = unsafe { ptr_ref(next) }.map(|n| n.prev) else {
        return;
    };
    loop {
        let n = next;
        // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
        let Some((entry_obj, entry_next)) = unsafe { ptr_ref(n) }.map(|n| (n.obj, n.next)) else {
            return;
        };
        next = entry_next;
        if entry_obj == obj {
            // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
            unsafe { remove_delnode(n) };
        }
        if n == last {
            break;
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn del_object(obj: object) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    while let Some(child) = unsafe { obj_ref(obj) }
        .map(|o| o.child)
        .filter(|c| !c.is_null())
    {
        // SAFETY: forwarded raw pointers satisfy `del_object`'s contract per this function's own `# Safety` requirements.
        unsafe { del_object(child) };
    }
    // SAFETY: forwarded raw pointers satisfy `free_object`'s contract per this function's own `# Safety` requirements.
    unsafe { free_object(obj) };
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn update_app_globals(obj: object) {
    // These need to be set when the corresponding modules are initialized
    // For now, stub out the global drawstate reference
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if unsafe { obj_ref(obj) }.map_or(false, |o| !o.drawstate.is_null()) {
        // Keep the global drawstate hook in sync if a live drawstate tracker is added.
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn free_private(obj: object) {
    // SAFETY: forwarded raw pointers satisfy `remove_object`'s contract per this function's own `# Safety` requirements.
    unsafe { remove_object(obj) };
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let die = unsafe { obj_ref(obj) }
        // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
        .and_then(|o| unsafe { ptr_ref(o.call) })
        .and_then(|call| call.die);
    if let Some(die) = die {
        die(obj);
    }
    // SAFETY: forwarded raw pointers satisfy `update_app_globals`'s contract per this function's own `# Safety` requirements.
    unsafe { update_app_globals(obj) };
    // del_context(obj); // handled by context module
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(die) = unsafe { obj_ref(obj) }.and_then(|o| o.die) {
        die(obj);
    }
    remove_deleted_object(obj);
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn free_object(obj: object) {
    // SAFETY: forwarded raw pointers satisfy `free_private`'s contract per this function's own `# Safety` requirements.
    unsafe { free_private(obj) };
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some((drawstate, text, call)) =
        unsafe { obj_ref(obj) }.map(|o| (o.drawstate, o.text, o.call))
    else {
        return;
    };
    if !drawstate.is_null() {
        // SAFETY: forwarded raw pointers satisfy `memfree`'s contract per this function's own `# Safety` requirements.
        unsafe { memory::memfree(drawstate as *mut u8) };
    }
    if !text.is_null() {
        // SAFETY: forwarded raw pointers satisfy `memfree`'s contract per this function's own `# Safety` requirements.
        unsafe { memory::memfree(text as *mut u8) };
    }
    if !call.is_null() {
        // SAFETY: forwarded raw pointers satisfy `memfree`'s contract per this function's own `# Safety` requirements.
        unsafe { memory::memfree(call as *mut u8) };
    }
    // SAFETY: forwarded raw pointers satisfy `memfree`'s contract per this function's own `# Safety` requirements.
    unsafe { memory::memfree(obj as *mut u8) };
}

/// Traverse the deletion list and delete every object.
pub fn deletion_traversal() {
    with_graphapp_runtime(|runtime| runtime.objects.deletion_level += 1);
    if with_graphapp_runtime(|runtime| runtime.objects.deletion_level) == 1 {
        loop {
            let del_base = with_graphapp_runtime(|runtime| runtime.objects.deletion_base);
            if del_base.is_null() {
                break;
            }
            // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
            let Some(obj) = unsafe { ptr_ref(del_base) }.map(|d| d.obj) else {
                break;
            };
            if !obj.is_null() {
                // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
                if unsafe { obj_ref(obj) }.map_or(0, |o| o.refcount) == 0 {
                    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
                    unsafe { del_object(obj) };
                } else {
                    remove_deleted_object(obj);
                }
            }
        }
    }
    with_graphapp_runtime(|runtime| runtime.objects.deletion_level -= 1);
}

/// Create and return a new object with a refcount of 1.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn new_object(kind: c_int, handle: *mut c_void, parent: object) -> object {
    let obj = memory::memalloc_bytes(std::mem::size_of::<ObjInfo>()) as object;
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(o) = (unsafe { obj_mut(obj) }) else {
        return ptr::null_mut();
    };
    *o = ObjInfo {
        refcount: 1,
        kind,
        handle,
        bg: Transparent,
        ..Default::default()
    };

    if (kind & ControlObject) != 0 {
        let call = memory::memalloc_bytes(std::mem::size_of::<callinfo>()) as *mut callinfo;
        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
        let Some(call_ref) = (unsafe { ptr_mut(call) }) else {
            // SAFETY: forwarded raw pointers satisfy `memfree`'s contract per this function's own `# Safety` requirements.
            unsafe { memory::memfree(obj as *mut u8) };
            return ptr::null_mut();
        };
        *call_ref = callinfo::default();
        o.call = call;
    }

    // SAFETY: forwarded raw pointers satisfy `add_object`'s contract per this function's own `# Safety` requirements.
    unsafe { add_object(obj, parent) };
    obj
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn match_object(obj: object, handle: *mut c_void, id: c_int, key: c_int) -> object {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(o) = (unsafe { obj_ref(obj) }) else {
        return ptr::null_mut();
    };
    if !handle.is_null() && o.handle == handle {
        return obj;
    }
    if id != 0 && o.id == id {
        return obj;
    }
    if key != 0 && o.key == key && o.kind == MenuitemObject {
        return obj;
    }
    ptr::null_mut()
}

/// Perform a multi-child tree traversal to find an object.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn tree_search(top: object, handle: *mut c_void, id: c_int, key: c_int) -> object {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(child) = unsafe { obj_ref(top) }.map(|t| t.child) else {
        return ptr::null_mut();
    };
    if child.is_null() {
        return ptr::null_mut();
    }

    let mut first_object = child;
    let mut obj = first_object;

    while obj != top {
        // SAFETY: forwarded raw pointers satisfy `match_object`'s contract per this function's own `# Safety` requirements.
        let found = unsafe { match_object(obj, handle, id, key) };
        if !found.is_null() {
            return found;
        }

        // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
        let Some(o) = (unsafe { obj_ref(obj) }) else {
            return ptr::null_mut();
        };
        if !o.child.is_null() {
            first_object = o.child;
            obj = first_object;
            continue;
        } else {
            obj = o.next;
        }

        while obj == first_object {
            // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
            let Some(parent) = unsafe { obj_ref(obj) }.map(|o| o.parent) else {
                return ptr::null_mut();
            };
            obj = parent;
            if obj == top || obj.is_null() {
                break;
            }
            // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
            let Some((parent, next)) = unsafe { obj_ref(obj) }.map(|o| (o.parent, o.next)) else {
                return ptr::null_mut();
            };
            if !parent.is_null() {
                // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
                first_object = unsafe { obj_ref(parent) }.map_or(ptr::null_mut(), |p| p.child);
                obj = next;
            } else {
                break;
            }
        }
    }

    ptr::null_mut()
}

/// Find an object in the tree.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn find_object(handle: *mut c_void, id: c_int, key: c_int) -> object {
    // SAFETY: forwarded raw pointers satisfy `tree_search`'s contract per this function's own `# Safety` requirements.
    unsafe {
        tree_search(
            with_graphapp_runtime(|runtime| runtime.objects.base_object),
            handle,
            id,
            key,
        )
    }
}

/// Remove a menu item from the hierarchy.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn remove_menu_item(obj: object) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(die) = unsafe { obj_ref(obj) }.and_then(|o| o.die) else {
        // SAFETY: forwarded raw pointers satisfy `remove_object`'s contract per this function's own `# Safety` requirements.
        unsafe { remove_object(obj) };
        remove_deleted_object(obj);
        return;
    };
    die(obj);
    // SAFETY: forwarded raw pointers satisfy `remove_object`'s contract per this function's own `# Safety` requirements.
    unsafe { remove_object(obj) };
    remove_deleted_object(obj);
}

/// Finish objects (cleanup at application exit).
pub fn finish_objects() {
    let base = with_graphapp_runtime(|runtime| runtime.objects.base_object);
    if base.is_null() {
        return;
    }
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    while let Some(child) = unsafe { obj_ref(base) }
        .map(|b| b.child)
        .filter(|c| !c.is_null())
    {
        // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
        unsafe { free_object(child) };
    }
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    unsafe { free_object(base) };
    with_graphapp_runtime(|runtime| runtime.objects.base_object = ptr::null_mut());
}

/// Find next valid sibling (visible, enabled, not a label).
pub fn find_next_valid_sibling(obj: object) -> object {
    find_valid_sibling(obj, true)
}

/// Find previous valid sibling.
pub fn find_prev_valid_sibling(obj: object) -> object {
    find_valid_sibling(obj, false)
}

fn find_valid_sibling(obj: object, next_dir: bool) -> object {
    if obj.is_null() {
        return ptr::null_mut();
    }
    let first = obj;
    let mut o = obj;
    loop {
        // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
        let Some(o_ref) = (unsafe { obj_ref(o) }) else {
            return first;
        };
        if (o_ref.kind & ControlObject) != 0
            && (o_ref.state & GA_Enabled) != 0
            && (o_ref.state & GA_Visible) != 0
            && o_ref.kind != LabelObject
        {
            return o;
        }
        // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
        let Some(next) = unsafe { obj_ref(o) }.map(|o| if next_dir { o.next } else { o.prev })
        else {
            return first;
        };
        o = next;
        if o == first {
            break;
        }
    }
    first
}

/// Delete an object (public API).
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn delobj(obj: objptr) {
    if !obj.is_null() {
        // SAFETY: forwarded raw pointers satisfy `decrease_refcount`'s contract per this function's own `# Safety` requirements.
        unsafe { decrease_refcount(obj) };
    }
}
