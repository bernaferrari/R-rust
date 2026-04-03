#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Object management for GraphApp.
//!
//! Ported from objects.c - maintains internal info about graphical objects
//! using a linked-list hierarchy with reference counting.

use std::os::raw::{c_int, c_void};
use std::ptr;

use super::memory;
use super::strings;
use super::types::*;

/// Global base object at the top of the object hierarchy.
static mut BASE_OBJECT: object = ptr::null_mut();

/// Deletion list node.
struct DelNode {
    obj: object,
    next: *mut DelNode,
    prev: *mut DelNode,
}

static mut DEL_BASE: *mut DelNode = ptr::null_mut();

/// Initialise the base object and set the list to be empty.
pub unsafe fn init_objects() {
    unsafe {
        if !BASE_OBJECT.is_null() {
            return;
        }
        let obj = memory::memalloc(std::mem::size_of::<ObjInfo>() as i64) as object;
        if obj.is_null() {
            return;
        }
        ptr::write_bytes(obj as *mut u8, 0, std::mem::size_of::<ObjInfo>());
        (*obj).kind = BaseObject;
        (*obj).next = obj;
        (*obj).prev = obj;
        (*obj).parent = ptr::null_mut();
        (*obj).child = ptr::null_mut();
        BASE_OBJECT = obj;
    }
}

unsafe fn add_object(obj: object, parent: object) {
    unsafe {
        let parent = if parent.is_null() {
            BASE_OBJECT
        } else {
            parent
        };

        if !(*parent).child.is_null() {
            (*obj).prev = (*(*parent).child).prev;
            (*obj).next = (*parent).child;
            (*(*obj).prev).next = obj;
            (*(*obj).next).prev = obj;
        } else {
            (*obj).next = obj;
            (*obj).prev = obj;
            (*parent).child = obj;
        }
        (*obj).parent = parent;
    }
}

unsafe fn remove_object(obj: object) {
    unsafe {
        if !(*obj).next.is_null() && (*obj).next != obj {
            (*(*obj).prev).next = (*obj).next;
            (*(*obj).next).prev = (*obj).prev;
        } else {
            (*obj).next = ptr::null_mut();
            (*obj).prev = ptr::null_mut();
        }
        if !(*obj).parent.is_null() {
            if (*(*obj).parent).child == obj {
                (*(*obj).parent).child = (*obj).next;
            }
        }
    }
}

/// Bring an object to the front of its sibling list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn move_to_front(obj: object) {
    unsafe {
        if obj.is_null() {
            return;
        }
        let parent = (*obj).parent;
        remove_object(obj);
        add_object(obj, parent);
        (*parent).child = obj;
    }
}

/// Call a function on all objects in a sibling list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn apply_to_list(first: object, fn_: actionfn) {
    unsafe {
        if first.is_null() || fn_.is_none() {
            return;
        }
        let start = (*(*first).parent).child;
        let mut obj = first;
        loop {
            fn_.unwrap()(obj);
            obj = (*obj).next;
            if obj == start {
                break;
            }
        }
    }
}

/// Decrease the reference count of an object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn decrease_refcount(obj: object) {
    unsafe {
        if obj.is_null() {
            return;
        }
        if (*obj).refcount <= 0 {
            return;
        }
        (*obj).refcount -= 1;
        if (*obj).refcount != 0 {
            return;
        }

        // Add to deletion list
        let new_node = memory::memalloc(std::mem::size_of::<DelNode>() as i64) as *mut DelNode;
        if new_node.is_null() {
            return;
        }
        (*new_node).obj = obj;
        (*new_node).next = new_node;
        (*new_node).prev = new_node;

        if !DEL_BASE.is_null() {
            (*new_node).prev = (*DEL_BASE).prev;
            (*new_node).next = DEL_BASE;
            (*(*new_node).prev).next = new_node;
            (*(*new_node).next).prev = new_node;
        } else {
            DEL_BASE = new_node;
        }
    }
}

/// Increase the reference count of an object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn increase_refcount(obj: object) {
    unsafe {
        if !obj.is_null() && (*obj).refcount >= 0 {
            (*obj).refcount += 1;
        }
    }
}

/// Protect an object from deletion by setting refcount to -1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn protect_object(obj: object) {
    unsafe {
        if !obj.is_null() {
            (*obj).refcount = -1;
        }
    }
}

unsafe fn remove_delnode(n: *mut DelNode) {
    unsafe {
        (*(*n).prev).next = (*n).next;
        (*(*n).next).prev = (*n).prev;
        if n == (*n).next {
            DEL_BASE = ptr::null_mut();
        } else if n == DEL_BASE {
            DEL_BASE = (*n).next;
        }
        memory::memfree(n as *mut u8);
    }
}

unsafe fn remove_deleted_object(obj: object) {
    unsafe {
        if DEL_BASE.is_null() {
            return;
        }
        let mut next = DEL_BASE;
        let last = (*DEL_BASE).prev;
        loop {
            let n = next;
            next = (*n).next;
            if (*n).obj == obj {
                remove_delnode(n);
            }
            if n == last {
                break;
            }
        }
    }
}

unsafe fn del_object(obj: object) {
    unsafe {
        while !(*obj).child.is_null() {
            del_object((*obj).child);
        }
        free_object(obj);
    }
}

unsafe fn update_app_globals(obj: object) {
    unsafe {
        // These need to be set when the corresponding modules are initialized
        // For now, stub out the global drawstate reference
        if !(*obj).drawstate.is_null() {
            // TODO: check if drawstate is current
        }
    }
}

unsafe fn free_private(obj: object) {
    unsafe {
        remove_object(obj);
        if !(*obj).call.is_null() && !(*(*obj).call).die.is_none() {
            (*(*obj).call).die.unwrap()(obj);
        }
        update_app_globals(obj);
        // del_context(obj); // handled by context module
        if !(*obj).die.is_none() {
            (*obj).die.unwrap()(obj);
        }
        remove_deleted_object(obj);
    }
}

unsafe fn free_object(obj: object) {
    unsafe {
        free_private(obj);
        if !(*obj).drawstate.is_null() {
            memory::memfree((*obj).drawstate as *mut u8);
        }
        if !(*obj).text.is_null() {
            memory::memfree((*obj).text as *mut u8);
        }
        if !(*obj).call.is_null() {
            memory::memfree((*obj).call as *mut u8);
        }
        memory::memfree(obj as *mut u8);
    }
}

/// Traverse the deletion list and delete every object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn deletion_traversal() {
    unsafe {
        static mut LEVEL: c_int = 0;
        LEVEL += 1;
        if LEVEL == 1 {
            while !DEL_BASE.is_null() {
                let obj = (*DEL_BASE).obj;
                if !obj.is_null() {
                    if (*obj).refcount == 0 {
                        del_object(obj);
                    } else {
                        remove_deleted_object(obj);
                    }
                }
            }
        }
        LEVEL -= 1;
    }
}

/// Create and return a new object with a refcount of 1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn new_object(kind: c_int, handle: *mut c_void, parent: object) -> object {
    unsafe {
        let obj = memory::memalloc(std::mem::size_of::<ObjInfo>() as i64) as object;
        if obj.is_null() {
            return ptr::null_mut();
        }
        ptr::write_bytes(obj as *mut u8, 0, std::mem::size_of::<ObjInfo>());

        (*obj).refcount = 1;
        (*obj).kind = kind;
        (*obj).handle = handle;
        (*obj).bg = Transparent;

        if (kind & ControlObject) != 0 {
            let call = memory::memalloc(std::mem::size_of::<callinfo>() as i64) as *mut callinfo;
            if call.is_null() {
                memory::memfree(obj as *mut u8);
                return ptr::null_mut();
            }
            ptr::write_bytes(call as *mut u8, 0, std::mem::size_of::<callinfo>());
            (*obj).call = call;
        }

        add_object(obj, parent);
        obj
    }
}

unsafe fn match_object(obj: object, handle: *mut c_void, id: c_int, key: c_int) -> object {
    unsafe {
        if !handle.is_null() && (*obj).handle == handle {
            return obj;
        }
        if id != 0 && (*obj).id == id {
            return obj;
        }
        if key != 0 && (*obj).key == key && (*obj).kind == MenuitemObject {
            return obj;
        }
        ptr::null_mut()
    }
}

/// Perform a multi-child tree traversal to find an object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tree_search(
    top: object,
    handle: *mut c_void,
    id: c_int,
    key: c_int,
) -> object {
    unsafe {
        if top.is_null() || (*top).child.is_null() {
            return ptr::null_mut();
        }

        let mut first_object = (*top).child;
        let mut obj = first_object;

        while obj != top {
            let found = match_object(obj, handle, id, key);
            if !found.is_null() {
                return found;
            }

            if !(*obj).child.is_null() {
                first_object = (*obj).child;
                obj = first_object;
                continue;
            } else {
                obj = (*obj).next;
            }

            while obj == first_object {
                obj = (*obj).parent;
                if obj == top || obj.is_null() {
                    break;
                }
                if !(*obj).parent.is_null() {
                    first_object = (*(*obj).parent).child;
                    obj = (*obj).next;
                } else {
                    break;
                }
            }
        }

        ptr::null_mut()
    }
}

/// Find an object in the tree.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn find_object(handle: *mut c_void, id: c_int, key: c_int) -> object {
    unsafe { tree_search(BASE_OBJECT, handle, id, key) }
}

/// Remove a menu item from the hierarchy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remove_menu_item(obj: object) {
    unsafe {
        if obj.is_null() {
            return;
        }
        if !(*obj).die.is_none() {
            (*obj).die.unwrap()(obj);
        }
        remove_object(obj);
        remove_deleted_object(obj);
    }
}

/// Finish objects (cleanup at application exit).
pub unsafe fn finish_objects() {
    unsafe {
        if !BASE_OBJECT.is_null() {
            // Walk children and free
            while !(*BASE_OBJECT).child.is_null() {
                free_object((*BASE_OBJECT).child);
            }
            free_object(BASE_OBJECT);
            BASE_OBJECT = ptr::null_mut();
        }
    }
}

/// Find next valid sibling (visible, enabled, not a label).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn find_next_valid_sibling(obj: object) -> object {
    unsafe { find_valid_sibling(obj, true) }
}

/// Find previous valid sibling.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn find_prev_valid_sibling(obj: object) -> object {
    unsafe { find_valid_sibling(obj, false) }
}

unsafe fn find_valid_sibling(obj: object, next_dir: bool) -> object {
    unsafe {
        if obj.is_null() {
            return ptr::null_mut();
        }
        let first = obj;
        let mut o = obj;
        loop {
            if ((*o).kind & ControlObject) != 0
                && ((*o).state & GA_Enabled) != 0
                && ((*o).state & GA_Visible) != 0
                && (*o).kind != LabelObject
            {
                return o;
            }
            o = if next_dir { (*o).next } else { (*o).prev };
            if o == first {
                break;
            }
        }
        first
    }
}

/// Delete an object (public API).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn delobj(obj: objptr) {
    unsafe {
        if !obj.is_null() {
            decrease_refcount(obj);
        }
    }
}
