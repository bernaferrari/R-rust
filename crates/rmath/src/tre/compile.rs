#![allow(unused_variables)]
#![allow(unused_assignments)]
/*
  tre/compile.rs - TRE regex compiler

  Ported from tre-compile.c
*/
#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::identity_op)]

use std::os::raw::{c_int, c_void};
use std::ptr;

use super::ast::*;
use super::mem;
use super::stack;

// Compiler continuations can contain several stack words. Stop on the first
// failed push so a partial continuation is never interpreted as a complete one.
macro_rules! checked_stack_push {
    ($push:expr) => {
        checked_stack_push!($push, {})
    };
    ($push:expr, $cleanup:block) => {
        if $push != stack::REG_OK {
            $cleanup
            return REG_ESPACE as c_int;
        }
    };
}

// ===== Tag insertion =====

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum tre_addtags_symbol_t {
    ADDTAGS_RECURSE,
    ADDTAGS_AFTER_ITERATION,
    ADDTAGS_AFTER_UNION_LEFT,
    ADDTAGS_AFTER_UNION_RIGHT,
    ADDTAGS_AFTER_CAT_LEFT,
    ADDTAGS_AFTER_CAT_RIGHT,
    ADDTAGS_SET_SUBMATCH_END,
}

#[repr(C)]
struct tre_tag_states_t {
    tag: c_int,
    next_tag: c_int,
}

// Construct every field before publishing a child copied during tag insertion.
unsafe fn tre_tagged_child(
    mem: mem::tre_mem_t,
    node: *const tre_ast_node_t,
) -> *mut tre_ast_node_t {
    unsafe {
        let child =
            mem::tre_mem_alloc(mem, std::mem::size_of::<tre_ast_node_t>()) as *mut tre_ast_node_t;
        if !child.is_null() {
            child.write(tre_ast_node_t {
                type_: (*node).type_,
                obj: (*node).obj,
                nullable: -1,
                submatch_id: -1,
                num_submatches: (*node).num_submatches,
                num_tags: 0,
                firstpos: ptr::null_mut(),
                lastpos: ptr::null_mut(),
            });
        }
        child
    }
}

#[test]
fn tag_wrapper_children_initialize_all_metadata_before_expansion() {
    unsafe {
        for left in [false, true] {
            let arena = mem::tre_mem_new();
            assert!(!arena.is_null());
            let source = tre_ast_new_literal(arena, 65, 65, 0);
            assert!(!source.is_null());
            (*source).num_submatches = 7;
            let status = if left {
                tre_add_tag_left(arena, source, 0)
            } else {
                tre_add_tag_right(arena, source, 0)
            };
            assert_eq!(status, REG_OK);
            let catenation = (*source).obj as *mut tre_catenation_t;
            let copied = if left {
                (*catenation).right
            } else {
                (*catenation).left
            };
            assert_eq!((*copied).num_submatches, 7);
            assert_eq!((*copied).num_tags, 0);
            assert_eq!((*copied).nullable, -1);
            assert_eq!((*copied).submatch_id, -1);
            assert!((*copied).firstpos.is_null());
            assert!((*copied).lastpos.is_null());
            mem::tre_mem_destroy(arena);
        }
    }
}

unsafe fn tre_add_tag_left(mem: mem::tre_mem_t, node: *mut tre_ast_node_t, tag_id: c_int) -> c_int {
    unsafe {
        let c = mem::tre_mem_alloc(mem, std::mem::size_of::<tre_catenation_t>())
            as *mut tre_catenation_t;
        if c.is_null() {
            return REG_ESPACE as c_int;
        }
        (*c).left = tre_ast_new_literal(mem, TAG as c_int, tag_id, -1);
        if (*c).left.is_null() {
            return REG_ESPACE as c_int;
        }
        (*c).right = tre_tagged_child(mem, node);
        if (*c).right.is_null() {
            return REG_ESPACE as c_int;
        }

        (*node).obj = c as *mut c_void;
        (*node).type_ = tre_ast_type_t::CATENATION;
        REG_OK as c_int
    }
}

unsafe fn tre_add_tag_right(
    mem: mem::tre_mem_t,
    node: *mut tre_ast_node_t,
    tag_id: c_int,
) -> c_int {
    unsafe {
        let c = mem::tre_mem_alloc(mem, std::mem::size_of::<tre_catenation_t>())
            as *mut tre_catenation_t;
        if c.is_null() {
            return REG_ESPACE as c_int;
        }
        (*c).right = tre_ast_new_literal(mem, TAG as c_int, tag_id, -1);
        if (*c).right.is_null() {
            return REG_ESPACE as c_int;
        }
        (*c).left = tre_tagged_child(mem, node);
        if (*c).left.is_null() {
            return REG_ESPACE as c_int;
        }

        (*node).obj = c as *mut c_void;
        (*node).type_ = tre_ast_type_t::CATENATION;
        REG_OK as c_int
    }
}

unsafe fn tre_purge_regset(regset: *mut c_int, tnfa: *mut tre_tnfa_t, tag: c_int) {
    unsafe {
        let mut i = 0;
        while *regset.offset(i) >= 0 {
            let id = *regset.offset(i) / 2;
            let start = if *regset.offset(i) % 2 != 0 { 0 } else { 1 };
            if start != 0 {
                (*(*tnfa).submatch_data.offset(id as isize)).so_tag = tag;
            } else {
                (*(*tnfa).submatch_data.offset(id as isize)).eo_tag = tag;
            }
            i += 1;
        }
        *regset = -1;
    }
}

unsafe fn tre_add_tags(
    mem: mem::tre_mem_t,
    stack: *mut stack::tre_stack_rec,
    tree: *mut tre_ast_node_t,
    tnfa: *mut tre_tnfa_t,
    storage: &mut Option<&mut super::owned_arena::OwnedArena>,
) -> c_int {
    unsafe {
        let mut status: c_int = REG_OK as c_int;
        let mut node = tree;
        let bottom = stack::tre_stack_num_objects(stack);
        let first_pass = mem.is_null() || tnfa.is_null();
        let mut regset = mem::xmalloc(
            std::mem::size_of::<c_int>() * (((*tnfa).num_submatches as usize + 1) * 2),
        ) as *mut c_int;
        if regset.is_null() {
            return REG_ESPACE as c_int;
        }
        *regset = -1;
        let orig_regset = regset;

        let parents =
            mem::xmalloc(std::mem::size_of::<c_int>() * ((*tnfa).num_submatches as usize + 1))
                as *mut c_int;
        if parents.is_null() {
            mem::xfree(regset as *mut c_void);
            return REG_ESPACE as c_int;
        }
        *parents = -1;

        let saved_states = mem::xmalloc(
            std::mem::size_of::<tre_tag_states_t>() * ((*tnfa).num_submatches as usize + 1),
        ) as *mut tre_tag_states_t;
        if saved_states.is_null() {
            mem::xfree(regset as *mut c_void);
            mem::xfree(parents as *mut c_void);
            return REG_ESPACE as c_int;
        }
        for i in 0..=(*tnfa).num_submatches as isize {
            (*saved_states.offset(i)).tag = -1;
        }

        // These scratch tables predate arena ownership; release them on a
        // stack allocation failure just as on the ordinary exit path.
        macro_rules! checked_tag_stack_push {
            ($push:expr) => {
                checked_stack_push!($push, {
                    mem::xfree(orig_regset as *mut c_void);
                    mem::xfree(parents as *mut c_void);
                    mem::xfree(saved_states as *mut c_void);
                })
            };
        }

        checked_tag_stack_push!(stack::tre_stack_push_voidptr(stack, node as *mut c_void));
        checked_tag_stack_push!(stack::tre_stack_push_int(
            stack,
            tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int
        ));

        let mut num_tags: c_int = 0;
        let mut num_minimals: c_int = 0;
        let mut tag: c_int = 0;
        let mut next_tag: c_int = 1;
        let mut minimal_tag: c_int = -1;
        let mut direction = tre_tag_direction_t::TRE_TAG_MINIMIZE;

        if !first_pass {
            (*tnfa).end_tag = 0;
            *(*tnfa).minimal_tags = -1;
        }

        while stack::tre_stack_num_objects(stack) > bottom {
            if status != REG_OK as c_int {
                break;
            }

            let symbol = stack::tre_stack_pop_int(stack);
            match symbol {
                x if x == tre_addtags_symbol_t::ADDTAGS_SET_SUBMATCH_END as c_int => {
                    let id = stack::tre_stack_pop_int(stack);
                    let mut i = 0;
                    while *regset.offset(i) >= 0 {
                        i += 1;
                    }
                    *regset.offset(i) = id * 2 + 1;
                    *regset.offset(i + 1) = -1;

                    let mut i = 0;
                    while *parents.offset(i) >= 0 {
                        i += 1;
                    }
                    *parents.offset(i - 1) = -1;
                }
                x if x == tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int => {
                    node = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;

                    if (*node).submatch_id >= 0 {
                        let id = (*node).submatch_id;
                        let mut i = 0;
                        while *regset.offset(i) >= 0 {
                            i += 1;
                        }
                        *regset.offset(i) = id * 2;
                        *regset.offset(i + 1) = -1;

                        if !first_pass {
                            let mut i = 0;
                            while *parents.offset(i) >= 0 {
                                i += 1;
                            }
                            (*(*tnfa).submatch_data.offset(id as isize)).parents = ptr::null_mut();
                            if i > 0 {
                                let p = usize::try_from(i)
                                    .ok()
                                    .and_then(|n| n.checked_add(1))
                                    .map_or(ptr::null_mut(), |count| {
                                        allocate_retained_ints(count, false, storage)
                                    });
                                if p.is_null() {
                                    status = REG_ESPACE as c_int;
                                    // continue to break out of the loop
                                    continue;
                                }
                                for j in 0..i {
                                    *p.offset(j as isize) = *parents.offset(j as isize);
                                }
                                *p.offset(i as isize) = -1;
                                (*(*tnfa).submatch_data.offset(id as isize)).parents = p;
                            }
                        }

                        checked_tag_stack_push!(stack::tre_stack_push_int(
                            stack,
                            (*node).submatch_id
                        ));
                        checked_tag_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_addtags_symbol_t::ADDTAGS_SET_SUBMATCH_END as c_int,
                        ));
                    }

                    match (*node).type_ {
                        tre_ast_type_t::LITERAL => {
                            let lit = (*node).obj as *mut tre_literal_t;
                            if (!IS_SPECIAL(&*lit) || IS_BACKREF(&*lit)) && *regset >= 0 {
                                if !first_pass {
                                    status = tre_add_tag_left(mem, node, tag);
                                    *(*tnfa).tag_directions.offset(tag as isize) =
                                        direction as c_int;
                                    if minimal_tag >= 0 {
                                        let mut i = 0;
                                        while *(*tnfa).minimal_tags.offset(i) >= 0 {
                                            i += 1;
                                        }
                                        *(*tnfa).minimal_tags.offset(i) = tag;
                                        *(*tnfa).minimal_tags.offset(i + 1) = minimal_tag;
                                        *(*tnfa).minimal_tags.offset(i + 2) = -1;
                                        minimal_tag = -1;
                                        num_minimals += 1;
                                    }
                                    tre_purge_regset(regset, tnfa, tag);
                                } else {
                                    (*node).num_tags = 1;
                                }
                                *regset = -1;
                                tag = next_tag;
                                num_tags += 1;
                                next_tag += 1;
                            }
                        }
                        tre_ast_type_t::CATENATION => {
                            let cat = (*node).obj as *mut tre_catenation_t;
                            let left = (*cat).left;
                            let right = (*cat).right;
                            let mut reserved_tag: c_int = -1;

                            checked_tag_stack_push!(stack::tre_stack_push_voidptr(
                                stack,
                                node as *mut c_void
                            ));
                            checked_tag_stack_push!(stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_AFTER_CAT_RIGHT as c_int,
                            ));

                            checked_tag_stack_push!(stack::tre_stack_push_voidptr(
                                stack,
                                right as *mut c_void
                            ));
                            checked_tag_stack_push!(stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int,
                            ));

                            checked_tag_stack_push!(stack::tre_stack_push_int(
                                stack,
                                next_tag + (*left).num_tags
                            ));
                            if (*left).num_tags > 0 && (*right).num_tags > 0 {
                                reserved_tag = next_tag;
                                next_tag += 1;
                            }
                            checked_tag_stack_push!(stack::tre_stack_push_int(stack, reserved_tag));
                            checked_tag_stack_push!(stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_AFTER_CAT_LEFT as c_int,
                            ));

                            checked_tag_stack_push!(stack::tre_stack_push_voidptr(
                                stack,
                                left as *mut c_void
                            ));
                            checked_tag_stack_push!(stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int,
                            ));
                        }
                        tre_ast_type_t::ITERATION => {
                            let iter = (*node).obj as *mut tre_iteration_t;

                            if first_pass {
                                checked_tag_stack_push!(stack::tre_stack_push_int(
                                    stack,
                                    if *regset >= 0 || (*iter).minimal != 0 {
                                        1
                                    } else {
                                        0
                                    },
                                ));
                            } else {
                                checked_tag_stack_push!(stack::tre_stack_push_int(stack, tag));
                                checked_tag_stack_push!(stack::tre_stack_push_int(
                                    stack,
                                    (*iter).minimal as c_int
                                ));
                            }
                            checked_tag_stack_push!(stack::tre_stack_push_voidptr(
                                stack,
                                node as *mut c_void
                            ));
                            checked_tag_stack_push!(stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_AFTER_ITERATION as c_int,
                            ));

                            checked_tag_stack_push!(stack::tre_stack_push_voidptr(
                                stack,
                                (*iter).arg as *mut c_void
                            ));
                            checked_tag_stack_push!(stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int,
                            ));

                            if *regset >= 0 || (*iter).minimal != 0 {
                                if !first_pass {
                                    status = tre_add_tag_left(mem, node, tag);
                                    if (*iter).minimal != 0 {
                                        *(*tnfa).tag_directions.offset(tag as isize) =
                                            tre_tag_direction_t::TRE_TAG_MAXIMIZE as c_int;
                                    } else {
                                        *(*tnfa).tag_directions.offset(tag as isize) =
                                            direction as c_int;
                                    }
                                    if minimal_tag >= 0 {
                                        let mut i = 0;
                                        while *(*tnfa).minimal_tags.offset(i) >= 0 {
                                            i += 1;
                                        }
                                        *(*tnfa).minimal_tags.offset(i) = tag;
                                        *(*tnfa).minimal_tags.offset(i + 1) = minimal_tag;
                                        *(*tnfa).minimal_tags.offset(i + 2) = -1;
                                        minimal_tag = -1;
                                        num_minimals += 1;
                                    }
                                    tre_purge_regset(regset, tnfa, tag);
                                }
                                *regset = -1;
                                tag = next_tag;
                                num_tags += 1;
                                next_tag += 1;
                            }
                            direction = tre_tag_direction_t::TRE_TAG_MINIMIZE;
                        }
                        tre_ast_type_t::UNION => {
                            let uni = (*node).obj as *mut tre_union_t;
                            let left = (*uni).left;
                            let right = (*uni).right;
                            let (left_tag, right_tag) = if *regset >= 0 {
                                (next_tag, next_tag + 1)
                            } else {
                                (tag, next_tag)
                            };

                            checked_tag_stack_push!(stack::tre_stack_push_int(stack, right_tag));
                            checked_tag_stack_push!(stack::tre_stack_push_int(stack, left_tag));
                            checked_tag_stack_push!(stack::tre_stack_push_voidptr(
                                stack,
                                regset as *mut c_void
                            ));
                            checked_tag_stack_push!(stack::tre_stack_push_int(
                                stack,
                                if *regset >= 0 { 1 } else { 0 }
                            ));
                            checked_tag_stack_push!(stack::tre_stack_push_voidptr(
                                stack,
                                node as *mut c_void
                            ));
                            checked_tag_stack_push!(stack::tre_stack_push_voidptr(
                                stack,
                                right as *mut c_void
                            ));
                            checked_tag_stack_push!(stack::tre_stack_push_voidptr(
                                stack,
                                left as *mut c_void
                            ));
                            checked_tag_stack_push!(stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_AFTER_UNION_RIGHT as c_int,
                            ));

                            checked_tag_stack_push!(stack::tre_stack_push_voidptr(
                                stack,
                                right as *mut c_void
                            ));
                            checked_tag_stack_push!(stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int,
                            ));

                            checked_tag_stack_push!(stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_AFTER_UNION_LEFT as c_int,
                            ));

                            checked_tag_stack_push!(stack::tre_stack_push_voidptr(
                                stack,
                                left as *mut c_void
                            ));
                            checked_tag_stack_push!(stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int,
                            ));

                            if *regset >= 0 {
                                if !first_pass {
                                    status = tre_add_tag_left(mem, node, tag);
                                    *(*tnfa).tag_directions.offset(tag as isize) =
                                        direction as c_int;
                                    if minimal_tag >= 0 {
                                        let mut i = 0;
                                        while *(*tnfa).minimal_tags.offset(i) >= 0 {
                                            i += 1;
                                        }
                                        *(*tnfa).minimal_tags.offset(i) = tag;
                                        *(*tnfa).minimal_tags.offset(i + 1) = minimal_tag;
                                        *(*tnfa).minimal_tags.offset(i + 2) = -1;
                                        minimal_tag = -1;
                                        num_minimals += 1;
                                    }
                                    tre_purge_regset(regset, tnfa, tag);
                                }
                                *regset = -1;
                                tag = next_tag;
                                num_tags += 1;
                                next_tag += 1;
                            }

                            if (*node).num_submatches > 0 {
                                next_tag += 1;
                                tag = next_tag;
                                next_tag += 1;
                            }
                        }
                    }

                    if (*node).submatch_id >= 0 {
                        let mut i = 0;
                        while *parents.offset(i) >= 0 {
                            i += 1;
                        }
                        *parents.offset(i) = (*node).submatch_id;
                        *parents.offset(i + 1) = -1;
                    }
                }
                x if x == tre_addtags_symbol_t::ADDTAGS_AFTER_ITERATION as c_int => {
                    node = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;
                    if first_pass {
                        let val = stack::tre_stack_pop_int(stack);
                        (*node).num_tags = (*((*node).obj as *mut tre_iteration_t))
                            .arg
                            .as_ref()
                            .map_or(0, |a| a.num_tags)
                            + val;
                        minimal_tag = -1;
                    } else {
                        let minimal = stack::tre_stack_pop_int(stack);
                        let enter_tag = stack::tre_stack_pop_int(stack);
                        if minimal != 0 {
                            minimal_tag = enter_tag;
                        }
                        direction = if minimal != 0 {
                            tre_tag_direction_t::TRE_TAG_MINIMIZE
                        } else {
                            tre_tag_direction_t::TRE_TAG_MAXIMIZE
                        };
                    }
                }
                x if x == tre_addtags_symbol_t::ADDTAGS_AFTER_CAT_LEFT as c_int => {
                    let new_tag = stack::tre_stack_pop_int(stack);
                    next_tag = stack::tre_stack_pop_int(stack);
                    if new_tag >= 0 {
                        tag = new_tag;
                    }
                }
                x if x == tre_addtags_symbol_t::ADDTAGS_AFTER_CAT_RIGHT as c_int => {
                    node = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;
                    if first_pass {
                        (*node).num_tags = (*((*node).obj as *mut tre_catenation_t))
                            .left
                            .as_ref()
                            .map_or(0, |l| l.num_tags)
                            + (*((*node).obj as *mut tre_catenation_t))
                                .right
                                .as_ref()
                                .map_or(0, |r| r.num_tags);
                    }
                }
                x if x == tre_addtags_symbol_t::ADDTAGS_AFTER_UNION_LEFT as c_int => {
                    while *regset >= 0 {
                        regset = regset.add(1);
                    }
                }
                x if x == tre_addtags_symbol_t::ADDTAGS_AFTER_UNION_RIGHT as c_int => {
                    let _left = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;
                    let _right = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;
                    node = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;
                    let added_tags = stack::tre_stack_pop_int(stack);
                    if first_pass {
                        (*node).num_tags = (*((*node).obj as *mut tre_union_t))
                            .left
                            .as_ref()
                            .map_or(0, |l| l.num_tags)
                            + (*((*node).obj as *mut tre_union_t))
                                .right
                                .as_ref()
                                .map_or(0, |r| r.num_tags)
                            + added_tags
                            + if (*node).num_submatches > 0 { 2 } else { 0 };
                    }
                    regset = stack::tre_stack_pop_voidptr(stack) as *mut c_int;
                    let _tag_left = stack::tre_stack_pop_int(stack);
                    let _tag_right = stack::tre_stack_pop_int(stack);

                    if (*node).num_submatches > 0 {
                        if !first_pass {
                            status = tre_add_tag_right(mem, _left, _tag_left);
                            *(*tnfa).tag_directions.offset(_tag_left as isize) =
                                tre_tag_direction_t::TRE_TAG_MAXIMIZE as c_int;
                            status = tre_add_tag_right(mem, _right, _tag_right);
                            *(*tnfa).tag_directions.offset(_tag_right as isize) =
                                tre_tag_direction_t::TRE_TAG_MAXIMIZE as c_int;
                        }
                        num_tags += 2;
                    }
                    direction = tre_tag_direction_t::TRE_TAG_MAXIMIZE;
                }
                _ => {}
            }
        }

        if !first_pass {
            tre_purge_regset(regset, tnfa, tag);
        }

        if !first_pass && minimal_tag >= 0 {
            let mut i = 0;
            while *(*tnfa).minimal_tags.offset(i) >= 0 {
                i += 1;
            }
            *(*tnfa).minimal_tags.offset(i) = tag;
            *(*tnfa).minimal_tags.offset(i + 1) = minimal_tag;
            *(*tnfa).minimal_tags.offset(i + 2) = -1;
            minimal_tag = -1;
            num_minimals += 1;
        }

        (*tnfa).end_tag = num_tags;
        (*tnfa).num_tags = num_tags;
        (*tnfa).num_minimals = num_minimals;
        mem::xfree(orig_regset as *mut c_void);
        mem::xfree(parents as *mut c_void);
        mem::xfree(saved_states as *mut c_void);
        status
    }
}

// ===== AST expansion =====

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum tre_expand_ast_symbol_t {
    EXPAND_RECURSE,
    EXPAND_AFTER_ITER,
}

const COPY_REMOVE_TAGS: c_int = 1;
const COPY_MAXIMIZE_FIRST_TAG: c_int = 2;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum tre_copyast_symbol_t {
    COPY_RECURSE,
    COPY_SET_RESULT_PTR,
}

unsafe fn tre_copy_ast(
    mem: mem::tre_mem_t,
    stack: *mut stack::tre_stack_rec,
    ast: *mut tre_ast_node_t,
    flags: c_int,
    pos_add: *mut c_int,
    tag_directions: *mut c_int,
    copy: *mut *mut tre_ast_node_t,
    max_pos: *mut c_int,
) -> c_int {
    unsafe {
        let mut status: c_int = REG_OK as c_int;
        let bottom = stack::tre_stack_num_objects(stack);
        let mut num_copied: c_int = 0;
        let mut first_tag: c_int = 1;
        let mut result = copy;

        checked_stack_push!(stack::tre_stack_push_voidptr(stack, ast as *mut c_void));
        checked_stack_push!(stack::tre_stack_push_int(
            stack,
            tre_copyast_symbol_t::COPY_RECURSE as c_int
        ));

        while status == REG_OK as c_int && stack::tre_stack_num_objects(stack) > bottom {
            let symbol = stack::tre_stack_pop_int(stack);
            match symbol {
                x if x == tre_copyast_symbol_t::COPY_SET_RESULT_PTR as c_int => {
                    result = stack::tre_stack_pop_voidptr(stack) as *mut *mut tre_ast_node_t;
                }
                x if x == tre_copyast_symbol_t::COPY_RECURSE as c_int => {
                    let node = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;
                    match (*node).type_ {
                        tre_ast_type_t::LITERAL => {
                            let lit = (*node).obj as *mut tre_literal_t;
                            let mut pos = (*lit).position;
                            let min = (*lit).code_min as c_int;
                            let max = (*lit).code_max as c_int;
                            if !IS_SPECIAL(&*lit) || IS_BACKREF(&*lit) {
                                pos += *pos_add;
                                num_copied += 1;
                            } else if IS_TAG(&*lit) && (flags & COPY_REMOVE_TAGS) != 0 {
                                // Change to empty
                            } else if IS_TAG(&*lit)
                                && (flags & COPY_MAXIMIZE_FIRST_TAG) != 0
                                && first_tag != 0
                            {
                                if !tag_directions.is_null() {
                                    *tag_directions.offset(max as isize) =
                                        tre_tag_direction_t::TRE_TAG_MAXIMIZE as c_int;
                                }
                                first_tag = 0;
                            }
                            *result = tre_ast_new_literal(mem, min, max, pos);
                            if (*result).is_null() {
                                status = REG_ESPACE as c_int;
                            } else {
                                (*((**result).obj as *mut tre_literal_t))
                                    .set_class((*lit).get_class());
                                (*((**result).obj as *mut tre_literal_t))
                                    .set_params((*lit).get_params());
                                if pos > *max_pos {
                                    *max_pos = pos;
                                }
                            }
                        }
                        tre_ast_type_t::UNION => {
                            let uni = (*node).obj as *mut tre_union_t;
                            *result = tre_ast_new_union(mem, (*uni).left, (*uni).right);
                            if (*result).is_null() {
                                status = REG_ESPACE as c_int;
                            } else {
                                let tmp = (**result).obj as *mut tre_union_t;
                                result = &mut (*tmp).left;
                                checked_stack_push!(stack::tre_stack_push_voidptr(
                                    stack,
                                    (*uni).right as *mut c_void
                                ));
                                checked_stack_push!(stack::tre_stack_push_int(
                                    stack,
                                    tre_copyast_symbol_t::COPY_RECURSE as c_int,
                                ));
                                let right_ptr: *mut *mut tre_ast_node_t =
                                    std::ptr::addr_of_mut!((*tmp).right);
                                checked_stack_push!(stack::tre_stack_push_voidptr(
                                    stack,
                                    right_ptr as *mut c_void
                                ));
                                checked_stack_push!(stack::tre_stack_push_int(
                                    stack,
                                    tre_copyast_symbol_t::COPY_SET_RESULT_PTR as c_int,
                                ));
                                checked_stack_push!(stack::tre_stack_push_voidptr(
                                    stack,
                                    (*uni).left as *mut c_void
                                ));
                                checked_stack_push!(stack::tre_stack_push_int(
                                    stack,
                                    tre_copyast_symbol_t::COPY_RECURSE as c_int,
                                ));
                            }
                        }
                        tre_ast_type_t::CATENATION => {
                            let cat = (*node).obj as *mut tre_catenation_t;
                            *result = tre_ast_new_catenation(mem, (*cat).left, (*cat).right);
                            if (*result).is_null() {
                                status = REG_ESPACE as c_int;
                            } else {
                                let tmp = (**result).obj as *mut tre_catenation_t;
                                (*tmp).left = ptr::null_mut();
                                (*tmp).right = ptr::null_mut();
                                result = &mut (*tmp).left;
                                checked_stack_push!(stack::tre_stack_push_voidptr(
                                    stack,
                                    (*cat).right as *mut c_void
                                ));
                                checked_stack_push!(stack::tre_stack_push_int(
                                    stack,
                                    tre_copyast_symbol_t::COPY_RECURSE as c_int,
                                ));
                                let right_ptr2: *mut *mut tre_ast_node_t =
                                    std::ptr::addr_of_mut!((*tmp).right);
                                checked_stack_push!(stack::tre_stack_push_voidptr(
                                    stack,
                                    right_ptr2 as *mut c_void
                                ));
                                checked_stack_push!(stack::tre_stack_push_int(
                                    stack,
                                    tre_copyast_symbol_t::COPY_SET_RESULT_PTR as c_int,
                                ));
                                checked_stack_push!(stack::tre_stack_push_voidptr(
                                    stack,
                                    (*cat).left as *mut c_void
                                ));
                                checked_stack_push!(stack::tre_stack_push_int(
                                    stack,
                                    tre_copyast_symbol_t::COPY_RECURSE as c_int,
                                ));
                            }
                        }
                        tre_ast_type_t::ITERATION => {
                            let iter = (*node).obj as *mut tre_iteration_t;
                            checked_stack_push!(stack::tre_stack_push_voidptr(
                                stack,
                                (*iter).arg as *mut c_void
                            ));
                            checked_stack_push!(stack::tre_stack_push_int(
                                stack,
                                tre_copyast_symbol_t::COPY_RECURSE as c_int,
                            ));
                            *result = tre_ast_new_iter(
                                mem,
                                (*iter).arg,
                                (*iter).min,
                                (*iter).max,
                                (*iter).minimal as c_int,
                            );
                            if (*result).is_null() {
                                status = REG_ESPACE as c_int;
                            } else {
                                let iter2 = (**result).obj as *mut tre_iteration_t;
                                result = &mut (*iter2).arg;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        *pos_add += num_copied;
        status
    }
}

unsafe fn tre_expand_ast(
    mem: mem::tre_mem_t,
    stack: *mut stack::tre_stack_rec,
    ast: *mut tre_ast_node_t,
    position: *mut c_int,
    tag_directions: *mut c_int,
    max_depth: *mut c_int,
) -> c_int {
    unsafe {
        let mut status: c_int = REG_OK as c_int;
        let bottom = stack::tre_stack_num_objects(stack);
        let mut pos_add: c_int = 0;
        let mut pos_add_total: c_int = 0;
        let mut max_pos: c_int = 0;
        let params: [c_int; TRE_PARAM_LAST] = [TRE_PARAM_DEFAULT; TRE_PARAM_LAST];
        let params_depth: c_int = 0;
        let mut iter_depth: c_int = 0;

        checked_stack_push!(stack::tre_stack_push_voidptr(stack, ast as *mut c_void));
        checked_stack_push!(stack::tre_stack_push_int(
            stack,
            tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int
        ));

        while status == REG_OK as c_int && stack::tre_stack_num_objects(stack) > bottom {
            let symbol = stack::tre_stack_pop_int(stack);
            let node = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;

            match symbol {
                x if x == tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int => match (*node).type_ {
                    tre_ast_type_t::LITERAL => {
                        let lit = (*node).obj as *mut tre_literal_t;
                        if !IS_SPECIAL(&*lit) || IS_BACKREF(&*lit) {
                            (*lit).position += pos_add;
                            if (*lit).position > max_pos {
                                max_pos = (*lit).position;
                            }
                        }
                    }
                    tre_ast_type_t::UNION => {
                        let uni = (*node).obj as *mut tre_union_t;
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*uni).right as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int,
                        ));
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*uni).left as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int,
                        ));
                    }
                    tre_ast_type_t::CATENATION => {
                        let cat = (*node).obj as *mut tre_catenation_t;
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*cat).right as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int,
                        ));
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*cat).left as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int,
                        ));
                    }
                    tre_ast_type_t::ITERATION => {
                        let iter = (*node).obj as *mut tre_iteration_t;
                        checked_stack_push!(stack::tre_stack_push_int(stack, pos_add));
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            node as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_expand_ast_symbol_t::EXPAND_AFTER_ITER as c_int,
                        ));
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*iter).arg as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int,
                        ));
                        if (*iter).min > 1 || (*iter).max > 1 {
                            pos_add = 0;
                        }
                        iter_depth += 1;
                    }
                },
                x if x == tre_expand_ast_symbol_t::EXPAND_AFTER_ITER as c_int => {
                    let iter = (*node).obj as *mut tre_iteration_t;
                    let pos_add_last = stack::tre_stack_pop_int(stack);

                    if (*iter).min > 1 || (*iter).max > 1 {
                        let mut seq1: *mut tre_ast_node_t = ptr::null_mut();
                        let mut seq2: *mut tre_ast_node_t = ptr::null_mut();
                        let mut pos_add_save = pos_add;

                        for j in 0..(*iter).min {
                            let flags = if j + 1 < (*iter).min {
                                COPY_REMOVE_TAGS
                            } else {
                                COPY_MAXIMIZE_FIRST_TAG
                            };
                            pos_add_save = pos_add;
                            let mut copy: *mut tre_ast_node_t = ptr::null_mut();
                            status = tre_copy_ast(
                                mem,
                                stack,
                                (*iter).arg,
                                flags,
                                &mut pos_add,
                                tag_directions,
                                &mut copy,
                                &mut max_pos,
                            );
                            if status != REG_OK as c_int {
                                return status;
                            }
                            if !seq1.is_null() {
                                seq1 = tre_ast_new_catenation(mem, seq1, copy);
                            } else {
                                seq1 = copy;
                            }
                            if seq1.is_null() {
                                return REG_ESPACE as c_int;
                            }
                        }

                        if (*iter).max == -1 {
                            pos_add_save = pos_add;
                            let mut copy2: *mut tre_ast_node_t = ptr::null_mut();
                            status = tre_copy_ast(
                                mem,
                                stack,
                                (*iter).arg,
                                0,
                                &mut pos_add,
                                ptr::null_mut(),
                                &mut copy2,
                                &mut max_pos,
                            );
                            if status != REG_OK as c_int {
                                return status;
                            }
                            seq2 = tre_ast_new_iter(mem, copy2, 0, -1, 0);
                            if seq2.is_null() {
                                return REG_ESPACE as c_int;
                            }
                        } else {
                            for j in (*iter).min..(*iter).max {
                                let mut copy2: *mut tre_ast_node_t = ptr::null_mut();
                                pos_add_save = pos_add;
                                status = tre_copy_ast(
                                    mem,
                                    stack,
                                    (*iter).arg,
                                    0,
                                    &mut pos_add,
                                    ptr::null_mut(),
                                    &mut copy2,
                                    &mut max_pos,
                                );
                                if status != REG_OK as c_int {
                                    return status;
                                }
                                if !seq2.is_null() {
                                    seq2 = tre_ast_new_catenation(mem, copy2, seq2);
                                } else {
                                    seq2 = copy2;
                                }
                                if seq2.is_null() {
                                    return REG_ESPACE as c_int;
                                }
                                let tmp = tre_ast_new_literal(mem, EMPTY as c_int, -1, -1);
                                if tmp.is_null() {
                                    return REG_ESPACE as c_int;
                                }
                                seq2 = tre_ast_new_union(mem, tmp, seq2);
                                if seq2.is_null() {
                                    return REG_ESPACE as c_int;
                                }
                            }
                        }

                        pos_add = pos_add_save;
                        if seq1.is_null() {
                            seq1 = seq2;
                        } else if !seq2.is_null() {
                            seq1 = tre_ast_new_catenation(mem, seq1, seq2);
                        }
                        if seq1.is_null() {
                            return REG_ESPACE as c_int;
                        }
                        (*node).obj = (*seq1).obj;
                        (*node).type_ = (*seq1).type_;
                    }

                    iter_depth -= 1;
                    pos_add_total += pos_add - pos_add_last;
                    if iter_depth == 0 {
                        pos_add = pos_add_total;
                    }

                    // Approximate parameter handling simplified
                    if !(*iter).params.is_null() {
                        // Skip approximate parameter wrapping for now
                        // as it requires more complex AST manipulation
                    }
                }
                _ => {}
            }
        }

        *position += pos_add_total;
        if max_pos > *position {
            *position = max_pos;
        }
        status
    }
}

// ===== Set operations =====

unsafe fn tre_set_empty(mem: mem::tre_mem_t) -> *mut tre_pos_and_tags_t {
    unsafe {
        let new_set = mem::tre_mem_calloc(mem, std::mem::size_of::<tre_pos_and_tags_t>())
            as *mut tre_pos_and_tags_t;
        if new_set.is_null() {
            return ptr::null_mut();
        }
        (*new_set).position = -1;
        (*new_set).code_min = -1;
        (*new_set).code_max = -1;
        new_set
    }
}

unsafe fn tre_set_one(
    mem: mem::tre_mem_t,
    position: c_int,
    code_min: c_int,
    code_max: c_int,
    class: tre_ctype_t,
    neg_classes: *mut tre_ctype_t,
    backref: c_int,
) -> *mut tre_pos_and_tags_t {
    unsafe {
        let new_set = mem::tre_mem_calloc(mem, std::mem::size_of::<tre_pos_and_tags_t>() * 2)
            as *mut tre_pos_and_tags_t;
        if new_set.is_null() {
            return ptr::null_mut();
        }
        (*new_set).position = position;
        (*new_set).code_min = code_min;
        (*new_set).code_max = code_max;
        (*new_set).class = class;
        (*new_set).neg_classes = neg_classes;
        (*new_set).backref = backref;
        (*new_set.add(1)).position = -1;
        (*new_set.add(1)).code_min = -1;
        (*new_set.add(1)).code_max = -1;
        new_set
    }
}

unsafe fn tre_set_union(
    mem: mem::tre_mem_t,
    set1: *const tre_pos_and_tags_t,
    set2: *const tre_pos_and_tags_t,
    tags: *const c_int,
    assertions: c_int,
    params: *const c_int,
) -> *mut tre_pos_and_tags_t {
    unsafe {
        let mut num_tags: c_int = 0;
        if !tags.is_null() {
            let mut i: c_int = 0;
            while *tags.offset(i as isize) >= 0 {
                i += 1;
            }
            num_tags = i;
        }

        let mut s1: c_int = 0;
        while (*set1.offset(s1 as isize)).position >= 0 {
            s1 += 1;
        }
        let mut s2: c_int = 0;
        while (*set2.offset(s2 as isize)).position >= 0 {
            s2 += 1;
        }

        let new_set = mem::tre_mem_calloc(
            mem,
            std::mem::size_of::<tre_pos_and_tags_t>() * ((s1 + s2 + 1) as usize),
        ) as *mut tre_pos_and_tags_t;
        if new_set.is_null() {
            return ptr::null_mut();
        }

        // Copy set1 entries with tag and param merging
        for idx in 0..s1 as isize {
            (*new_set.offset(idx)).position = (*set1.offset(idx)).position;
            (*new_set.offset(idx)).code_min = (*set1.offset(idx)).code_min;
            (*new_set.offset(idx)).code_max = (*set1.offset(idx)).code_max;
            (*new_set.offset(idx)).assertions = (*set1.offset(idx)).assertions | assertions;
            (*new_set.offset(idx)).class = (*set1.offset(idx)).class;
            (*new_set.offset(idx)).neg_classes = (*set1.offset(idx)).neg_classes;
            (*new_set.offset(idx)).backref = (*set1.offset(idx)).backref;

            // Merge tags: combine existing entry tags with the `tags` parameter
            if (*set1.offset(idx)).tags.is_null() && tags.is_null() {
                (*new_set.offset(idx)).tags = ptr::null_mut();
            } else {
                let mut existing_count: c_int = 0;
                if !(*set1.offset(idx)).tags.is_null() {
                    while *(*set1.offset(idx)).tags.offset(existing_count as isize) >= 0 {
                        existing_count += 1;
                    }
                }
                let new_tags = mem::tre_mem_alloc(
                    mem,
                    std::mem::size_of::<c_int>() * (existing_count + num_tags + 1) as usize,
                ) as *mut c_int;
                if new_tags.is_null() {
                    return ptr::null_mut();
                }
                let mut j: c_int = 0;
                while j < existing_count {
                    *new_tags.offset(j as isize) = *(*set1.offset(idx)).tags.offset(j as isize);
                    j += 1;
                }
                for k in 0..num_tags {
                    *new_tags.offset((j + k) as isize) = *tags.offset(k as isize);
                }
                *new_tags.offset((j + num_tags) as isize) = -1;
                (*new_set.offset(idx)).tags = new_tags;
            }

            // Handle params
            if !(*set1.offset(idx)).params.is_null() {
                (*new_set.offset(idx)).params = (*set1.offset(idx)).params;
            }
            if !params.is_null() {
                if (*new_set.offset(idx)).params.is_null() {
                    (*new_set.offset(idx)).params = params as *mut c_int;
                } else {
                    let new_params =
                        mem::tre_mem_alloc(mem, std::mem::size_of::<c_int>() * TRE_PARAM_LAST)
                            as *mut c_int;
                    if new_params.is_null() {
                        return ptr::null_mut();
                    }
                    for k in 0..TRE_PARAM_LAST {
                        *new_params.add(k) = *(*new_set.offset(idx)).params.add(k);
                        if *params.add(k) != TRE_PARAM_UNSET {
                            *new_params.add(k) = *params.add(k);
                        }
                    }
                    (*new_set.offset(idx)).params = new_params;
                }
            }
        }

        // Copy set2 entries with tag copying
        for idx in 0..s2 as isize {
            (*new_set.offset(s1 as isize + idx)).position = (*set2.offset(idx)).position;
            (*new_set.offset(s1 as isize + idx)).code_min = (*set2.offset(idx)).code_min;
            (*new_set.offset(s1 as isize + idx)).code_max = (*set2.offset(idx)).code_max;
            (*new_set.offset(s1 as isize + idx)).assertions = (*set2.offset(idx)).assertions;
            (*new_set.offset(s1 as isize + idx)).class = (*set2.offset(idx)).class;
            (*new_set.offset(s1 as isize + idx)).neg_classes = (*set2.offset(idx)).neg_classes;
            (*new_set.offset(s1 as isize + idx)).backref = (*set2.offset(idx)).backref;

            // Deep copy tags for set2 entries
            if (*set2.offset(idx)).tags.is_null() {
                (*new_set.offset(s1 as isize + idx)).tags = ptr::null_mut();
            } else {
                let mut tag_count: c_int = 0;
                while *(*set2.offset(idx)).tags.offset(tag_count as isize) >= 0 {
                    tag_count += 1;
                }
                let new_tags = mem::tre_mem_alloc(
                    mem,
                    std::mem::size_of::<c_int>() * (tag_count + 1) as usize,
                ) as *mut c_int;
                if new_tags.is_null() {
                    return ptr::null_mut();
                }
                for k in 0..tag_count as isize {
                    *new_tags.offset(k) = *(*set2.offset(idx)).tags.offset(k);
                }
                *new_tags.offset(tag_count as isize) = -1;
                (*new_set.offset(s1 as isize + idx)).tags = new_tags;
            }

            // Handle params for set2
            if !(*set2.offset(idx)).params.is_null() {
                (*new_set.offset(s1 as isize + idx)).params = (*set2.offset(idx)).params;
            }
            if !params.is_null() {
                if (*new_set.offset(s1 as isize + idx)).params.is_null() {
                    (*new_set.offset(s1 as isize + idx)).params = params as *mut c_int;
                } else {
                    let new_params =
                        mem::tre_mem_alloc(mem, std::mem::size_of::<c_int>() * TRE_PARAM_LAST)
                            as *mut c_int;
                    if new_params.is_null() {
                        return ptr::null_mut();
                    }
                    for k in 0..TRE_PARAM_LAST {
                        *new_params.add(k) = *(*new_set.offset(s1 as isize + idx)).params.add(k);
                        if *params.add(k) != TRE_PARAM_UNSET {
                            *new_params.add(k) = *params.add(k);
                        }
                    }
                    (*new_set.offset(s1 as isize + idx)).params = new_params;
                }
            }
        }

        (*new_set.offset((s1 + s2) as isize)).position = -1;
        new_set
    }
}

// ===== Match empty =====

unsafe fn tre_match_empty(
    stack: *mut stack::tre_stack_rec,
    mut node: *mut tre_ast_node_t,
    tags: *mut c_int,
    assertions: *mut c_int,
    params: *mut c_int,
    num_tags_seen: *mut c_int,
    _params_seen: *mut c_int,
) -> c_int {
    unsafe {
        let bottom = stack::tre_stack_num_objects(stack);
        let status: c_int = REG_OK as c_int;

        if !num_tags_seen.is_null() {
            *num_tags_seen = 0;
        }
        if !_params_seen.is_null() {
            *_params_seen = 0;
        }

        checked_stack_push!(stack::tre_stack_push_voidptr(stack, node as *mut c_void));

        while status == REG_OK as c_int && stack::tre_stack_num_objects(stack) > bottom {
            node = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;
            match (*node).type_ {
                tre_ast_type_t::LITERAL => {
                    let lit = (*node).obj as *mut tre_literal_t;
                    match (*lit).code_min {
                        TAG => {
                            if (*lit).code_max >= 0 {
                                if !tags.is_null() {
                                    let mut i = 0;
                                    while *tags.offset(i) >= 0 {
                                        if *tags.offset(i) == (*lit).code_max as c_int {
                                            break;
                                        }
                                        i += 1;
                                    }
                                    if *tags.offset(i) < 0 {
                                        *tags.offset(i) = (*lit).code_max as c_int;
                                        *tags.offset(i + 1) = -1;
                                    }
                                }
                                if !num_tags_seen.is_null() {
                                    *num_tags_seen += 1;
                                }
                            }
                        }
                        ASSERTION => {
                            if !assertions.is_null() {
                                *assertions |= (*lit).code_max as c_int;
                            }
                        }
                        PARAMETER => {
                            if !params.is_null() {
                                let p = (*lit).get_params();
                                for i in 0..TRE_PARAM_LAST {
                                    *params.add(i) = *p.add(i);
                                }
                            }
                            if !_params_seen.is_null() {
                                *_params_seen = 1;
                            }
                        }
                        EMPTY => {}
                        _ => {}
                    }
                }
                tre_ast_type_t::UNION => {
                    let uni = (*node).obj as *mut tre_union_t;
                    if !(*uni).left.is_null() && (*(*uni).left).nullable != 0 {
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*uni).left as *mut c_void
                        ));
                    } else if !(*uni).right.is_null() && (*(*uni).right).nullable != 0 {
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*uni).right as *mut c_void
                        ));
                    }
                }
                tre_ast_type_t::CATENATION => {
                    let cat = (*node).obj as *mut tre_catenation_t;
                    checked_stack_push!(stack::tre_stack_push_voidptr(
                        stack,
                        (*cat).left as *mut c_void
                    ));
                    checked_stack_push!(stack::tre_stack_push_voidptr(
                        stack,
                        (*cat).right as *mut c_void
                    ));
                }
                tre_ast_type_t::ITERATION => {
                    let iter = (*node).obj as *mut tre_iteration_t;
                    if !(*iter).arg.is_null() && (*(*iter).arg).nullable != 0 {
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*iter).arg as *mut c_void
                        ));
                    }
                }
            }
        }
        status
    }
}

// ===== Compute NFL =====

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum tre_nfl_stack_symbol_t {
    NFL_RECURSE,
    NFL_POST_UNION,
    NFL_POST_CATENATION,
    NFL_POST_ITERATION,
}

unsafe fn tre_compute_nfl(
    mem: mem::tre_mem_t,
    stack: *mut stack::tre_stack_rec,
    tree: *mut tre_ast_node_t,
) -> c_int {
    unsafe {
        let bottom = stack::tre_stack_num_objects(stack);
        checked_stack_push!(stack::tre_stack_push_voidptr(stack, tree as *mut c_void));
        checked_stack_push!(stack::tre_stack_push_int(
            stack,
            tre_nfl_stack_symbol_t::NFL_RECURSE as c_int
        ));

        while stack::tre_stack_num_objects(stack) > bottom {
            let symbol = stack::tre_stack_pop_int(stack);
            let node = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;

            match symbol {
                x if x == tre_nfl_stack_symbol_t::NFL_RECURSE as c_int => match (*node).type_ {
                    tre_ast_type_t::LITERAL => {
                        let lit = (*node).obj as *mut tre_literal_t;
                        if IS_BACKREF(&*lit) {
                            (*node).nullable = 0;
                            (*node).firstpos = tre_set_one(
                                mem,
                                (*lit).position,
                                0,
                                TRE_CHAR_MAX as c_int,
                                0,
                                ptr::null_mut(),
                                -1,
                            );
                            if (*node).firstpos.is_null() {
                                return REG_ESPACE as c_int;
                            }
                            (*node).lastpos = tre_set_one(
                                mem,
                                (*lit).position,
                                0,
                                TRE_CHAR_MAX as c_int,
                                0,
                                ptr::null_mut(),
                                (*lit).code_max as c_int,
                            );
                            if (*node).lastpos.is_null() {
                                return REG_ESPACE as c_int;
                            }
                        } else if (*lit).code_min < 0 {
                            (*node).nullable = 1;
                            (*node).firstpos = tre_set_empty(mem);
                            if (*node).firstpos.is_null() {
                                return REG_ESPACE as c_int;
                            }
                            (*node).lastpos = tre_set_empty(mem);
                            if (*node).lastpos.is_null() {
                                return REG_ESPACE as c_int;
                            }
                        } else {
                            (*node).nullable = 0;
                            (*node).firstpos = tre_set_one(
                                mem,
                                (*lit).position,
                                (*lit).code_min as c_int,
                                (*lit).code_max as c_int,
                                0,
                                ptr::null_mut(),
                                -1,
                            );
                            if (*node).firstpos.is_null() {
                                return REG_ESPACE as c_int;
                            }
                            (*node).lastpos = tre_set_one(
                                mem,
                                (*lit).position,
                                (*lit).code_min as c_int,
                                (*lit).code_max as c_int,
                                (*lit).get_class(),
                                (*lit).neg_classes,
                                -1,
                            );
                            if (*node).lastpos.is_null() {
                                return REG_ESPACE as c_int;
                            }
                        }
                    }
                    tre_ast_type_t::UNION => {
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            node as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_POST_UNION as c_int,
                        ));
                        let uni = (*node).obj as *mut tre_union_t;
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*uni).right as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_RECURSE as c_int,
                        ));
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*uni).left as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_RECURSE as c_int,
                        ));
                    }
                    tre_ast_type_t::CATENATION => {
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            node as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_POST_CATENATION as c_int,
                        ));
                        let cat = (*node).obj as *mut tre_catenation_t;
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*cat).right as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_RECURSE as c_int,
                        ));
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*cat).left as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_RECURSE as c_int,
                        ));
                    }
                    tre_ast_type_t::ITERATION => {
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            node as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_POST_ITERATION as c_int,
                        ));
                        let iter = (*node).obj as *mut tre_iteration_t;
                        checked_stack_push!(stack::tre_stack_push_voidptr(
                            stack,
                            (*iter).arg as *mut c_void
                        ));
                        checked_stack_push!(stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_RECURSE as c_int,
                        ));
                    }
                },
                x if x == tre_nfl_stack_symbol_t::NFL_POST_UNION as c_int => {
                    let uni = (*node).obj as *mut tre_union_t;
                    (*node).nullable =
                        if (*(*uni).left).nullable != 0 || (*(*uni).right).nullable != 0 {
                            1
                        } else {
                            0
                        };
                    (*node).firstpos = tre_set_union(
                        mem,
                        (*(*uni).left).firstpos,
                        (*(*uni).right).firstpos,
                        ptr::null(),
                        0,
                        ptr::null(),
                    );
                    if (*node).firstpos.is_null() {
                        return REG_ESPACE as c_int;
                    }
                    (*node).lastpos = tre_set_union(
                        mem,
                        (*(*uni).left).lastpos,
                        (*(*uni).right).lastpos,
                        ptr::null(),
                        0,
                        ptr::null(),
                    );
                    if (*node).lastpos.is_null() {
                        return REG_ESPACE as c_int;
                    }
                }
                x if x == tre_nfl_stack_symbol_t::NFL_POST_ITERATION as c_int => {
                    let iter = (*node).obj as *mut tre_iteration_t;
                    if (*iter).min == 0 || (!(*iter).arg.is_null() && (*(*iter).arg).nullable != 0)
                    {
                        (*node).nullable = 1;
                    } else {
                        (*node).nullable = 0;
                    }
                    if !(*iter).arg.is_null() {
                        (*node).firstpos = (*(*iter).arg).firstpos;
                        (*node).lastpos = (*(*iter).arg).lastpos;
                    }
                }
                x if x == tre_nfl_stack_symbol_t::NFL_POST_CATENATION as c_int => {
                    let cat = (*node).obj as *mut tre_catenation_t;
                    (*node).nullable =
                        if (*(*cat).left).nullable != 0 && (*(*cat).right).nullable != 0 {
                            1
                        } else {
                            0
                        };

                    if (*(*cat).left).nullable != 0 {
                        let mut num_tags: c_int = 0;
                        let mut params_seen: c_int = 0;
                        let _status = tre_match_empty(
                            stack,
                            (*cat).left,
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            &mut num_tags,
                            &mut params_seen,
                        );
                        if _status != REG_OK as c_int {
                            return _status;
                        }
                        let tags =
                            mem::xmalloc(std::mem::size_of::<c_int>() * (num_tags as usize + 1))
                                as *mut c_int;
                        if tags.is_null() {
                            return REG_ESPACE as c_int;
                        }
                        *tags = -1;
                        let mut assertions: c_int = 0;
                        let _status = tre_match_empty(
                            stack,
                            (*cat).left,
                            tags,
                            &mut assertions,
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                        if _status != REG_OK as c_int {
                            mem::xfree(tags as *mut c_void);
                            return _status;
                        }
                        (*node).firstpos = tre_set_union(
                            mem,
                            (*(*cat).right).firstpos,
                            (*(*cat).left).firstpos,
                            tags,
                            assertions,
                            ptr::null(),
                        );
                        mem::xfree(tags as *mut c_void);
                        if (*node).firstpos.is_null() {
                            return REG_ESPACE as c_int;
                        }
                    } else {
                        (*node).firstpos = (*(*cat).left).firstpos;
                    }

                    if (*(*cat).right).nullable != 0 {
                        let mut num_tags: c_int = 0;
                        let mut params_seen: c_int = 0;
                        let _status = tre_match_empty(
                            stack,
                            (*cat).right,
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            &mut num_tags,
                            &mut params_seen,
                        );
                        if _status != REG_OK as c_int {
                            return _status;
                        }
                        let tags =
                            mem::xmalloc(std::mem::size_of::<c_int>() * (num_tags as usize + 1))
                                as *mut c_int;
                        if tags.is_null() {
                            return REG_ESPACE as c_int;
                        }
                        *tags = -1;
                        let mut assertions: c_int = 0;
                        let _status = tre_match_empty(
                            stack,
                            (*cat).right,
                            tags,
                            &mut assertions,
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                        if _status != REG_OK as c_int {
                            mem::xfree(tags as *mut c_void);
                            return _status;
                        }
                        (*node).lastpos = tre_set_union(
                            mem,
                            (*(*cat).left).lastpos,
                            (*(*cat).right).lastpos,
                            tags,
                            assertions,
                            ptr::null(),
                        );
                        mem::xfree(tags as *mut c_void);
                        if (*node).lastpos.is_null() {
                            return REG_ESPACE as c_int;
                        }
                    } else {
                        (*node).lastpos = (*(*cat).right).lastpos;
                    }
                }
                _ => {}
            }
        }
        REG_OK as c_int
    }
}

// ===== TNFA construction =====

unsafe fn tre_make_trans(
    p1: *const tre_pos_and_tags_t,
    p2: *const tre_pos_and_tags_t,
    transitions: *mut tre_tnfa_transition_t,
    counts: *mut c_int,
    offs: *const c_int,
    storage: &mut Option<&mut super::owned_arena::OwnedArena>,
) -> c_int {
    unsafe {
        if !transitions.is_null() {
            let mut pi: isize = 0;
            while (*p1.offset(pi)).position >= 0 {
                let mut pj: isize = 0;
                let mut prev_p2_pos: c_int = -1;
                while (*p2.offset(pj)).position >= 0 {
                    if (*p2.offset(pj)).position == prev_p2_pos {
                        pj += 1;
                        continue;
                    }
                    prev_p2_pos = (*p2.offset(pj)).position;

                    let trans = transitions
                        .offset(*offs.offset((*p1.offset(pi)).position as isize) as isize);
                    let mut trans = trans;
                    while !(*trans).state.is_null() {
                        trans = trans.add(1);
                    }

                    if (*trans).state.is_null() {
                        // Set terminator for next transition
                        let _ = trans;
                    }

                    let mut t = transitions
                        .offset(*offs.offset((*p1.offset(pi)).position as isize) as isize);
                    while !(*t).state.is_null() {
                        t = t.add(1);
                    }
                    (*t).code_min = (*p1.offset(pi)).code_min as tre_cint_t;
                    (*t).code_max = (*p1.offset(pi)).code_max as tre_cint_t;
                    (*t).state = transitions
                        .offset(*offs.offset((*p2.offset(pj)).position as isize) as isize);
                    (*t).state_id = (*p2.offset(pj)).position;
                    (*t).assertions = (*p1.offset(pi)).assertions | (*p2.offset(pj)).assertions;
                    if (*p1.offset(pi)).class != 0 {
                        (*t).assertions |= ASSERT_CHAR_CLASS;
                    }
                    if !(*p1.offset(pi)).neg_classes.is_null() {
                        (*t).assertions |= ASSERT_CHAR_CLASS_NEG;
                    }
                    if (*p1.offset(pi)).backref >= 0 {
                        (*t).u.set_backref((*p1.offset(pi)).backref);
                        (*t).assertions |= ASSERT_BACKREF;
                    } else {
                        (*t).u.set_class((*p1.offset(pi)).class);
                    }
                    (*t).neg_classes = (*p1.offset(pi)).neg_classes;
                    (*t).tags = ptr::null_mut();
                    (*t).params = ptr::null_mut();

                    // Count tags
                    let mut i: c_int = 0;
                    if !(*p1.offset(pi)).tags.is_null() {
                        while *(*p1.offset(pi)).tags.offset(i as isize) >= 0 {
                            i += 1;
                        }
                    }
                    let mut j: c_int = 0;
                    if !(*p2.offset(pj)).tags.is_null() {
                        while *(*p2.offset(pj)).tags.offset(j as isize) >= 0 {
                            j += 1;
                        }
                    }

                    if i > 0 || j > 0 {
                        let total = match usize::try_from(i)
                            .ok()
                            .zip(usize::try_from(j).ok())
                            .and_then(|(i, j)| i.checked_add(j))
                            .and_then(|n| n.checked_add(1))
                        {
                            Some(total) => total,
                            None => return REG_ESPACE,
                        };
                        (*t).tags = allocate_retained_ints(total, false, storage);
                        if (*t).tags.is_null() {
                            return REG_ESPACE;
                        } else {
                            let mut k: c_int = 0;
                            if !(*p1.offset(pi)).tags.is_null() {
                                while *(*p1.offset(pi)).tags.offset(k as isize) >= 0 {
                                    *(*t).tags.offset(k as isize) =
                                        *(*p1.offset(pi)).tags.offset(k as isize);
                                    k += 1;
                                }
                            }
                            let mut l = k;
                            let mut j2: c_int = 0;
                            if !(*p2.offset(pj)).tags.is_null() {
                                while *(*p2.offset(pj)).tags.offset(j2 as isize) >= 0 {
                                    let mut dup = 0;
                                    let mut k2 = 0;
                                    while k2 < k {
                                        if *(*t).tags.offset(k2 as isize)
                                            == *(*p2.offset(pj)).tags.offset(j2 as isize)
                                        {
                                            dup = 1;
                                            break;
                                        }
                                        k2 += 1;
                                    }
                                    if dup == 0 {
                                        *(*t).tags.offset(l as isize) =
                                            *(*p2.offset(pj)).tags.offset(j2 as isize);
                                        l += 1;
                                    }
                                    j2 += 1;
                                }
                            }
                            *(*t).tags.offset(l as isize) = -1;
                        }
                    }

                    if !(*p1.offset(pi)).params.is_null() || !(*p2.offset(pj)).params.is_null() {
                        (*t).params = allocate_retained_ints(TRE_PARAM_LAST, false, storage);
                        if (*t).params.is_null() {
                            return REG_ESPACE;
                        } else {
                            for k in 0..TRE_PARAM_LAST {
                                *(*t).params.add(k) = TRE_PARAM_UNSET;
                                if !(*p1.offset(pi)).params.is_null()
                                    && *(*p1.offset(pi)).params.add(k) != TRE_PARAM_UNSET
                                {
                                    *(*t).params.add(k) = *(*p1.offset(pi)).params.add(k);
                                }
                                if !(*p2.offset(pj)).params.is_null()
                                    && *(*p2.offset(pj)).params.add(k) != TRE_PARAM_UNSET
                                {
                                    *(*t).params.add(k) = *(*p2.offset(pj)).params.add(k);
                                }
                            }
                        }
                    }

                    pj += 1;
                }
                pi += 1;
            }
        } else {
            let mut pi: isize = 0;
            while (*p1.offset(pi)).position >= 0 {
                let mut pj: isize = 0;
                while (*p2.offset(pj)).position >= 0 {
                    *counts.offset((*p1.offset(pi)).position as isize) += 1;
                    pj += 1;
                }
                pi += 1;
            }
        }
        REG_OK as c_int
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum tre_tnfa_symbol_t {
    TNFA_RECURSE,
}

unsafe fn tre_ast_to_tnfa_iter(
    stack: *mut stack::tre_stack_rec,
    node: *mut tre_ast_node_t,
    transitions: *mut tre_tnfa_transition_t,
    counts: *mut c_int,
    offs: *const c_int,
    storage: &mut Option<&mut super::owned_arena::OwnedArena>,
) -> c_int {
    unsafe {
        checked_stack_push!(stack::tre_stack_push_voidptr(stack, node as *mut c_void));

        while stack::tre_stack_num_objects(stack) > 0 {
            let node = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;
            match (*node).type_ {
                tre_ast_type_t::LITERAL => {}
                tre_ast_type_t::UNION => {
                    let uni = (*node).obj as *mut tre_union_t;
                    checked_stack_push!(stack::tre_stack_push_voidptr(
                        stack,
                        (*uni).right as *mut c_void
                    ));
                    checked_stack_push!(stack::tre_stack_push_voidptr(
                        stack,
                        (*uni).left as *mut c_void
                    ));
                }
                tre_ast_type_t::CATENATION => {
                    let cat = (*node).obj as *mut tre_catenation_t;
                    let errcode = tre_make_trans(
                        (*cat).left.as_ref().map_or(ptr::null(), |l| l.lastpos),
                        (*cat).right.as_ref().map_or(ptr::null(), |r| r.firstpos),
                        transitions,
                        counts,
                        offs,
                        storage,
                    );
                    if errcode != REG_OK as c_int {
                        return errcode;
                    }
                    checked_stack_push!(stack::tre_stack_push_voidptr(
                        stack,
                        (*cat).right as *mut c_void
                    ));
                    checked_stack_push!(stack::tre_stack_push_voidptr(
                        stack,
                        (*cat).left as *mut c_void
                    ));
                }
                tre_ast_type_t::ITERATION => {
                    let iter = (*node).obj as *mut tre_iteration_t;
                    if (*iter).max == -1 {
                        let errcode = tre_make_trans(
                            (*iter).arg.as_ref().map_or(ptr::null(), |a| a.lastpos),
                            (*iter).arg.as_ref().map_or(ptr::null(), |a| a.firstpos),
                            transitions,
                            counts,
                            offs,
                            storage,
                        );
                        if errcode != REG_OK as c_int {
                            return errcode;
                        }
                    }
                    checked_stack_push!(stack::tre_stack_push_voidptr(
                        stack,
                        (*iter).arg as *mut c_void
                    ));
                }
            }
        }
        REG_OK as c_int
    }
}

unsafe fn tre_ast_to_tnfa(
    node: *mut tre_ast_node_t,
    transitions: *mut tre_tnfa_transition_t,
    counts: *mut c_int,
    offs: *const c_int,
    storage: &mut Option<&mut super::owned_arena::OwnedArena>,
) -> c_int {
    unsafe {
        let stack = stack::tre_stack_new(1024, 256 * 1024, 4096);
        if stack.is_null() {
            return REG_ESPACE as c_int;
        }
        let errcode = tre_ast_to_tnfa_iter(stack, node, transitions, counts, offs, storage);
        stack::tre_stack_destroy(stack);
        errcode
    }
}

// ===== Main compile function =====

pub unsafe fn tre_compile(
    preg: *mut regex_t,
    regex: *const tre_char_t,
    n: usize,
    cflags: c_int,
) -> c_int {
    unsafe {
        tre_compile_with_workspace(
            preg,
            regex,
            n,
            cflags,
            crate::mainutils::grep::RegexWorkspace::unbounded(),
        )
    }
}

pub(crate) unsafe fn tre_compile_with_workspace(
    preg: *mut regex_t,
    regex: *const tre_char_t,
    n: usize,
    cflags: c_int,
    workspace: crate::mainutils::grep::RegexWorkspace,
) -> c_int {
    unsafe { tre_compile_tables(preg, regex, n, cflags, workspace, None) }
}

/// Opaque runtime owner: nested legacy allocations are freed while descriptor
/// and table cells remain live, then Rust drops their original-owner storage.
pub(crate) struct CompiledTre {
    preg: regex_t,
    _tables: super::owned_arena::OwnedArena,
}

impl CompiledTre {
    pub(crate) fn regex(&self) -> &regex_t {
        &self.preg
    }
}

impl Drop for CompiledTre {
    fn drop(&mut self) {
        unsafe {
            tre_free_tables(&mut self.preg, true);
        }
    }
}

pub(crate) unsafe fn tre_compile_owned(
    regex: *const tre_char_t,
    n: usize,
    cflags: c_int,
    workspace: crate::mainutils::grep::RegexWorkspace,
) -> Result<CompiledTre, c_int> {
    let mut tables =
        super::owned_arena::OwnedArena::new(workspace.clone()).map_err(|()| REG_ESPACE)?;
    let mut preg = regex_t {
        re_nsub: 0,
        value: ptr::null_mut(),
    };
    let status =
        unsafe { tre_compile_tables(&mut preg, regex, n, cflags, workspace, Some(&mut tables)) };
    if status == REG_OK {
        Ok(CompiledTre {
            preg,
            _tables: tables,
        })
    } else {
        Err(status)
    }
}

unsafe fn allocate_transition_table(
    count: c_int,
    storage: &mut Option<&mut super::owned_arena::OwnedArena>,
) -> *mut tre_tnfa_transition_t {
    let Some(count) = usize::try_from(count).ok().and_then(|n| n.checked_add(1)) else {
        return ptr::null_mut();
    };
    let Some(bytes) = count.checked_mul(std::mem::size_of::<tre_tnfa_transition_t>()) else {
        return ptr::null_mut();
    };
    if let Some(storage) = storage {
        storage
            .allocate_exact(bytes)
            .map_or(ptr::null_mut(), |p| p.cast())
    } else {
        unsafe { mem::xcalloc(count, std::mem::size_of::<tre_tnfa_transition_t>()).cast() }
    }
}

unsafe fn allocate_tnfa(
    storage: &mut Option<&mut super::owned_arena::OwnedArena>,
) -> *mut tre_tnfa_t {
    if let Some(storage) = storage {
        // Exact retained cells are zeroed and 16-byte aligned, as required by
        // this pointer/integer descriptor. Its arena outlives nested cleanup.
        storage
            .allocate_exact(std::mem::size_of::<tre_tnfa_t>())
            .map_or(ptr::null_mut(), |pointer| pointer.cast())
    } else {
        unsafe { mem::xcalloc(1, std::mem::size_of::<tre_tnfa_t>()).cast() }
    }
}

unsafe fn allocate_retained_ints(
    count: usize,
    zeroed: bool,
    storage: &mut Option<&mut super::owned_arena::OwnedArena>,
) -> *mut c_int {
    let Some(bytes) = count.checked_mul(std::mem::size_of::<c_int>()) else {
        return ptr::null_mut();
    };
    if bytes == 0 {
        return ptr::null_mut();
    }
    if let Some(storage) = storage {
        storage
            .allocate_exact(bytes)
            .map_or(ptr::null_mut(), |p| p.cast())
    } else if zeroed {
        unsafe { mem::xcalloc(count, std::mem::size_of::<c_int>()).cast() }
    } else {
        unsafe { mem::xmalloc(bytes).cast() }
    }
}

unsafe fn allocate_submatch_table(
    count: usize,
    storage: &mut Option<&mut super::owned_arena::OwnedArena>,
) -> *mut tre_submatch_data_t {
    let Some(bytes) = count.checked_mul(std::mem::size_of::<tre_submatch_data_t>()) else {
        return ptr::null_mut();
    };
    if bytes == 0 {
        return ptr::null_mut();
    }
    if let Some(storage) = storage {
        storage
            .allocate_exact(bytes)
            .map_or(ptr::null_mut(), |p| p.cast())
    } else {
        unsafe { mem::xcalloc(count, std::mem::size_of::<tre_submatch_data_t>()).cast() }
    }
}

unsafe fn tre_compile_tables(
    preg: *mut regex_t,
    regex: *const tre_char_t,
    n: usize,
    cflags: c_int,
    workspace: crate::mainutils::grep::RegexWorkspace,
    mut storage: Option<&mut super::owned_arena::OwnedArena>,
) -> c_int {
    let owned_tables = storage.is_some();
    unsafe {
        (*preg).value = ptr::null_mut();
        (*preg).re_nsub = 0;
        let mut parse_ctx: tre_parse_ctx_t = std::mem::zeroed();
        let mut counts: *mut c_int = ptr::null_mut();
        let mut offs: *mut c_int = ptr::null_mut();

        let stack = stack::tre_stack_new(512, 10240, 128);
        if stack.is_null() {
            return REG_ESPACE as c_int;
        }
        let Ok(mut arena) = mem::tre_mem_struct::new(workspace) else {
            stack::tre_stack_destroy(stack);
            return REG_ESPACE as c_int;
        };
        let mem = &mut arena as mem::tre_mem_t;

        parse_ctx.mem = mem;
        parse_ctx.stack = stack;
        parse_ctx.re = regex;
        parse_ctx.len = n;
        parse_ctx.cflags = cflags;
        parse_ctx.max_backref = -1;
        parse_ctx.cur_max = 1; // Simplified: always use byte mode

        let errcode = super::parse::tre_parse(&mut parse_ctx);
        if errcode != REG_OK as c_int {
            return compile_error(
                mem,
                stack,
                counts,
                offs,
                preg,
                ptr::null_mut(),
                errcode,
                owned_tables,
            );
        }

        (*preg).re_nsub = if parse_ctx.submatch_id > 0 {
            (parse_ctx.submatch_id - 1) as usize
        } else {
            0
        };
        let tree = parse_ctx.result;

        // Allocate TNFA
        let tnfa = allocate_tnfa(&mut storage);
        if tnfa.is_null() {
            return compile_error(
                mem,
                stack,
                counts,
                offs,
                preg,
                ptr::null_mut(),
                REG_ESPACE as c_int,
                owned_tables,
            );
        }
        (*tnfa).have_backrefs = if parse_ctx.max_backref >= 0 { 1 } else { 0 };
        (*tnfa).have_approx = parse_ctx.have_approx;
        (*tnfa).num_submatches = parse_ctx.submatch_id as u32;

        // Set up tags for submatch addressing
        if (*tnfa).have_backrefs != 0 || (cflags & REG_NOSUB) == 0 {
            let _errcode = tre_add_tags(ptr::null_mut(), stack, tree, tnfa, &mut storage);
            if _errcode != REG_OK {
                return compile_error(mem, stack, counts, offs, preg, tnfa, _errcode, owned_tables);
            }
            if (*tnfa).num_tags > 0 {
                let tag_directions = usize::try_from((*tnfa).num_tags)
                    .ok()
                    .and_then(|n| n.checked_add(1))
                    .map_or(ptr::null_mut(), |count| {
                        allocate_retained_ints(count, false, &mut storage)
                    });
                if tag_directions.is_null() {
                    return compile_error(
                        mem,
                        stack,
                        counts,
                        offs,
                        preg,
                        tnfa,
                        REG_ESPACE as c_int,
                        owned_tables,
                    );
                }
                (*tnfa).tag_directions = tag_directions;
                for i in 0..=(*tnfa).num_tags {
                    *tag_directions.offset(i as isize) = -1;
                }
            }
            (*tnfa).minimal_tags = usize::try_from((*tnfa).num_tags)
                .ok()
                .and_then(|n| n.checked_mul(2))
                .and_then(|n| n.checked_add(1))
                .map_or(ptr::null_mut(), |count| {
                    allocate_retained_ints(count, true, &mut storage)
                });
            if (*tnfa).minimal_tags.is_null() {
                return compile_error(
                    mem,
                    stack,
                    counts,
                    offs,
                    preg,
                    tnfa,
                    REG_ESPACE as c_int,
                    owned_tables,
                );
            }

            let submatch_data = usize::try_from(parse_ctx.submatch_id)
                .ok()
                .map_or(ptr::null_mut(), |count| {
                    allocate_submatch_table(count, &mut storage)
                });
            if submatch_data.is_null() {
                return compile_error(
                    mem,
                    stack,
                    counts,
                    offs,
                    preg,
                    tnfa,
                    REG_ESPACE as c_int,
                    owned_tables,
                );
            }
            (*tnfa).submatch_data = submatch_data;

            let _errcode = tre_add_tags(mem, stack, tree, tnfa, &mut storage);
            if _errcode != REG_OK {
                return compile_error(mem, stack, counts, offs, preg, tnfa, _errcode, owned_tables);
            }
        }

        // Expand iteration nodes
        let _errcode = tre_expand_ast(
            mem,
            stack,
            tree,
            &mut parse_ctx.position,
            (*tnfa).tag_directions,
            &mut (*tnfa).params_depth,
        );
        if _errcode != REG_OK {
            return compile_error(mem, stack, counts, offs, preg, tnfa, _errcode, owned_tables);
        }

        // Add dummy node for final state
        let tmp_ast_l = tree;
        let tmp_ast_r = tre_ast_new_literal(mem, 0, 0, parse_ctx.position);
        if tmp_ast_r.is_null() {
            return compile_error(
                mem,
                stack,
                counts,
                offs,
                preg,
                tnfa,
                REG_ESPACE as c_int,
                owned_tables,
            );
        }
        parse_ctx.position += 1;
        let tree = tre_ast_new_catenation(mem, tmp_ast_l, tmp_ast_r);
        if tree.is_null() {
            return compile_error(
                mem,
                stack,
                counts,
                offs,
                preg,
                tnfa,
                REG_ESPACE as c_int,
                owned_tables,
            );
        }

        let _errcode = tre_compute_nfl(mem, stack, tree);
        if _errcode != REG_OK as c_int {
            return compile_error(mem, stack, counts, offs, preg, tnfa, _errcode, owned_tables);
        }

        counts =
            mem::xmalloc(std::mem::size_of::<c_int>() * parse_ctx.position as usize) as *mut c_int;
        if counts.is_null() {
            return compile_error(
                mem,
                stack,
                counts,
                offs,
                preg,
                tnfa,
                REG_ESPACE as c_int,
                owned_tables,
            );
        }
        offs =
            mem::xmalloc(std::mem::size_of::<c_int>() * parse_ctx.position as usize) as *mut c_int;
        if offs.is_null() {
            return compile_error(
                mem,
                stack,
                counts,
                offs,
                preg,
                tnfa,
                REG_ESPACE as c_int,
                owned_tables,
            );
        }

        for i in 0..parse_ctx.position as isize {
            *counts.offset(i) = 0;
        }
        let _errcode = tre_ast_to_tnfa(tree, ptr::null_mut(), counts, ptr::null(), &mut storage);
        if _errcode != REG_OK {
            return compile_error(mem, stack, counts, offs, preg, tnfa, _errcode, owned_tables);
        }

        let mut add: c_int = 0;
        for i in 0..parse_ctx.position as isize {
            *offs.offset(i) = add;
            add += *counts.offset(i) + 1;
            *counts.offset(i) = 0;
        }

        let transitions = allocate_transition_table(add, &mut storage);
        if transitions.is_null() {
            return compile_error(
                mem,
                stack,
                counts,
                offs,
                preg,
                tnfa,
                REG_ESPACE as c_int,
                owned_tables,
            );
        }
        (*tnfa).transitions = transitions;
        (*tnfa).num_transitions = add as u32;

        let _errcode = tre_ast_to_tnfa(tree, transitions, counts, offs, &mut storage);
        if _errcode != REG_OK as c_int {
            return compile_error(mem, stack, counts, offs, preg, tnfa, _errcode, owned_tables);
        }

        (*tnfa).first_char = -1;
        (*tnfa).firstpos_chars = ptr::null_mut();

        // Build initial transitions
        let mut p = (*tree).firstpos;
        let mut i: c_int = 0;
        while !p.is_null() && (*p).position >= 0 {
            i += 1;
            p = p.add(1);
        }

        let initial = allocate_transition_table(i, &mut storage);
        if initial.is_null() {
            return compile_error(
                mem,
                stack,
                counts,
                offs,
                preg,
                tnfa,
                REG_ESPACE as c_int,
                owned_tables,
            );
        }
        (*tnfa).initial = initial;

        let mut i2: c_int = 0;
        p = (*tree).firstpos;
        while !p.is_null() && (*p).position >= 0 {
            (*initial.offset(i2 as isize)).state =
                transitions.offset(*offs.offset((*p).position as isize) as isize);
            (*initial.offset(i2 as isize)).state_id = (*p).position;
            (*initial.offset(i2 as isize)).tags = ptr::null_mut();
            (*initial.offset(i2 as isize)).params = ptr::null_mut();
            (*initial.offset(i2 as isize)).assertions = (*p).assertions;
            // Copy tags
            if !(*p).tags.is_null() {
                let mut j = 0;
                while *(*p).tags.offset(j as isize) >= 0 {
                    j += 1;
                }
                let tags = usize::try_from(j)
                    .ok()
                    .and_then(|n| n.checked_add(1))
                    .map_or(ptr::null_mut(), |count| {
                        allocate_retained_ints(count, false, &mut storage)
                    });
                if tags.is_null() {
                    return compile_error(
                        mem,
                        stack,
                        counts,
                        offs,
                        preg,
                        tnfa,
                        REG_ESPACE,
                        owned_tables,
                    );
                } else {
                    for k in 0..j as isize {
                        *tags.offset(k) = *(*p).tags.offset(k);
                    }
                    *tags.offset(j) = -1;
                    (*initial.offset(i2 as isize)).tags = tags;
                }
            }
            // Copy params
            if !(*p).params.is_null() {
                let params = allocate_retained_ints(TRE_PARAM_LAST, false, &mut storage);
                if params.is_null() {
                    return compile_error(
                        mem,
                        stack,
                        counts,
                        offs,
                        preg,
                        tnfa,
                        REG_ESPACE,
                        owned_tables,
                    );
                } else {
                    for k in 0..TRE_PARAM_LAST {
                        *params.add(k) = *(*p).params.add(k);
                    }
                    (*initial.offset(i2 as isize)).params = params;
                }
            }
            i2 += 1;
            p = p.add(1);
        }
        (*initial.offset(i2 as isize)).state = ptr::null_mut();

        (*tnfa).num_transitions = add as u32;
        (*tnfa).final_ =
            transitions.offset(*offs.offset((*(*tree).lastpos).position as isize) as isize);
        (*tnfa).num_states = parse_ctx.position;
        (*tnfa).cflags = cflags;

        (*mem).clear();
        stack::tre_stack_destroy(stack);
        mem::xfree(counts as *mut c_void);
        mem::xfree(offs as *mut c_void);

        (*preg).value = tnfa as *mut c_void;
        REG_OK as c_int
    }
}

#[allow(clippy::too_many_arguments)] // Legacy cleanup now carries explicit table ownership.
unsafe fn compile_error(
    mem: mem::tre_mem_t,
    stack: *mut stack::tre_stack_rec,
    counts: *mut c_int,
    offs: *mut c_int,
    preg: *mut regex_t,
    tnfa: *mut tre_tnfa_t,
    errcode: c_int,
    owned_tables: bool,
) -> c_int {
    unsafe {
        if !mem.is_null() {
            (*mem).clear();
        }
        if !stack.is_null() {
            stack::tre_stack_destroy(stack);
        }
        if !counts.is_null() {
            mem::xfree(counts as *mut c_void);
        }
        if !offs.is_null() {
            mem::xfree(offs as *mut c_void);
        }
        (*preg).value = tnfa as *mut c_void;
        tre_free_tables(preg, owned_tables);
        errcode
    }
}

pub unsafe fn tre_free(preg: *mut regex_t) {
    unsafe {
        tre_free_tables(preg, false);
    }
}

unsafe fn tre_free_tables(preg: *mut regex_t, owned_tables: bool) {
    unsafe {
        if preg.is_null() {
            return;
        }
        let tnfa = (*preg).value as *mut tre_tnfa_t;
        (*preg).value = ptr::null_mut();
        (*preg).re_nsub = 0;
        if tnfa.is_null() {
            return;
        }

        let mut i: u32 = 0;
        while i < (*tnfa).num_transitions {
            let trans = (*tnfa).transitions.offset(i as isize);
            if !(*trans).state.is_null() {
                if !owned_tables && !(*trans).tags.is_null() {
                    mem::xfree((*trans).tags as *mut c_void);
                }
                if !(*trans).neg_classes.is_null() {
                    mem::xfree((*trans).neg_classes as *mut c_void);
                }
                if !owned_tables && !(*trans).params.is_null() {
                    mem::xfree((*trans).params as *mut c_void);
                }
            }
            i += 1;
        }
        if !owned_tables && !(*tnfa).transitions.is_null() {
            mem::xfree((*tnfa).transitions as *mut c_void);
        }

        if !(*tnfa).initial.is_null() {
            let mut trans = (*tnfa).initial;
            while !(*trans).state.is_null() {
                if !owned_tables && !(*trans).tags.is_null() {
                    mem::xfree((*trans).tags as *mut c_void);
                }
                if !owned_tables && !(*trans).params.is_null() {
                    mem::xfree((*trans).params as *mut c_void);
                }
                trans = trans.add(1);
            }
            if !owned_tables {
                mem::xfree((*tnfa).initial as *mut c_void);
            }
        }

        // Owned submatch cells and parent arrays drop together after this
        // legacy cleanup. Standalone compilation retains its malloc/free path.
        if !owned_tables && !(*tnfa).submatch_data.is_null() {
            let mut i: u32 = 0;
            while i < (*tnfa).num_submatches {
                if !(*(*tnfa).submatch_data.offset(i as isize))
                    .parents
                    .is_null()
                {
                    mem::xfree((*(*tnfa).submatch_data.offset(i as isize)).parents as *mut c_void);
                }
                i += 1;
            }
            mem::xfree((*tnfa).submatch_data as *mut c_void);
        }

        if !owned_tables && !(*tnfa).tag_directions.is_null() {
            mem::xfree((*tnfa).tag_directions as *mut c_void);
        }
        if !(*tnfa).firstpos_chars.is_null() {
            mem::xfree((*tnfa).firstpos_chars as *mut c_void);
        }
        if !owned_tables && !(*tnfa).minimal_tags.is_null() {
            mem::xfree((*tnfa).minimal_tags as *mut c_void);
        }
        if !owned_tables {
            mem::xfree(tnfa as *mut c_void);
        }
    }
}

#[cfg(test)]
mod owned_table_tests {
    use super::*;

    #[test]
    fn retained_tre_submatch_parents_preserve_nested_capture_layout_and_cleanup() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        for (pattern, text) in [
            ("(a(b(c)?))", "abc"),
            ("(a(b)?)+", "aba"),
            ("((a|b)+?)", "aaab"),
        ] {
            let pattern: Vec<u32> = pattern.chars().map(u32::from).collect();
            let text: Vec<u32> = text.chars().map(u32::from).collect();
            let (_, report) = mem::fault::observe(None, || unsafe {
                let owned = tre_compile_owned(
                    pattern.as_ptr(),
                    pattern.len(),
                    REG_EXTENDED,
                    crate::mainutils::grep::regex_workspace(),
                )
                .unwrap_or_else(|e| panic!("{e}"));
                let mut legacy = regex_t {
                    re_nsub: 0,
                    value: ptr::null_mut(),
                };
                assert_eq!(
                    tre_compile(&mut legacy, pattern.as_ptr(), pattern.len(), REG_EXTENDED),
                    REG_OK
                );
                let a = &*owned.preg.value.cast::<tre_tnfa_t>();
                let b = &*legacy.value.cast::<tre_tnfa_t>();
                assert_eq!(a.num_submatches, b.num_submatches);
                for id in 0..a.num_submatches as usize {
                    let left = (*a.submatch_data.add(id)).parents;
                    let right = (*b.submatch_data.add(id)).parents;
                    assert_eq!(left.is_null(), right.is_null());
                    if !left.is_null() {
                        assert!(owned._tables.contains(left.cast()));
                        let mut terminated = false;
                        for n in 0..=a.num_submatches as usize {
                            assert_eq!(*left.add(n), *right.add(n));
                            if *left.add(n) == -1 {
                                terminated = true;
                                break;
                            }
                        }
                        assert!(terminated);
                    }
                }
                let mut left = vec![regmatch_t::default(); owned.preg.re_nsub + 1];
                let mut right = vec![regmatch_t::default(); legacy.re_nsub + 1];
                assert_eq!(
                    super::super::regapi::tre_regwnexec(owned.regex(), &text, &mut left, 0),
                    REG_OK
                );
                assert_eq!(
                    super::super::regapi::tre_regwnexec(&legacy, &text, &mut right, 0),
                    REG_OK
                );
                assert_eq!(left[0].rm_so, 0);
                assert!(left[0].rm_eo > 0);
                for (a, b) in left.iter().zip(&right) {
                    assert_eq!((a.rm_so, a.rm_eo), (b.rm_so, b.rm_eo));
                }
                tre_free(&mut legacy);
                drop(owned);
            });
            assert_eq!(report.remaining, 0);
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                0
            );
        }
    }

    #[test]
    fn retained_tre_submatch_parents_belong_to_original_owned_storage() {
        let mut session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let ledger = owner
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let pattern: Vec<u32> = "((a|b)+?)".chars().map(u32::from).collect();
        let compiled = unsafe {
            tre_compile_owned(
                pattern.as_ptr(),
                pattern.len(),
                REG_EXTENDED,
                crate::mainutils::grep::regex_workspace(),
            )
        }
        .unwrap_or_else(|error| panic!("compile failed: {error}"));
        unsafe {
            let descriptor = &*compiled.preg.value.cast::<tre_tnfa_t>();
            let mut retained_parents = 0;
            for i in 0..descriptor.num_submatches as usize {
                let parents = (*descriptor.submatch_data.add(i)).parents;
                if !parents.is_null() {
                    retained_parents += 1;
                    assert!(
                        compiled._tables.contains(parents.cast()),
                        "production submatch parent array escaped original owned storage"
                    );
                }
            }
            assert!(
                retained_parents >= 2,
                "fixture needs multiple retained parent arrays"
            );
        }
        owner.full_gc().unwrap();
        let replacement = crate::sexp::session::RSession::new_for_gc_tests();
        let other = replacement
            .owner_token()
            .unwrap()
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let held = other.reserve(256).unwrap();
        session.close();
        drop(compiled);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(other.workspace_bytes_for_test(), 256);
        drop(held);
    }

    #[test]
    fn retained_tre_submatch_table_belongs_to_original_owned_storage() {
        let mut session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let ledger = owner
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let pattern: Vec<u32> = "((a|b)+?)".chars().map(u32::from).collect();
        let compiled = unsafe {
            tre_compile_owned(
                pattern.as_ptr(),
                pattern.len(),
                REG_EXTENDED,
                crate::mainutils::grep::regex_workspace(),
            )
        }
        .unwrap_or_else(|error| panic!("compile failed: {error}"));
        unsafe {
            let descriptor = &*compiled.preg.value.cast::<tre_tnfa_t>();
            assert!(descriptor.num_submatches >= 3);
            assert!(!descriptor.submatch_data.is_null());
            assert!(
                compiled._tables.contains(descriptor.submatch_data.cast()),
                "production submatch descriptor table escaped original owned storage"
            );
            assert!(
                (0..descriptor.num_submatches as usize).any(|i| !(*descriptor
                    .submatch_data
                    .add(i))
                .parents
                .is_null()),
                "fixture must exercise nested legacy parent cleanup"
            );
        }
        owner.full_gc().unwrap();
        let replacement = crate::sexp::session::RSession::new_for_gc_tests();
        let other = replacement
            .owner_token()
            .unwrap()
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let held = other.reserve(256).unwrap();
        session.close();
        drop(compiled);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(other.workspace_bytes_for_test(), 256);
        drop(held);
    }

    #[test]
    fn retained_tre_tag_arrays_belong_to_original_owned_storage() {
        let mut session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let ledger = owner
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let pattern: Vec<u32> = "((a|b)+?)".chars().map(u32::from).collect();
        let compiled = unsafe {
            tre_compile_owned(
                pattern.as_ptr(),
                pattern.len(),
                REG_EXTENDED,
                crate::mainutils::grep::regex_workspace(),
            )
        }
        .unwrap_or_else(|error| panic!("compile failed: {error}"));
        unsafe {
            let descriptor = &*compiled.preg.value.cast::<tre_tnfa_t>();
            assert!(descriptor.num_tags > 0);
            assert!(descriptor.num_minimals > 0);
            assert!(!descriptor.minimal_tags.is_null());
            assert!(
                compiled._tables.contains(descriptor.tag_directions.cast()),
                "production tag directions escaped original owned storage"
            );
            assert!(
                compiled._tables.contains(descriptor.minimal_tags.cast()),
                "production minimal tags escaped original owned storage"
            );
        }
        owner.full_gc().unwrap();
        let replacement = crate::sexp::session::RSession::new_for_gc_tests();
        let other = replacement
            .owner_token()
            .unwrap()
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let held = other.reserve(256).unwrap();
        session.close();
        drop(compiled);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(other.workspace_bytes_for_test(), 256);
        drop(held);
    }

    #[test]
    fn retained_tre_submatch_table_admits_cells_reject_overflow_and_retry() {
        use crate::sexp::{memory::ArenaBudget, session::RSession};
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut storage =
            super::super::owned_arena::OwnedArena::new(crate::mainutils::grep::regex_workspace())
                .unwrap();
        let count = 17;
        let bytes = count * std::mem::size_of::<tre_submatch_data_t>();
        owner
            .with_arena(|arena| {
                arena.set_budget(ArenaBudget::new(
                    arena.total_bytes_allocated() + bytes - 1,
                    0,
                ))
            })
            .unwrap();
        assert!(unsafe { allocate_submatch_table(count, &mut Some(&mut storage)) }.is_null());
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        let pointer = unsafe { allocate_submatch_table(count, &mut Some(&mut storage)) };
        assert!(!pointer.is_null() && storage.contains(pointer.cast()));
        assert!(owner.with_arena(|arena| arena.workspace_bytes()).unwrap() >= bytes);
        unsafe {
            assert!(
                std::slice::from_raw_parts(pointer.cast::<u8>(), bytes)
                    .iter()
                    .all(|n| *n == 0)
            );
        }
        drop(storage);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
        let (_, report) = mem::fault::observe(None, || unsafe {
            assert!(allocate_submatch_table(usize::MAX, &mut None).is_null());
            assert!(allocate_submatch_table(0, &mut None).is_null());
        });
        assert_eq!(report.attempts, 0);
        assert_eq!(report.remaining, 0);
    }

    #[test]
    fn retained_tre_tag_arrays_admit_cells_reject_overflow_and_retry() {
        use crate::sexp::{memory::ArenaBudget, session::RSession};
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut storage =
            super::super::owned_arena::OwnedArena::new(crate::mainutils::grep::regex_workspace())
                .unwrap();
        let count = 17;
        let bytes = count * std::mem::size_of::<c_int>();
        owner
            .with_arena(|arena| {
                arena.set_budget(ArenaBudget::new(
                    arena.total_bytes_allocated() + bytes - 1,
                    0,
                ))
            })
            .unwrap();
        assert!(unsafe { allocate_retained_ints(count, true, &mut Some(&mut storage)) }.is_null());
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        let pointer = unsafe { allocate_retained_ints(count, true, &mut Some(&mut storage)) };
        assert!(!pointer.is_null() && storage.contains(pointer.cast()));
        assert!(owner.with_arena(|arena| arena.workspace_bytes()).unwrap() >= bytes);
        unsafe {
            assert!(
                std::slice::from_raw_parts(pointer, count)
                    .iter()
                    .all(|n| *n == 0)
            );
        }
        drop(storage);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
        let (_, report) = mem::fault::observe(None, || unsafe {
            assert!(allocate_retained_ints(usize::MAX, true, &mut None).is_null());
            assert!(allocate_retained_ints(0, false, &mut None).is_null());
        });
        assert_eq!(report.attempts, 0);
        assert_eq!(report.remaining, 0);
    }

    #[test]
    fn retained_tre_tag_arrays_match_legacy_capture_layout_and_cleanup() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let pattern: Vec<u32> = "((a|b)+?)".chars().map(u32::from).collect();
        let text: Vec<u32> = "aaab".chars().map(u32::from).collect();
        let (_, report) = mem::fault::observe(None, || unsafe {
            let owned = tre_compile_owned(
                pattern.as_ptr(),
                pattern.len(),
                REG_EXTENDED,
                crate::mainutils::grep::regex_workspace(),
            )
            .unwrap_or_else(|e| panic!("{e}"));
            let mut legacy = regex_t {
                re_nsub: 0,
                value: ptr::null_mut(),
            };
            assert_eq!(
                tre_compile(&mut legacy, pattern.as_ptr(), pattern.len(), REG_EXTENDED),
                REG_OK
            );
            let mut owned_matches = [regmatch_t::default(); 3];
            let mut legacy_matches = [regmatch_t::default(); 3];
            assert_eq!(
                super::super::regapi::tre_regwnexec(owned.regex(), &text, &mut owned_matches, 0),
                REG_OK
            );
            assert_eq!(
                super::super::regapi::tre_regwnexec(&legacy, &text, &mut legacy_matches, 0),
                REG_OK
            );
            assert_eq!(owned_matches[0].rm_so, 0);
            assert!(owned_matches[0].rm_eo > 0);
            for (a, b) in owned_matches.iter().zip(&legacy_matches) {
                assert_eq!((a.rm_so, a.rm_eo), (b.rm_so, b.rm_eo));
            }
            tre_free(&mut legacy);
            drop(owned);
        });
        assert_eq!(report.remaining, 0);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }

    #[test]
    fn retained_tre_transition_payloads_belong_to_original_storage() {
        let mut session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let ledger = owner
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let pattern: Vec<u32> = "((a|b)+?)".chars().map(u32::from).collect();
        let text: Vec<u32> = "aaab".chars().map(u32::from).collect();
        let compiled = unsafe {
            tre_compile_owned(
                pattern.as_ptr(),
                pattern.len(),
                REG_EXTENDED,
                crate::mainutils::grep::regex_workspace(),
            )
        }
        .unwrap_or_else(|error| panic!("compile failed: {error}"));
        let mut legacy = regex_t {
            re_nsub: 0,
            value: ptr::null_mut(),
        };
        assert_eq!(
            unsafe { tre_compile(&mut legacy, pattern.as_ptr(), pattern.len(), REG_EXTENDED) },
            REG_OK
        );
        let mut owned_matches = [regmatch_t::default(); 3];
        let mut legacy_matches = [regmatch_t::default(); 3];
        assert_eq!(
            unsafe {
                super::super::regapi::tre_regwnexec(compiled.regex(), &text, &mut owned_matches, 0)
            },
            REG_OK
        );
        assert_eq!(
            unsafe { super::super::regapi::tre_regwnexec(&legacy, &text, &mut legacy_matches, 0) },
            REG_OK
        );
        for (owned, standalone) in owned_matches.iter().zip(&legacy_matches) {
            assert_eq!(
                (owned.rm_so, owned.rm_eo),
                (standalone.rm_so, standalone.rm_eo)
            );
        }
        unsafe {
            let descriptor = &*compiled.preg.value.cast::<tre_tnfa_t>();
            let mut tagged_transitions = 0;
            for index in 0..descriptor.num_transitions {
                let transition = &*descriptor.transitions.add(index as usize);
                if transition.state.is_null() {
                    continue;
                }
                if !transition.tags.is_null() {
                    tagged_transitions += 1;
                    assert!(compiled._tables.contains(transition.tags.cast()));
                }
                if !transition.params.is_null() {
                    assert!(compiled._tables.contains(transition.params.cast()));
                }
            }
            let mut initial = descriptor.initial;
            while !(*initial).state.is_null() {
                if !(*initial).tags.is_null() {
                    tagged_transitions += 1;
                    assert!(compiled._tables.contains((*initial).tags.cast()));
                }
                if !(*initial).params.is_null() {
                    assert!(compiled._tables.contains((*initial).params.cast()));
                }
                initial = initial.add(1);
            }
            assert!(
                tagged_transitions > 0,
                "fixture must retain transition tags"
            );
        }
        unsafe { tre_free(&mut legacy) };
        owner.full_gc().unwrap();
        let replacement = crate::sexp::session::RSession::new_for_gc_tests();
        let other = replacement
            .owner_token()
            .unwrap()
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let held = other.reserve(256).unwrap();
        session.close();
        drop(compiled);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(other.workspace_bytes_for_test(), 256);
        drop(held);
    }

    fn synthetic_transition_positions(
        left_tags: &mut [c_int; 2],
        right_tags: &mut [c_int; 2],
        params: &mut [c_int; TRE_PARAM_LAST],
    ) -> ([tre_pos_and_tags_t; 2], [tre_pos_and_tags_t; 2], [c_int; 2]) {
        let mut left: [tre_pos_and_tags_t; 2] = [unsafe { std::mem::zeroed() }; 2];
        left[0].position = 0;
        left[0].code_min = i32::from(b'a');
        left[0].code_max = i32::from(b'a');
        left[0].tags = left_tags.as_mut_ptr();
        left[0].backref = -1;
        left[1].position = -1;

        let mut right: [tre_pos_and_tags_t; 2] = [unsafe { std::mem::zeroed() }; 2];
        right[0].position = 1;
        right[0].code_min = i32::from(b'a');
        right[0].code_max = i32::from(b'a');
        right[0].tags = right_tags.as_mut_ptr();
        right[0].params = params.as_mut_ptr();
        right[0].backref = -1;
        right[1].position = -1;
        (left, right, [0, 1])
    }

    #[test]
    fn retained_tre_transition_tag_and_params_belong_to_original_storage() {
        use crate::sexp::session::RSession;

        let mut session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let ledger = owner
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let mut storage =
            super::super::owned_arena::OwnedArena::new(crate::mainutils::grep::regex_workspace())
                .unwrap();
        let mut left_tags = [7, -1];
        let mut right_tags = [9, -1];
        let mut params = [TRE_PARAM_UNSET; TRE_PARAM_LAST];
        params[TRE_PARAM_COST_INS] = 3;
        let (left, right, offsets) =
            synthetic_transition_positions(&mut left_tags, &mut right_tags, &mut params);

        let descriptor = unsafe { allocate_tnfa(&mut Some(&mut storage)) };
        let transitions = unsafe { allocate_transition_table(1, &mut Some(&mut storage)) };
        assert!(!descriptor.is_null() && !transitions.is_null());
        let status = unsafe {
            tre_make_trans(
                left.as_ptr(),
                right.as_ptr(),
                transitions,
                ptr::null_mut(),
                offsets.as_ptr(),
                &mut Some(&mut storage),
            )
        };
        assert_eq!(status, REG_OK);
        assert!(storage.contains(unsafe { (*transitions).tags.cast() }));
        assert!(storage.contains(unsafe { (*transitions).params.cast() }));
        unsafe {
            assert_eq!(
                std::slice::from_raw_parts((*transitions).tags, 3),
                [7, 9, -1]
            );
            assert_eq!(
                *(*transitions).params.add(TRE_PARAM_COST_INS),
                params[TRE_PARAM_COST_INS]
            );
            (*descriptor).transitions = transitions;
            (*descriptor).num_transitions = 1;
            (*transitions).state = transitions.add(1);
        }
        let compiled = CompiledTre {
            preg: regex_t {
                re_nsub: 0,
                value: descriptor.cast(),
            },
            _tables: storage,
        };
        owner.full_gc().unwrap();
        let replacement = RSession::new_for_gc_tests();
        let other = replacement
            .owner_token()
            .unwrap()
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let held = other.reserve(256).unwrap();
        session.close();
        drop(compiled);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(other.workspace_bytes_for_test(), 256);
        drop(held);
    }

    #[test]
    fn retained_tre_transition_params_deny_unfunded_cells_and_retry() {
        use crate::sexp::{memory::ArenaBudget, session::RSession};

        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut storage =
            super::super::owned_arena::OwnedArena::new(crate::mainutils::grep::regex_workspace())
                .unwrap();
        let mut left_tags = [7, -1];
        let mut right_tags = [9, -1];
        let mut params = [TRE_PARAM_UNSET; TRE_PARAM_LAST];
        params[TRE_PARAM_COST_INS] = 3;
        let (mut left, mut right, offsets) =
            synthetic_transition_positions(&mut left_tags, &mut right_tags, &mut params);
        left[0].tags = ptr::null_mut();
        right[0].tags = ptr::null_mut();

        let bytes = TRE_PARAM_LAST * std::mem::size_of::<c_int>();
        owner
            .with_arena(|arena| {
                arena.set_budget(ArenaBudget::new(
                    arena.total_bytes_allocated() + bytes - 1,
                    0,
                ))
            })
            .unwrap();
        let mut denied: [tre_tnfa_transition_t; 2] = unsafe { std::mem::zeroed() };
        let status = unsafe {
            tre_make_trans(
                left.as_ptr(),
                right.as_ptr(),
                denied.as_mut_ptr(),
                ptr::null_mut(),
                offsets.as_ptr(),
                &mut Some(&mut storage),
            )
        };
        assert_eq!(status, REG_ESPACE);
        assert!(denied[0].tags.is_null());
        assert!(denied[0].params.is_null());
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );

        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        let mut recovered: [tre_tnfa_transition_t; 2] = unsafe { std::mem::zeroed() };
        let status = unsafe {
            tre_make_trans(
                left.as_ptr(),
                right.as_ptr(),
                recovered.as_mut_ptr(),
                ptr::null_mut(),
                offsets.as_ptr(),
                &mut Some(&mut storage),
            )
        };
        assert_eq!(status, REG_OK);
        assert!(recovered[0].tags.is_null());
        assert!(storage.contains(recovered[0].params.cast()));
        unsafe {
            assert_eq!(
                *recovered[0].params.add(TRE_PARAM_COST_INS),
                params[TRE_PARAM_COST_INS]
            );
        }
        drop(storage);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }

    #[test]
    fn standalone_tre_transition_tag_and_params_use_malloc_free_cleanup() {
        let mut left_tags = [7, -1];
        let mut right_tags = [9, -1];
        let mut params = [TRE_PARAM_UNSET; TRE_PARAM_LAST];
        params[TRE_PARAM_COST_INS] = 3;
        let (left, right, offsets) =
            synthetic_transition_positions(&mut left_tags, &mut right_tags, &mut params);
        let (status, report) = mem::fault::observe(None, || unsafe {
            let mut storage = None;
            let descriptor = allocate_tnfa(&mut storage);
            let transitions = allocate_transition_table(1, &mut storage);
            assert!(!descriptor.is_null() && !transitions.is_null());
            let status = tre_make_trans(
                left.as_ptr(),
                right.as_ptr(),
                transitions,
                ptr::null_mut(),
                offsets.as_ptr(),
                &mut storage,
            );
            assert_eq!(status, REG_OK);
            assert!(!(*transitions).tags.is_null());
            assert!(!(*transitions).params.is_null());
            (*descriptor).transitions = transitions;
            (*descriptor).num_transitions = 1;
            let mut preg = regex_t {
                re_nsub: 0,
                value: descriptor.cast(),
            };
            tre_free(&mut preg);
            assert!(preg.value.is_null());
            status
        });
        assert_eq!(status, REG_OK);
        assert!(report.attempts > 1);
        assert_eq!(report.remaining, 0);
    }

    #[test]
    fn retained_tre_descriptor_belongs_to_original_owned_storage() {
        let mut session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let ledger = owner
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let pattern = [u32::from('a')];
        let compiled = unsafe {
            tre_compile_owned(
                pattern.as_ptr(),
                pattern.len(),
                REG_EXTENDED,
                crate::mainutils::grep::regex_workspace(),
            )
        }
        .unwrap_or_else(|error| panic!("compile failed: {error}"));
        assert!(
            compiled._tables.contains(compiled.preg.value),
            "production TNFA descriptor escaped retained original-owner storage"
        );
        owner.full_gc().unwrap();
        let replacement = crate::sexp::session::RSession::new_for_gc_tests();
        let other = replacement
            .owner_token()
            .unwrap()
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let held = other.reserve(192).unwrap();
        session.close();
        drop(compiled);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(other.workspace_bytes_for_test(), 192);
        drop(held);
    }

    #[test]
    fn retained_tre_descriptor_denies_unfunded_cells_and_retries() {
        use crate::sexp::{memory::ArenaBudget, session::RSession};
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut storage =
            super::super::owned_arena::OwnedArena::new(crate::mainutils::grep::regex_workspace())
                .unwrap();
        let bytes = std::mem::size_of::<tre_tnfa_t>();
        owner
            .with_arena(|arena| {
                arena.set_budget(ArenaBudget::new(
                    arena.total_bytes_allocated() + bytes - 1,
                    0,
                ))
            })
            .unwrap();
        let denied = unsafe { allocate_tnfa(&mut Some(&mut storage)) };
        assert!(denied.is_null());
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        let descriptor = unsafe { allocate_tnfa(&mut Some(&mut storage)) };
        assert!(!descriptor.is_null());
        assert!(storage.contains(descriptor.cast()));
        assert!(owner.with_arena(|arena| arena.workspace_bytes()).unwrap() >= bytes);
        unsafe {
            assert!((*descriptor).transitions.is_null());
            assert!((*descriptor).initial.is_null());
            assert_eq!((*descriptor).num_states, 0);
            assert_eq!((*descriptor).num_submatches, 0);
        }
        drop(storage);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }

    #[test]
    fn retained_tre_descriptor_standalone_compile_keeps_legacy_cleanup() {
        let pattern = [u32::from('a')];
        let (status, report) = mem::fault::observe(None, || unsafe {
            let mut preg = regex_t {
                re_nsub: 0,
                value: ptr::null_mut(),
            };
            let status = tre_compile(&mut preg, pattern.as_ptr(), pattern.len(), REG_EXTENDED);
            tre_free(&mut preg);
            assert!(preg.value.is_null());
            status
        });
        assert_eq!(status, REG_OK);
        assert!(report.attempts > 1);
        assert_eq!(report.remaining, 0);
    }

    #[test]
    fn retained_tre_tables_every_raw_failure_releases_owned_and_legacy_storage() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let pattern: Vec<u32> = "((ab|[c-e]){1,3})+".chars().map(u32::from).collect();
        let compile = || {
            let result = unsafe {
                tre_compile_owned(
                    pattern.as_ptr(),
                    pattern.len(),
                    REG_EXTENDED,
                    crate::mainutils::grep::regex_workspace(),
                )
            };
            let success = result.is_ok();
            drop(result);
            success
        };
        let (success, baseline) = mem::fault::observe(None, compile);
        assert!(success);
        assert_eq!(baseline.remaining, 0);
        assert!(baseline.attempts > 4);
        let failures: Vec<usize> = if cfg!(miri) {
            vec![0, 1, baseline.attempts / 2, baseline.attempts - 1]
        } else {
            (0..baseline.attempts).collect()
        };
        for failure in failures {
            let (success, report) = mem::fault::observe(Some(failure), compile);
            assert!(
                !success,
                "raw allocation failure {failure} must reject publication"
            );
            assert_eq!(report.remaining, 0, "raw allocation failure {failure}");
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                0,
                "owned table/arena storage survives failed compile {failure}"
            );
        }
        assert!(compile());
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }
}

#[cfg(test)]
mod traversal_stack_tests {
    use super::*;

    #[derive(Clone, Copy, Debug)]
    enum Pass {
        Tags,
        Copy,
        Expand,
        Empty,
        Nfl,
        Tnfa,
    }

    // Empty leaves let TNFA construction exercise the same continuation pushes
    // without needing transition buffers; every pass visits union, catenation,
    // and iteration nodes.
    fn run(pass: Pass, fail_at: Option<usize>) -> (c_int, mem::fault::Report) {
        let mut arena =
            mem::tre_mem_struct::new(crate::mainutils::grep::RegexWorkspace::unbounded()).unwrap();
        unsafe {
            let mem = &mut arena as mem::tre_mem_t;
            let left = tre_ast_new_literal(mem, EMPTY as c_int, -1, -1);
            let right = tre_ast_new_literal(mem, EMPTY as c_int, -1, -1);
            let union = tre_ast_new_union(mem, left, right);
            let iter = tre_ast_new_iter(mem, union, 0, -1, 0);
            let tail = tre_ast_new_literal(mem, EMPTY as c_int, -1, -1);
            let tree = tre_ast_new_catenation(mem, iter, tail);
            assert!(!tree.is_null());
            // Nullable and position sets are prerequisites for the final two
            // traversals; prepare them before observing scratch allocations.
            if matches!(pass, Pass::Empty | Pass::Tnfa) {
                let stack = stack::tre_stack_new(64, 10240, 64);
                assert!(!stack.is_null());
                assert_eq!(tre_compute_nfl(mem, stack, tree), REG_OK);
                stack::tre_stack_destroy(stack);
            }
            mem::fault::observe(fail_at, || {
                let stack = stack::tre_stack_new(1, 10240, 1);
                if stack.is_null() {
                    return REG_ESPACE;
                }
                let mut value = 0;
                let mut depth = 0;
                let mut copy = ptr::null_mut();
                let mut tnfa: tre_tnfa_t = std::mem::zeroed();
                let status = match pass {
                    Pass::Tags => tre_add_tags(ptr::null_mut(), stack, tree, &mut tnfa, &mut None),
                    Pass::Copy => tre_copy_ast(
                        mem,
                        stack,
                        tree,
                        0,
                        &mut value,
                        ptr::null_mut(),
                        &mut copy,
                        &mut depth,
                    ),
                    Pass::Expand => {
                        tre_expand_ast(mem, stack, tree, &mut value, ptr::null_mut(), &mut depth)
                    }
                    Pass::Empty => tre_match_empty(
                        stack,
                        tree,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    ),
                    Pass::Nfl => tre_compute_nfl(mem, stack, tree),
                    Pass::Tnfa => tre_ast_to_tnfa_iter(
                        stack,
                        tree,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null(),
                        &mut None,
                    ),
                };
                stack::tre_stack_destroy(stack);
                status
            })
        }
    }

    #[test]
    fn every_compiler_traversal_rejects_stack_growth_failure_and_recovers() {
        for pass in [
            Pass::Tags,
            Pass::Copy,
            Pass::Expand,
            Pass::Empty,
            Pass::Nfl,
            Pass::Tnfa,
        ] {
            let (status, baseline) = run(pass, None);
            assert_eq!(status, REG_OK, "{pass:?}");
            assert_eq!(baseline.remaining, 0, "{pass:?}");
            assert!(baseline.attempts > 2, "{pass:?} must grow its stack");
            for failure in 0..baseline.attempts {
                let (status, report) = run(pass, Some(failure));
                assert_eq!(status, REG_ESPACE, "{pass:?}: allocation {failure}");
                assert_eq!(report.remaining, 0, "{pass:?}: allocation {failure}");
            }
            assert_eq!(run(pass, None).0, REG_OK, "{pass:?} recovery");
        }
    }
}
