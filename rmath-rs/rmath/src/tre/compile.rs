#![allow(unreachable_code)]
#![allow(unused_variables)]
#![allow(unused_variables)]
#![allow(unused_assignments)]
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
        (*c).right =
            mem::tre_mem_alloc(mem, std::mem::size_of::<tre_ast_node_t>()) as *mut tre_ast_node_t;
        if (*c).right.is_null() {
            return REG_ESPACE as c_int;
        }

        (*(*c).right).obj = (*node).obj;
        (*(*c).right).type_ = (*node).type_;
        (*(*c).right).nullable = -1;
        (*(*c).right).submatch_id = -1;
        (*(*c).right).firstpos = ptr::null_mut();
        (*(*c).right).lastpos = ptr::null_mut();
        (*(*c).right).num_tags = 0;
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
        (*c).left =
            mem::tre_mem_alloc(mem, std::mem::size_of::<tre_ast_node_t>()) as *mut tre_ast_node_t;
        if (*c).left.is_null() {
            return REG_ESPACE as c_int;
        }

        (*(*c).left).obj = (*node).obj;
        (*(*c).left).type_ = (*node).type_;
        (*(*c).left).nullable = -1;
        (*(*c).left).submatch_id = -1;
        (*(*c).left).firstpos = ptr::null_mut();
        (*(*c).left).lastpos = ptr::null_mut();
        (*(*c).left).num_tags = 0;
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

        stack::tre_stack_push_voidptr(stack, node as *mut c_void);
        stack::tre_stack_push_int(stack, tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int);

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
                                let p =
                                    mem::xmalloc(std::mem::size_of::<c_int>() * (i as usize + 1))
                                        as *mut c_int;
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

                        stack::tre_stack_push_int(stack, (*node).submatch_id);
                        stack::tre_stack_push_int(
                            stack,
                            tre_addtags_symbol_t::ADDTAGS_SET_SUBMATCH_END as c_int,
                        );
                    }

                    match (*node).type_ {
                        tre_ast_type_t::LITERAL => {
                            let lit = (*node).obj as *mut tre_literal_t;
                            if !IS_SPECIAL(&*lit) || IS_BACKREF(&*lit) {
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
                                    } else {
                                        (*node).num_tags = 1;
                                    }
                                    *regset = -1;
                                    tag = next_tag;
                                    num_tags += 1;
                                    next_tag += 1;
                                }
                            }
                        }
                        tre_ast_type_t::CATENATION => {
                            let cat = (*node).obj as *mut tre_catenation_t;
                            let left = (*cat).left;
                            let right = (*cat).right;
                            let mut reserved_tag: c_int = -1;

                            stack::tre_stack_push_voidptr(stack, node as *mut c_void);
                            stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_AFTER_CAT_RIGHT as c_int,
                            );

                            stack::tre_stack_push_voidptr(stack, right as *mut c_void);
                            stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int,
                            );

                            stack::tre_stack_push_int(stack, next_tag + (*left).num_tags);
                            if (*left).num_tags > 0 && (*right).num_tags > 0 {
                                reserved_tag = next_tag;
                                next_tag += 1;
                            }
                            stack::tre_stack_push_int(stack, reserved_tag);
                            stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_AFTER_CAT_LEFT as c_int,
                            );

                            stack::tre_stack_push_voidptr(stack, left as *mut c_void);
                            stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int,
                            );
                        }
                        tre_ast_type_t::ITERATION => {
                            let iter = (*node).obj as *mut tre_iteration_t;

                            if first_pass {
                                stack::tre_stack_push_int(
                                    stack,
                                    if *regset >= 0 || (*iter).minimal != 0 {
                                        1
                                    } else {
                                        0
                                    },
                                );
                            } else {
                                stack::tre_stack_push_int(stack, tag);
                                stack::tre_stack_push_int(stack, (*iter).minimal as c_int);
                            }
                            stack::tre_stack_push_voidptr(stack, node as *mut c_void);
                            stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_AFTER_ITERATION as c_int,
                            );

                            stack::tre_stack_push_voidptr(stack, (*iter).arg as *mut c_void);
                            stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int,
                            );

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

                            stack::tre_stack_push_int(stack, right_tag);
                            stack::tre_stack_push_int(stack, left_tag);
                            stack::tre_stack_push_voidptr(stack, regset as *mut c_void);
                            stack::tre_stack_push_int(stack, if *regset >= 0 { 1 } else { 0 });
                            stack::tre_stack_push_voidptr(stack, node as *mut c_void);
                            stack::tre_stack_push_voidptr(stack, right as *mut c_void);
                            stack::tre_stack_push_voidptr(stack, left as *mut c_void);
                            stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_AFTER_UNION_RIGHT as c_int,
                            );

                            stack::tre_stack_push_voidptr(stack, right as *mut c_void);
                            stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int,
                            );

                            stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_AFTER_UNION_LEFT as c_int,
                            );

                            stack::tre_stack_push_voidptr(stack, left as *mut c_void);
                            stack::tre_stack_push_int(
                                stack,
                                tre_addtags_symbol_t::ADDTAGS_RECURSE as c_int,
                            );

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
                            .map_or(0, |a| (*a).num_tags)
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
                            .map_or(0, |l| (*l).num_tags)
                            + (*((*node).obj as *mut tre_catenation_t))
                                .right
                                .as_ref()
                                .map_or(0, |r| (*r).num_tags);
                    }
                }
                x if x == tre_addtags_symbol_t::ADDTAGS_AFTER_UNION_LEFT as c_int => {
                    while *regset >= 0 {
                        regset = regset.offset(1);
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
                            .map_or(0, |l| (*l).num_tags)
                            + (*((*node).obj as *mut tre_union_t))
                                .right
                                .as_ref()
                                .map_or(0, |r| (*r).num_tags)
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

        stack::tre_stack_push_voidptr(stack, ast as *mut c_void);
        stack::tre_stack_push_int(stack, tre_copyast_symbol_t::COPY_RECURSE as c_int);

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
                                stack::tre_stack_push_voidptr(stack, (*uni).right as *mut c_void);
                                stack::tre_stack_push_int(
                                    stack,
                                    tre_copyast_symbol_t::COPY_RECURSE as c_int,
                                );
                                let right_ptr: *mut *mut tre_ast_node_t =
                                    std::ptr::addr_of_mut!((*tmp).right);
                                stack::tre_stack_push_voidptr(stack, right_ptr as *mut c_void);
                                stack::tre_stack_push_int(
                                    stack,
                                    tre_copyast_symbol_t::COPY_SET_RESULT_PTR as c_int,
                                );
                                stack::tre_stack_push_voidptr(stack, (*uni).left as *mut c_void);
                                stack::tre_stack_push_int(
                                    stack,
                                    tre_copyast_symbol_t::COPY_RECURSE as c_int,
                                );
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
                                stack::tre_stack_push_voidptr(stack, (*cat).right as *mut c_void);
                                stack::tre_stack_push_int(
                                    stack,
                                    tre_copyast_symbol_t::COPY_RECURSE as c_int,
                                );
                                let right_ptr2: *mut *mut tre_ast_node_t =
                                    std::ptr::addr_of_mut!((*tmp).right);
                                stack::tre_stack_push_voidptr(stack, right_ptr2 as *mut c_void);
                                stack::tre_stack_push_int(
                                    stack,
                                    tre_copyast_symbol_t::COPY_SET_RESULT_PTR as c_int,
                                );
                                stack::tre_stack_push_voidptr(stack, (*cat).left as *mut c_void);
                                stack::tre_stack_push_int(
                                    stack,
                                    tre_copyast_symbol_t::COPY_RECURSE as c_int,
                                );
                            }
                        }
                        tre_ast_type_t::ITERATION => {
                            let iter = (*node).obj as *mut tre_iteration_t;
                            stack::tre_stack_push_voidptr(stack, (*iter).arg as *mut c_void);
                            stack::tre_stack_push_int(
                                stack,
                                tre_copyast_symbol_t::COPY_RECURSE as c_int,
                            );
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
        let mut params: [c_int; TRE_PARAM_LAST] = [TRE_PARAM_DEFAULT; TRE_PARAM_LAST];
        let mut params_depth: c_int = 0;
        let mut iter_depth: c_int = 0;

        stack::tre_stack_push_voidptr(stack, ast as *mut c_void);
        stack::tre_stack_push_int(stack, tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int);

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
                        stack::tre_stack_push_voidptr(stack, (*uni).right as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int,
                        );
                        stack::tre_stack_push_voidptr(stack, (*uni).left as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int,
                        );
                    }
                    tre_ast_type_t::CATENATION => {
                        let cat = (*node).obj as *mut tre_catenation_t;
                        stack::tre_stack_push_voidptr(stack, (*cat).right as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int,
                        );
                        stack::tre_stack_push_voidptr(stack, (*cat).left as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int,
                        );
                    }
                    tre_ast_type_t::ITERATION => {
                        let iter = (*node).obj as *mut tre_iteration_t;
                        stack::tre_stack_push_int(stack, pos_add);
                        stack::tre_stack_push_voidptr(stack, node as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_expand_ast_symbol_t::EXPAND_AFTER_ITER as c_int,
                        );
                        stack::tre_stack_push_voidptr(stack, (*iter).arg as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_expand_ast_symbol_t::EXPAND_RECURSE as c_int,
                        );
                        if (*iter).min > 1 || (*iter).max > 1 {
                            pos_add = 0;
                        }
                        iter_depth += 1;
                    }
                },
                x if x == tre_expand_ast_symbol_t::EXPAND_AFTER_ITER as c_int => {
                    let iter = (*node).obj as *mut tre_iteration_t;
                    let pos_add_last = stack::tre_stack_pop_int(stack);
                    let saved_pos_add = stack::tre_stack_pop_int(stack);

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
        (*new_set.offset(1)).position = -1;
        (*new_set.offset(1)).code_min = -1;
        (*new_set.offset(1)).code_max = -1;
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

        for i in 0..s1 {
            *new_set.offset(i as isize) = *set1.offset(i as isize);
            (*new_set.offset(i as isize)).assertions |= assertions;
        }
        for i in 0..s2 {
            *new_set.offset((s1 + i) as isize) = *set2.offset(i as isize);
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
        let mut status: c_int = REG_OK as c_int;

        if !num_tags_seen.is_null() {
            *num_tags_seen = 0;
        }
        if !_params_seen.is_null() {
            *_params_seen = 0;
        }

        stack::tre_stack_push_voidptr(stack, node as *mut c_void);

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
                                    *params.offset(i as isize) = *p.offset(i as isize);
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
                        stack::tre_stack_push_voidptr(stack, (*uni).left as *mut c_void);
                    } else if !(*uni).right.is_null() && (*(*uni).right).nullable != 0 {
                        stack::tre_stack_push_voidptr(stack, (*uni).right as *mut c_void);
                    }
                }
                tre_ast_type_t::CATENATION => {
                    let cat = (*node).obj as *mut tre_catenation_t;
                    stack::tre_stack_push_voidptr(stack, (*cat).left as *mut c_void);
                    stack::tre_stack_push_voidptr(stack, (*cat).right as *mut c_void);
                }
                tre_ast_type_t::ITERATION => {
                    let iter = (*node).obj as *mut tre_iteration_t;
                    if !(*iter).arg.is_null() && (*(*iter).arg).nullable != 0 {
                        stack::tre_stack_push_voidptr(stack, (*iter).arg as *mut c_void);
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
        stack::tre_stack_push_voidptr(stack, tree as *mut c_void);
        stack::tre_stack_push_int(stack, tre_nfl_stack_symbol_t::NFL_RECURSE as c_int);

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
                        stack::tre_stack_push_voidptr(stack, node as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_POST_UNION as c_int,
                        );
                        let uni = (*node).obj as *mut tre_union_t;
                        stack::tre_stack_push_voidptr(stack, (*uni).right as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_RECURSE as c_int,
                        );
                        stack::tre_stack_push_voidptr(stack, (*uni).left as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_RECURSE as c_int,
                        );
                    }
                    tre_ast_type_t::CATENATION => {
                        stack::tre_stack_push_voidptr(stack, node as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_POST_CATENATION as c_int,
                        );
                        let cat = (*node).obj as *mut tre_catenation_t;
                        stack::tre_stack_push_voidptr(stack, (*cat).right as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_RECURSE as c_int,
                        );
                        stack::tre_stack_push_voidptr(stack, (*cat).left as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_RECURSE as c_int,
                        );
                    }
                    tre_ast_type_t::ITERATION => {
                        stack::tre_stack_push_voidptr(stack, node as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_POST_ITERATION as c_int,
                        );
                        let iter = (*node).obj as *mut tre_iteration_t;
                        stack::tre_stack_push_voidptr(stack, (*iter).arg as *mut c_void);
                        stack::tre_stack_push_int(
                            stack,
                            tre_nfl_stack_symbol_t::NFL_RECURSE as c_int,
                        );
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
                        trans = trans.offset(1);
                    }

                    if (*trans).state.is_null() {
                        // Set terminator for next transition
                        let _ = trans;
                    }

                    let mut t = transitions
                        .offset(*offs.offset((*p1.offset(pi)).position as isize) as isize);
                    while !(*t).state.is_null() {
                        t = t.offset(1);
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

                    if i + j > 0 {
                        let total = (i + j + 1) as usize;
                        (*t).tags =
                            mem::xmalloc(std::mem::size_of::<c_int>() * total) as *mut c_int;
                        if !(*t).tags.is_null() {
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
                        (*t).params = mem::xmalloc(std::mem::size_of::<c_int>() * TRE_PARAM_LAST)
                            as *mut c_int;
                        if !(*t).params.is_null() {
                            for k in 0..TRE_PARAM_LAST {
                                *(*t).params.offset(k as isize) = TRE_PARAM_UNSET;
                                if !(*p1.offset(pi)).params.is_null()
                                    && *(*p1.offset(pi)).params.offset(k as isize)
                                        != TRE_PARAM_UNSET
                                {
                                    *(*t).params.offset(k as isize) =
                                        *(*p1.offset(pi)).params.offset(k as isize);
                                }
                                if !(*p2.offset(pj)).params.is_null()
                                    && *(*p2.offset(pj)).params.offset(k as isize)
                                        != TRE_PARAM_UNSET
                                {
                                    *(*t).params.offset(k as isize) =
                                        *(*p2.offset(pj)).params.offset(k as isize);
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
) -> c_int {
    unsafe {
        stack::tre_stack_push_voidptr(stack, node as *mut c_void);

        while stack::tre_stack_num_objects(stack) > 0 {
            let node = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;
            match (*node).type_ {
                tre_ast_type_t::LITERAL => {}
                tre_ast_type_t::UNION => {
                    let uni = (*node).obj as *mut tre_union_t;
                    stack::tre_stack_push_voidptr(stack, (*uni).right as *mut c_void);
                    stack::tre_stack_push_voidptr(stack, (*uni).left as *mut c_void);
                }
                tre_ast_type_t::CATENATION => {
                    let cat = (*node).obj as *mut tre_catenation_t;
                    let errcode = tre_make_trans(
                        (*cat).left.as_ref().map_or(ptr::null(), |l| (*l).lastpos),
                        (*cat).right.as_ref().map_or(ptr::null(), |r| (*r).firstpos),
                        transitions,
                        counts,
                        offs,
                    );
                    if errcode != REG_OK as c_int {
                        return errcode;
                    }
                    stack::tre_stack_push_voidptr(stack, (*cat).right as *mut c_void);
                    stack::tre_stack_push_voidptr(stack, (*cat).left as *mut c_void);
                }
                tre_ast_type_t::ITERATION => {
                    let iter = (*node).obj as *mut tre_iteration_t;
                    if (*iter).max == -1 {
                        let errcode = tre_make_trans(
                            (*iter).arg.as_ref().map_or(ptr::null(), |a| (*a).lastpos),
                            (*iter).arg.as_ref().map_or(ptr::null(), |a| (*a).firstpos),
                            transitions,
                            counts,
                            offs,
                        );
                        if errcode != REG_OK as c_int {
                            return errcode;
                        }
                    }
                    stack::tre_stack_push_voidptr(stack, (*iter).arg as *mut c_void);
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
) -> c_int {
    unsafe {
        let stack = stack::tre_stack_new(1024, 256 * 1024, 4096);
        if stack.is_null() {
            return REG_ESPACE as c_int;
        }
        let errcode = tre_ast_to_tnfa_iter(stack, node, transitions, counts, offs);
        stack::tre_stack_destroy(stack);
        errcode
    }
}

// ===== Main compile function =====

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_compile(
    preg: *mut regex_t,
    regex: *const tre_char_t,
    n: usize,
    cflags: c_int,
) -> c_int {
    unsafe {
        let mut parse_ctx: tre_parse_ctx_t = std::mem::zeroed();
        let mut counts: *mut c_int = ptr::null_mut();
        let mut offs: *mut c_int = ptr::null_mut();

        let stack = stack::tre_stack_new(512, 10240, 128);
        if stack.is_null() {
            return REG_ESPACE as c_int;
        }
        let mem = mem::tre_mem_new();
        if mem.is_null() {
            stack::tre_stack_destroy(stack);
            return REG_ESPACE as c_int;
        }

        parse_ctx.mem = mem;
        parse_ctx.stack = stack;
        parse_ctx.re = regex;
        parse_ctx.len = n;
        parse_ctx.cflags = cflags;
        parse_ctx.max_backref = -1;
        parse_ctx.cur_max = 1; // Simplified: always use byte mode

        let errcode = super::parse::tre_parse(&mut parse_ctx);
        if errcode != REG_OK as c_int {
            let _ = errcode;
            goto_error(
                mem,
                stack,
                counts,
                offs,
                preg,
                ptr::null_mut(),
                REG_ESPACE as c_int,
            );
            unreachable!()
        }

        (*preg).re_nsub = if parse_ctx.submatch_id > 0 {
            (parse_ctx.submatch_id - 1) as usize
        } else {
            0
        };
        let tree = parse_ctx.result;

        // Allocate TNFA
        let tnfa = mem::xcalloc(1, std::mem::size_of::<tre_tnfa_t>()) as *mut tre_tnfa_t;
        if tnfa.is_null() {
            goto_error(
                mem,
                stack,
                counts,
                offs,
                preg,
                ptr::null_mut(),
                REG_ESPACE as c_int,
            );
            unreachable!()
        }
        (*tnfa).have_backrefs = if parse_ctx.max_backref >= 0 { 1 } else { 0 };
        (*tnfa).have_approx = parse_ctx.have_approx;
        (*tnfa).num_submatches = parse_ctx.submatch_id as u32;

        // Set up tags for submatch addressing
        if (*tnfa).have_backrefs != 0 || (cflags & REG_NOSUB) == 0 {
            let _errcode = tre_add_tags(ptr::null_mut(), stack, tree, tnfa);
            if (*tnfa).num_tags > 0 {
                let tag_directions =
                    mem::xmalloc(std::mem::size_of::<c_int>() * ((*tnfa).num_tags as usize + 1))
                        as *mut c_int;
                if tag_directions.is_null() {
                    goto_error(mem, stack, counts, offs, preg, tnfa, REG_ESPACE as c_int);
                    unreachable!()
                }
                (*tnfa).tag_directions = tag_directions;
                for i in 0..=(*tnfa).num_tags {
                    *tag_directions.offset(i as isize) = -1;
                }
            }
            (*tnfa).minimal_tags = mem::xcalloc(
                (*tnfa).num_tags as usize * 2 + 1,
                std::mem::size_of::<c_int>(),
            ) as *mut c_int;
            if (*tnfa).minimal_tags.is_null() {
                goto_error(mem, stack, counts, offs, preg, tnfa, REG_ESPACE as c_int);
                unreachable!()
            }

            let submatch_data = mem::xcalloc(
                parse_ctx.submatch_id as usize,
                std::mem::size_of::<tre_submatch_data_t>(),
            ) as *mut tre_submatch_data_t;
            if submatch_data.is_null() {
                goto_error(mem, stack, counts, offs, preg, tnfa, REG_ESPACE as c_int);
                unreachable!()
            }
            (*tnfa).submatch_data = submatch_data;

            let _errcode = tre_add_tags(mem, stack, tree, tnfa);
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

        // Add dummy node for final state
        let tmp_ast_l = tree;
        let tmp_ast_r = tre_ast_new_literal(mem, 0, 0, parse_ctx.position);
        if tmp_ast_r.is_null() {
            goto_error(mem, stack, counts, offs, preg, tnfa, REG_ESPACE as c_int);
            unreachable!()
        }
        parse_ctx.position += 1;
        let tree = tre_ast_new_catenation(mem, tmp_ast_l, tmp_ast_r);
        if tree.is_null() {
            goto_error(mem, stack, counts, offs, preg, tnfa, REG_ESPACE as c_int);
            unreachable!()
        }

        let _errcode = tre_compute_nfl(mem, stack, tree);
        if _errcode != REG_OK as c_int {
            goto_error(mem, stack, counts, offs, preg, tnfa, _errcode);
            unreachable!()
        }

        counts =
            mem::xmalloc(std::mem::size_of::<c_int>() * parse_ctx.position as usize) as *mut c_int;
        if counts.is_null() {
            goto_error(mem, stack, counts, offs, preg, tnfa, REG_ESPACE as c_int);
            unreachable!()
        }
        offs =
            mem::xmalloc(std::mem::size_of::<c_int>() * parse_ctx.position as usize) as *mut c_int;
        if offs.is_null() {
            goto_error(mem, stack, counts, offs, preg, tnfa, REG_ESPACE as c_int);
            unreachable!()
        }

        for i in 0..parse_ctx.position as isize {
            *counts.offset(i) = 0;
        }
        let _errcode = tre_ast_to_tnfa(tree, ptr::null_mut(), counts, ptr::null());

        let mut add: c_int = 0;
        for i in 0..parse_ctx.position as isize {
            *offs.offset(i) = add;
            add += *counts.offset(i) + 1;
            *counts.offset(i) = 0;
        }

        let transitions = mem::xcalloc(
            (add + 1) as usize,
            std::mem::size_of::<tre_tnfa_transition_t>(),
        ) as *mut tre_tnfa_transition_t;
        if transitions.is_null() {
            goto_error(mem, stack, counts, offs, preg, tnfa, REG_ESPACE as c_int);
            unreachable!()
        }
        (*tnfa).transitions = transitions;
        (*tnfa).num_transitions = add as u32;

        let _errcode = tre_ast_to_tnfa(tree, transitions, counts, offs);
        if _errcode != REG_OK as c_int {
            goto_error(mem, stack, counts, offs, preg, tnfa, _errcode);
            unreachable!()
        }

        (*tnfa).first_char = -1;
        (*tnfa).firstpos_chars = ptr::null_mut();

        // Build initial transitions
        let mut p = (*tree).firstpos;
        let mut i: c_int = 0;
        while !p.is_null() && (*p).position >= 0 {
            i += 1;
            p = p.offset(1);
        }

        let initial = mem::xcalloc(
            (i + 1) as usize,
            std::mem::size_of::<tre_tnfa_transition_t>(),
        ) as *mut tre_tnfa_transition_t;
        if initial.is_null() {
            goto_error(mem, stack, counts, offs, preg, tnfa, REG_ESPACE as c_int);
            unreachable!()
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
                let tags =
                    mem::xmalloc(std::mem::size_of::<c_int>() * (j as usize + 1)) as *mut c_int;
                if !tags.is_null() {
                    for k in 0..j as isize {
                        *tags.offset(k) = *(*p).tags.offset(k);
                    }
                    *tags.offset(j) = -1;
                    (*initial.offset(i2 as isize)).tags = tags;
                }
            }
            // Copy params
            if !(*p).params.is_null() {
                let params =
                    mem::xmalloc(std::mem::size_of::<c_int>() * TRE_PARAM_LAST) as *mut c_int;
                if !params.is_null() {
                    for k in 0..TRE_PARAM_LAST {
                        *params.offset(k as isize) = *(*p).params.offset(k as isize);
                    }
                    (*initial.offset(i2 as isize)).params = params;
                }
            }
            i2 += 1;
            p = p.offset(1);
        }
        (*initial.offset(i2 as isize)).state = ptr::null_mut();

        (*tnfa).num_transitions = add as u32;
        (*tnfa).final_ =
            transitions.offset(*offs.offset((*(*tree).lastpos).position as isize) as isize);
        (*tnfa).num_states = parse_ctx.position;
        (*tnfa).cflags = cflags;

        mem::tre_mem_destroy(mem);
        stack::tre_stack_destroy(stack);
        mem::xfree(counts as *mut c_void);
        mem::xfree(offs as *mut c_void);

        (*preg).value = tnfa as *mut c_void;
        REG_OK as c_int
    }
}

unsafe fn goto_error(
    mem: mem::tre_mem_t,
    stack: *mut stack::tre_stack_rec,
    counts: *mut c_int,
    offs: *mut c_int,
    preg: *mut regex_t,
    tnfa: *mut tre_tnfa_t,
    errcode: c_int,
) -> ! {
    unsafe {
        if !mem.is_null() {
            mem::tre_mem_destroy(mem);
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
        if !tnfa.is_null() {
            (*preg).value = tnfa as *mut c_void;
            tre_free(preg);
        }
        std::process::exit(errcode as i32);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_free(preg: *mut regex_t) {
    unsafe {
        if preg.is_null() {
            return;
        }
        let tnfa = (*preg).value as *mut tre_tnfa_t;
        if tnfa.is_null() {
            return;
        }

        let mut i: u32 = 0;
        while i < (*tnfa).num_transitions {
            let trans = (*tnfa).transitions.offset(i as isize);
            if !(*trans).state.is_null() {
                if !(*trans).tags.is_null() {
                    mem::xfree((*trans).tags as *mut c_void);
                }
                if !(*trans).neg_classes.is_null() {
                    mem::xfree((*trans).neg_classes as *mut c_void);
                }
                if !(*trans).params.is_null() {
                    mem::xfree((*trans).params as *mut c_void);
                }
            }
            i += 1;
        }
        if !(*tnfa).transitions.is_null() {
            mem::xfree((*tnfa).transitions as *mut c_void);
        }

        if !(*tnfa).initial.is_null() {
            let mut trans = (*tnfa).initial;
            while !(*trans).state.is_null() {
                if !(*trans).tags.is_null() {
                    mem::xfree((*trans).tags as *mut c_void);
                }
                if !(*trans).params.is_null() {
                    mem::xfree((*trans).params as *mut c_void);
                }
                trans = trans.offset(1);
            }
            mem::xfree((*tnfa).initial as *mut c_void);
        }

        if !(*tnfa).submatch_data.is_null() {
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

        if !(*tnfa).tag_directions.is_null() {
            mem::xfree((*tnfa).tag_directions as *mut c_void);
        }
        if !(*tnfa).firstpos_chars.is_null() {
            mem::xfree((*tnfa).firstpos_chars as *mut c_void);
        }
        if !(*tnfa).minimal_tags.is_null() {
            mem::xfree((*tnfa).minimal_tags as *mut c_void);
        }
        mem::xfree(tnfa as *mut c_void);
    }
}
