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
    let Some(base) = obj_mut(obj) else { return };
    *base = ObjInfo {
        kind: BaseObject,
        next: obj,
        prev: obj,
        ..Default::default()
    };
    with_graphapp_runtime(|runtime| runtime.objects.base_object = obj);
}

fn add_object(obj: object, parent: object) {
    let parent = if parent.is_null() {
        with_graphapp_runtime(|runtime| runtime.objects.base_object)
    } else {
        parent
    };

    let child = obj_ref(parent).map_or(ptr::null_mut(), |p| p.child);
    if !child.is_null() {
        let prev = obj_ref(child).map_or(ptr::null_mut(), |c| c.prev);
        if let Some(o) = obj_mut(obj) {
            o.prev = prev;
            o.next = child;
        }
        if let Some(prev) = obj_mut(prev) {
            prev.next = obj;
        }
        if let Some(child) = obj_mut(child) {
            child.prev = obj;
        }
    } else {
        if let Some(o) = obj_mut(obj) {
            o.next = obj;
            o.prev = obj;
        }
        if let Some(parent) = obj_mut(parent) {
            parent.child = obj;
        }
    }
    if let Some(o) = obj_mut(obj) {
        o.parent = parent;
    }
}

fn remove_object(obj: object) {
    let Some((next, prev, parent)) = obj_ref(obj).map(|o| (o.next, o.prev, o.parent)) else {
        return;
    };
    let final_next = if !next.is_null() && next != obj {
        if let Some(prev) = obj_mut(prev) {
            prev.next = next;
        }
        if let Some(next) = obj_mut(next) {
            next.prev = prev;
        }
        next
    } else {
        if let Some(o) = obj_mut(obj) {
            o.next = ptr::null_mut();
            o.prev = ptr::null_mut();
        }
        ptr::null_mut()
    };
    if !parent.is_null() {
        if let Some(parent) = obj_mut(parent) {
            if parent.child == obj {
                parent.child = final_next;
            }
        }
    }
}

/// Bring an object to the front of its sibling list.
pub fn move_to_front(obj: object) {
    let Some(parent) = obj_ref(obj).map(|o| o.parent) else {
        return;
    };
    remove_object(obj);
    add_object(obj, parent);
    if let Some(parent) = obj_mut(parent) {
        parent.child = obj;
    }
}

/// Call a function on all objects in a sibling list.
pub fn apply_to_list(first: object, fn_: actionfn) {
    let Some(fn_) = fn_ else { return };
    let Some(start) = obj_ref(first)
        .and_then(|f| obj_ref(f.parent))
        .map(|p| p.child)
    else {
        return;
    };
    let mut obj = first;
    loop {
        fn_(obj);
        let Some(next) = obj_ref(obj).map(|o| o.next) else {
            return;
        };
        obj = next;
        if obj == start {
            break;
        }
    }
}

/// Decrease the reference count of an object.
pub fn decrease_refcount(obj: object) {
    let Some(o) = obj_mut(obj) else { return };
    if o.refcount <= 0 {
        return;
    }
    o.refcount -= 1;
    if o.refcount != 0 {
        return;
    }

    // Add to deletion list
    let new_node = memory::memalloc_bytes(std::mem::size_of::<DelNode>()) as *mut DelNode;
    let Some(node) = ptr_mut(new_node) else { return };
    node.obj = obj;
    node.next = new_node;
    node.prev = new_node;

    let del_base = with_graphapp_runtime(|runtime| runtime.objects.deletion_base);
    if let Some(base) = ptr_mut(del_base) {
        node.prev = base.prev;
        node.next = del_base;
        if let Some(prev) = ptr_mut(node.prev) {
            prev.next = new_node;
        }
        base.prev = new_node;
    } else {
        with_graphapp_runtime(|runtime| runtime.objects.deletion_base = new_node);
    }
}

/// Increase the reference count of an object.
pub fn increase_refcount(obj: object) {
    if let Some(o) = obj_mut(obj) {
        if o.refcount >= 0 {
            o.refcount += 1;
        }
    }
}

/// Protect an object from deletion by setting refcount to -1.
pub fn protect_object(obj: object) {
    if let Some(o) = obj_mut(obj) {
        o.refcount = -1;
    }
}

fn remove_delnode(n: *mut DelNode) {
    let Some((next, prev)) = ptr_ref(n).map(|n| (n.next, n.prev)) else {
        return;
    };
    if let Some(prev) = ptr_mut(prev) {
        prev.next = next;
    }
    if let Some(next) = ptr_mut(next) {
        next.prev = prev;
    }
    if n == next {
        with_graphapp_runtime(|runtime| runtime.objects.deletion_base = ptr::null_mut());
    } else if n == with_graphapp_runtime(|runtime| runtime.objects.deletion_base) {
        with_graphapp_runtime(|runtime| runtime.objects.deletion_base = next);
    }
    memory::memfree(n as *mut u8);
}

fn remove_deleted_object(obj: object) {
    let del_base = with_graphapp_runtime(|runtime| runtime.objects.deletion_base);
    if del_base.is_null() {
        return;
    }
    let mut next = del_base;
    let Some(last) = ptr_ref(next).map(|n| n.prev) else {
        return;
    };
    loop {
        let n = next;
        let Some((entry_obj, entry_next)) = ptr_ref(n).map(|n| (n.obj, n.next)) else {
            return;
        };
        next = entry_next;
        if entry_obj == obj {
            remove_delnode(n);
        }
        if n == last {
            break;
        }
    }
}

fn del_object(obj: object) {
    while let Some(child) = obj_ref(obj).map(|o| o.child).filter(|c| !c.is_null()) {
        del_object(child);
    }
    free_object(obj);
}

fn update_app_globals(obj: object) {
    // These need to be set when the corresponding modules are initialized
    // For now, stub out the global drawstate reference
    if obj_ref(obj).map_or(false, |o| !o.drawstate.is_null()) {
        // Keep the global drawstate hook in sync if a live drawstate tracker is added.
    }
}

fn free_private(obj: object) {
    remove_object(obj);
    let die = obj_ref(obj)
        .and_then(|o| ptr_ref(o.call))
        .and_then(|call| call.die);
    if let Some(die) = die {
        die(obj);
    }
    update_app_globals(obj);
    // del_context(obj); // handled by context module
    if let Some(die) = obj_ref(obj).and_then(|o| o.die) {
        die(obj);
    }
    remove_deleted_object(obj);
}

fn free_object(obj: object) {
    free_private(obj);
    let Some((drawstate, text, call)) = obj_ref(obj).map(|o| (o.drawstate, o.text, o.call))
    else {
        return;
    };
    if !drawstate.is_null() {
        memory::memfree(drawstate as *mut u8);
    }
    if !text.is_null() {
        memory::memfree(text as *mut u8);
    }
    if !call.is_null() {
        memory::memfree(call as *mut u8);
    }
    memory::memfree(obj as *mut u8);
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
            let Some(obj) = ptr_ref(del_base).map(|d| d.obj) else {
                break;
            };
            if !obj.is_null() {
                if obj_ref(obj).map_or(0, |o| o.refcount) == 0 {
                    del_object(obj);
                } else {
                    remove_deleted_object(obj);
                }
            }
        }
    }
    with_graphapp_runtime(|runtime| runtime.objects.deletion_level -= 1);
}

/// Create and return a new object with a refcount of 1.
pub fn new_object(kind: c_int, handle: *mut c_void, parent: object) -> object {
    let obj = memory::memalloc_bytes(std::mem::size_of::<ObjInfo>()) as object;
    let Some(o) = obj_mut(obj) else {
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
        let Some(call_ref) = ptr_mut(call) else {
            memory::memfree(obj as *mut u8);
            return ptr::null_mut();
        };
        *call_ref = callinfo::default();
        o.call = call;
    }

    add_object(obj, parent);
    obj
}

fn match_object(obj: object, handle: *mut c_void, id: c_int, key: c_int) -> object {
    let Some(o) = obj_ref(obj) else {
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
pub fn tree_search(top: object, handle: *mut c_void, id: c_int, key: c_int) -> object {
    let Some(child) = obj_ref(top).map(|t| t.child) else {
        return ptr::null_mut();
    };
    if child.is_null() {
        return ptr::null_mut();
    }

    let mut first_object = child;
    let mut obj = first_object;

    while obj != top {
        let found = match_object(obj, handle, id, key);
        if !found.is_null() {
            return found;
        }

        let Some(o) = obj_ref(obj) else {
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
            let Some(parent) = obj_ref(obj).map(|o| o.parent) else {
                return ptr::null_mut();
            };
            obj = parent;
            if obj == top || obj.is_null() {
                break;
            }
            let Some((parent, next)) = obj_ref(obj).map(|o| (o.parent, o.next)) else {
                return ptr::null_mut();
            };
            if !parent.is_null() {
                first_object = obj_ref(parent).map_or(ptr::null_mut(), |p| p.child);
                obj = next;
            } else {
                break;
            }
        }
    }

    ptr::null_mut()
}

/// Find an object in the tree.
pub fn find_object(handle: *mut c_void, id: c_int, key: c_int) -> object {
    tree_search(
        with_graphapp_runtime(|runtime| runtime.objects.base_object),
        handle,
        id,
        key,
    )
}

/// Remove a menu item from the hierarchy.
pub fn remove_menu_item(obj: object) {
    let Some(die) = obj_ref(obj).and_then(|o| o.die) else {
        remove_object(obj);
        remove_deleted_object(obj);
        return;
    };
    die(obj);
    remove_object(obj);
    remove_deleted_object(obj);
}

/// Finish objects (cleanup at application exit).
pub fn finish_objects() {
    let base = with_graphapp_runtime(|runtime| runtime.objects.base_object);
    if base.is_null() {
        return;
    }
    while let Some(child) = obj_ref(base).map(|b| b.child).filter(|c| !c.is_null()) {
        free_object(child);
    }
    free_object(base);
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
        let Some(o_ref) = obj_ref(o) else { return first };
        if (o_ref.kind & ControlObject) != 0
            && (o_ref.state & GA_Enabled) != 0
            && (o_ref.state & GA_Visible) != 0
            && o_ref.kind != LabelObject
        {
            return o;
        }
        let Some(next) = obj_ref(o).map(|o| if next_dir { o.next } else { o.prev }) else {
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
pub fn delobj(obj: objptr) {
    if !obj.is_null() {
        decrease_refcount(obj);
    }
}
