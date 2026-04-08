#![allow(unused_variables)]
#![allow(unused_assignments)]
/*
  tre/match_backtrack.rs - TRE backtracking regex matching engine

  Ported from tre-match-backtrack.c
*/
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::os::raw::{c_int, c_void};
use std::ptr;

use super::ast::*;
use super::mem;

#[repr(C)]
struct tre_backtrack_item_t {
    pos: c_int,
    str_byte: *const u8,
    state: *mut tre_tnfa_transition_t,
    state_id: c_int,
    next_c: tre_cint_t,
    tags: *mut c_int,
}

#[repr(C)]
struct tre_backtrack_struct {
    item: tre_backtrack_item_t,
    prev: *mut tre_backtrack_struct,
    next: *mut tre_backtrack_struct,
}

type tre_backtrack_t = *mut tre_backtrack_struct;

#[inline]
fn IS_WORD_CHAR(c: tre_cint_t) -> bool {
    c == '_' as tre_cint_t || super::parse::tre_isalnum(c)
}

#[inline]
fn CHECK_ASSERTIONS(
    assertions: c_int,
    pos: c_int,
    prev_c: tre_cint_t,
    next_c: tre_cint_t,
    reg_notbol: c_int,
    reg_noteol: c_int,
    reg_newline: c_int,
) -> bool {
    ((assertions & ASSERT_AT_BOL) != 0
        && (pos > 0 || reg_notbol != 0)
        && (prev_c != '\n' as tre_cint_t || reg_newline == 0))
        || ((assertions & ASSERT_AT_EOL) != 0
            && (next_c != 0 || reg_noteol != 0)
            && (next_c != '\n' as tre_cint_t || reg_newline == 0))
        || ((assertions & ASSERT_AT_BOW) != 0 && (IS_WORD_CHAR(prev_c) || !IS_WORD_CHAR(next_c)))
        || ((assertions & ASSERT_AT_EOW) != 0 && (!IS_WORD_CHAR(prev_c) || IS_WORD_CHAR(next_c)))
        || ((assertions & ASSERT_AT_WB) != 0
            && (pos != 0 && next_c != 0 && IS_WORD_CHAR(prev_c) == IS_WORD_CHAR(next_c)))
        || ((assertions & ASSERT_AT_WB_NEG) != 0
            && (pos == 0 || next_c == 0 || IS_WORD_CHAR(prev_c) != IS_WORD_CHAR(next_c)))
}

#[inline]
unsafe fn CHECK_CHAR_CLASSES(
    trans_i: *const tre_tnfa_transition_t,
    tnfa: *const tre_tnfa_t,
    eflags: c_int,
    prev_c: tre_cint_t,
) -> bool {
    unsafe {
        let assertions = (*trans_i).assertions;
        let icase = (*tnfa).cflags & REG_ICASE;
        ((assertions & ASSERT_CHAR_CLASS) != 0
            && icase == 0
            && !super::parse::tre_isctype(prev_c, (*trans_i).u.get_class()))
            || ((assertions & ASSERT_CHAR_CLASS) != 0
                && icase != 0
                && !super::parse::tre_isctype(
                    super::parse::tre_tolower(prev_c),
                    (*trans_i).u.get_class(),
                )
                && !super::parse::tre_isctype(
                    super::parse::tre_toupper(prev_c),
                    (*trans_i).u.get_class(),
                ))
            || ((assertions & ASSERT_CHAR_CLASS_NEG) != 0
                && super::parse::tre_neg_char_classes_match((*trans_i).neg_classes, prev_c, icase)
                    != 0)
    }
}

#[allow(clippy::if_same_then_else)]
pub unsafe fn tre_tnfa_run_backtrack(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: c_int,
    type_: tre_str_type_t,
    match_tags: *mut c_int,
    eflags: c_int,
    match_end_ofs: *mut c_int,
) -> c_int {
    unsafe {
        let mut str_byte = string as *const u8;
        let mut pos: c_int = 0;
        let pos_add_next: u32 = 1;

        let reg_notbol = eflags & REG_NOTBOL;
        let reg_noteol = eflags & REG_NOTEOL;
        let reg_newline = (*tnfa).cflags & REG_NEWLINE;

        let mut next_c_start: tre_cint_t = 0;
        let mut str_byte_start: *const u8 = ptr::null();
        let mut pos_start: c_int = -1;

        let mut match_eo: c_int = -1;
        let mut next_tags: *const c_int = ptr::null();
        let mut state: *mut tre_tnfa_transition_t = ptr::null_mut();

        let mem_ctx = mem::tre_mem_new();
        if mem_ctx.is_null() {
            return REG_ESPACE as c_int;
        }

        let mut stack: tre_backtrack_t =
            mem::tre_mem_alloc(mem_ctx, std::mem::size_of::<tre_backtrack_struct>())
                as tre_backtrack_t;
        if stack.is_null() {
            mem::tre_mem_destroy(mem_ctx);
            return REG_ESPACE as c_int;
        }
        (*stack).prev = ptr::null_mut();
        (*stack).next = ptr::null_mut();

        let tags: *mut c_int = if (*tnfa).num_tags > 0 {
            let t = mem::xmalloc(std::mem::size_of::<c_int>() * (*tnfa).num_tags as usize)
                as *mut c_int;
            if t.is_null() {
                mem::tre_mem_destroy(mem_ctx);
                return REG_ESPACE as c_int;
            }
            t
        } else {
            ptr::null_mut()
        };

        let pmatch: *mut regmatch_t = if (*tnfa).num_submatches > 0 {
            let p =
                mem::xmalloc(std::mem::size_of::<regmatch_t>() * (*tnfa).num_submatches as usize)
                    as *mut regmatch_t;
            if p.is_null() {
                if !tags.is_null() {
                    mem::xfree(tags as *mut c_void);
                }
                mem::tre_mem_destroy(mem_ctx);
                return REG_ESPACE as c_int;
            }
            p
        } else {
            ptr::null_mut()
        };

        let states_seen: *mut c_int = if (*tnfa).num_states > 0 {
            let s = mem::xmalloc(std::mem::size_of::<c_int>() * (*tnfa).num_states as usize)
                as *mut c_int;
            if s.is_null() {
                if !tags.is_null() {
                    mem::xfree(tags as *mut c_void);
                }
                if !pmatch.is_null() {
                    mem::xfree(pmatch as *mut c_void);
                }
                mem::tre_mem_destroy(mem_ctx);
                return REG_ESPACE as c_int;
            }
            s
        } else {
            ptr::null_mut()
        };

        let mut prev_c: tre_cint_t = 0;
        let mut next_c: tre_cint_t = 0;

        // retry label
        loop {
            // Initialize tags and states_seen
            for i in 0..(*tnfa).num_tags {
                *tags.offset(i as isize) = -1;
                if !match_tags.is_null() {
                    *match_tags.offset(i as isize) = -1;
                }
            }
            for i in 0..(*tnfa).num_states {
                *states_seen.offset(i as isize) = 0;
            }

            state = ptr::null_mut();
            pos = pos_start;

            // GET_NEXT_WCHAR
            prev_c = next_c;
            pos += 1;
            if len >= 0 && pos >= len {
                next_c = 0;
            } else if type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
                next_c = *str_byte.add(pos as usize) as tre_cint_t;
            } else {
                next_c = 0;
            }

            pos_start = pos;
            next_c_start = next_c;
            str_byte_start = if !str_byte.is_null() {
                str_byte
            } else {
                ptr::null()
            };

            // Handle initial states
            next_tags = ptr::null();
            let mut trans_i = (*tnfa).initial;
            while !(*trans_i).state.is_null() {
                if (*trans_i).assertions != 0
                    && CHECK_ASSERTIONS(
                        (*trans_i).assertions,
                        pos,
                        prev_c,
                        next_c,
                        reg_notbol,
                        reg_noteol,
                        reg_newline,
                    )
                {
                    trans_i = trans_i.add(1);
                    continue;
                }
                if state.is_null() {
                    state = (*trans_i).state;
                    next_tags = (*trans_i).tags;
                } else {
                    // Backtrack to this state
                    // BT_STACK_PUSH
                    if (*stack).next.is_null() {
                        let s = mem::tre_mem_alloc(
                            mem_ctx,
                            std::mem::size_of::<tre_backtrack_struct>(),
                        ) as tre_backtrack_t;
                        if s.is_null() {
                            mem::tre_mem_destroy(mem_ctx);
                            if !tags.is_null() {
                                mem::xfree(tags as *mut c_void);
                            }
                            if !pmatch.is_null() {
                                mem::xfree(pmatch as *mut c_void);
                            }
                            if !states_seen.is_null() {
                                mem::xfree(states_seen as *mut c_void);
                            }
                            return REG_ESPACE as c_int;
                        }
                        (*s).prev = stack;
                        (*s).next = ptr::null_mut();
                        (*s).item.tags = mem::tre_mem_alloc(
                            mem_ctx,
                            std::mem::size_of::<c_int>() * (*tnfa).num_tags as usize,
                        ) as *mut c_int;
                        if (*s).item.tags.is_null() {
                            mem::tre_mem_destroy(mem_ctx);
                            if !tags.is_null() {
                                mem::xfree(tags as *mut c_void);
                            }
                            if !pmatch.is_null() {
                                mem::xfree(pmatch as *mut c_void);
                            }
                            if !states_seen.is_null() {
                                mem::xfree(states_seen as *mut c_void);
                            }
                            return REG_ESPACE as c_int;
                        }
                        (*stack).next = s;
                        stack = s;
                    } else {
                        stack = (*stack).next;
                    }
                    (*stack).item.pos = pos;
                    (*stack).item.str_byte = str_byte;
                    (*stack).item.state = (*trans_i).state;
                    (*stack).item.state_id = (*trans_i).state_id;
                    (*stack).item.next_c = next_c;
                    for i in 0..(*tnfa).num_tags {
                        *(*stack).item.tags.offset(i as isize) = *tags.offset(i as isize);
                    }

                    // Apply tags from this transition
                    let mut tmp = (*trans_i).tags;
                    if !tmp.is_null() {
                        while *tmp >= 0 {
                            if *tmp < (*tnfa).num_tags {
                                *(*stack).item.tags.offset(*tmp as isize) = pos;
                            }
                            tmp = tmp.add(1);
                        }
                    }
                }
                trans_i = trans_i.add(1);
            }

            if !next_tags.is_null() {
                let mut nt = next_tags;
                while *nt >= 0 {
                    *tags.offset(*nt as isize) = pos;
                    nt = nt.add(1);
                }
            }

            if state.is_null() {
                // goto backtrack
                if !(*stack).prev.is_null() {
                    if (*(*stack).item.state).assertions & ASSERT_BACKREF != 0 {
                        *states_seen.offset((*stack).item.state_id as isize) = 0;
                    }
                    // BT_STACK_POP
                    pos = (*stack).item.pos;
                    str_byte = (*stack).item.str_byte;
                    state = (*stack).item.state;
                    next_c = (*stack).item.next_c as tre_cint_t;
                    for i in 0..(*tnfa).num_tags {
                        *tags.offset(i as isize) = *(*stack).item.tags.offset(i as isize);
                    }
                    stack = (*stack).prev;
                    continue; // Back to main loop
                } else if match_eo < 0 {
                    // Try starting from a later position
                    if len >= 0 && pos >= len {
                        break;
                    }
                    if next_c == 0 {
                        break;
                    }
                    next_c = next_c_start;
                    str_byte = str_byte_start;
                    continue; // retry
                } else {
                    break;
                }
            }

            // Main matching loop
            let mut done = false;
            while !done {
                if state == (*tnfa).final_ {
                    if match_eo < pos
                        || (match_eo == pos
                            && !match_tags.is_null()
                            && tre_tag_order(
                                (*tnfa).num_tags,
                                (*tnfa).tag_directions,
                                tags,
                                match_tags,
                            ) != 0)
                    {
                        match_eo = pos;
                        if !match_tags.is_null() {
                            for i in 0..(*tnfa).num_tags {
                                *match_tags.offset(i as isize) = *tags.offset(i as isize);
                            }
                        }
                    }
                    // goto backtrack
                    if !(*stack).prev.is_null() {
                        if (*(*stack).item.state).assertions & ASSERT_BACKREF != 0 {
                            *states_seen.offset((*stack).item.state_id as isize) = 0;
                        }
                        pos = (*stack).item.pos;
                        str_byte = (*stack).item.str_byte;
                        state = (*stack).item.state;
                        next_c = (*stack).item.next_c as tre_cint_t;
                        for i in 0..(*tnfa).num_tags {
                            *tags.offset(i as isize) = *(*stack).item.tags.offset(i as isize);
                        }
                        stack = (*stack).prev;
                        continue;
                    } else if match_eo < 0 {
                        if len >= 0 && pos >= len {
                            break;
                        }
                        if next_c == 0 {
                            break;
                        }
                        next_c = next_c_start;
                        str_byte = str_byte_start;
                        done = true;
                    } else {
                        break;
                    }
                    continue;
                }

                let mut empty_br_match: c_int = 0;
                let trans_i = state;

                if !(*trans_i).state.is_null() && (*trans_i).assertions & ASSERT_BACKREF != 0 {
                    // Back reference
                    let bt = (*trans_i).u.get_backref();
                    let bt_len: c_int;

                    // Get the substring to match against
                    super::regapi::tre_fill_pmatch(
                        (bt + 1) as usize,
                        pmatch,
                        (*tnfa).cflags & !REG_NOSUB,
                        tnfa,
                        tags,
                        pos,
                    );
                    let so = (*pmatch.offset(bt as isize)).rm_so;
                    let eo = (*pmatch.offset(bt as isize)).rm_eo;
                    bt_len = eo - so;

                    let result: c_int;
                    if len < 0 {
                        if type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
                            result = libc_memcmp(
                                string as *const u8,
                                so as usize,
                                str_byte.sub(1),
                                bt_len,
                            );
                        } else {
                            result = 1;
                        }
                    } else if len - pos < bt_len {
                        result = 1;
                    } else if type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
                        result =
                            libc_memcmp(string as *const u8, so as usize, str_byte.sub(1), bt_len);
                    } else {
                        result = 1;
                    }

                    if result == 0 {
                        if bt_len == 0 {
                            empty_br_match = 1;
                        }
                        if empty_br_match != 0
                            && *states_seen.offset((*trans_i).state_id as isize) != 0
                        {
                            // goto backtrack
                            if !(*stack).prev.is_null() {
                                if (*(*stack).item.state).assertions & ASSERT_BACKREF != 0 {
                                    *states_seen.offset((*stack).item.state_id as isize) = 0;
                                }
                                pos = (*stack).item.pos;
                                str_byte = (*stack).item.str_byte;
                                state = (*stack).item.state;
                                next_c = (*stack).item.next_c as tre_cint_t;
                                for i in 0..(*tnfa).num_tags {
                                    *tags.offset(i as isize) =
                                        *(*stack).item.tags.offset(i as isize);
                                }
                                stack = (*stack).prev;
                                continue;
                            } else if match_eo < 0 {
                                if next_c == 0 {
                                    break;
                                }
                                next_c = next_c_start;
                                str_byte = str_byte_start;
                                done = true;
                            } else {
                                break;
                            }
                            continue;
                        }
                        *states_seen.offset((*trans_i).state_id as isize) = empty_br_match;
                        // Advance in input string
                        if type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
                            str_byte = str_byte.offset(bt_len as isize - 1);
                        }
                        pos += bt_len - 1;
                        // GET_NEXT_WCHAR
                        prev_c = next_c;
                        pos += 1;
                        if len >= 0 && pos >= len {
                            next_c = 0;
                        } else if type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
                            next_c = *str_byte.add(pos as usize) as tre_cint_t;
                        } else {
                            next_c = 0;
                        }
                    } else {
                        // goto backtrack
                        if !(*stack).prev.is_null() {
                            if (*(*stack).item.state).assertions & ASSERT_BACKREF != 0 {
                                *states_seen.offset((*stack).item.state_id as isize) = 0;
                            }
                            pos = (*stack).item.pos;
                            str_byte = (*stack).item.str_byte;
                            state = (*stack).item.state;
                            next_c = (*stack).item.next_c as tre_cint_t;
                            for i in 0..(*tnfa).num_tags {
                                *tags.offset(i as isize) = *(*stack).item.tags.offset(i as isize);
                            }
                            stack = (*stack).prev;
                            continue;
                        } else if match_eo < 0 {
                            if next_c == 0 {
                                break;
                            }
                            next_c = next_c_start;
                            str_byte = str_byte_start;
                            done = true;
                        } else {
                            break;
                        }
                        continue;
                    }
                } else {
                    // Check for end of string
                    if len >= 0 && pos >= len {
                        // goto backtrack
                        if !(*stack).prev.is_null() {
                            if (*(*stack).item.state).assertions & ASSERT_BACKREF != 0 {
                                *states_seen.offset((*stack).item.state_id as isize) = 0;
                            }
                            pos = (*stack).item.pos;
                            str_byte = (*stack).item.str_byte;
                            state = (*stack).item.state;
                            next_c = (*stack).item.next_c as tre_cint_t;
                            for i in 0..(*tnfa).num_tags {
                                *tags.offset(i as isize) = *(*stack).item.tags.offset(i as isize);
                            }
                            stack = (*stack).prev;
                            continue;
                        } else if match_eo < 0 {
                            if next_c == 0 {
                                break;
                            }
                            next_c = next_c_start;
                            str_byte = str_byte_start;
                            done = true;
                        } else {
                            break;
                        }
                        continue;
                    }
                    if len < 0 && next_c == 0 {
                        // goto backtrack
                        if !(*stack).prev.is_null() {
                            if (*(*stack).item.state).assertions & ASSERT_BACKREF != 0 {
                                *states_seen.offset((*stack).item.state_id as isize) = 0;
                            }
                            pos = (*stack).item.pos;
                            str_byte = (*stack).item.str_byte;
                            state = (*stack).item.state;
                            next_c = (*stack).item.next_c as tre_cint_t;
                            for i in 0..(*tnfa).num_tags {
                                *tags.offset(i as isize) = *(*stack).item.tags.offset(i as isize);
                            }
                            stack = (*stack).prev;
                            continue;
                        } else if match_eo < 0 {
                            break;
                        } else {
                            break;
                        }
                    }

                    // GET_NEXT_WCHAR
                    prev_c = next_c;
                    pos += 1;
                    if len >= 0 && pos >= len {
                        next_c = 0;
                    } else if type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
                        next_c = *str_byte.add(pos as usize) as tre_cint_t;
                    } else {
                        next_c = 0;
                    }
                }

                let mut next_state: *mut tre_tnfa_transition_t = ptr::null_mut();
                let mut trans_i = state;
                while !(*trans_i).state.is_null() {
                    if (*trans_i).code_min <= prev_c && (*trans_i).code_max >= prev_c {
                        if (*trans_i).assertions != 0
                            && (CHECK_ASSERTIONS(
                                (*trans_i).assertions,
                                pos,
                                prev_c,
                                next_c,
                                reg_notbol,
                                reg_noteol,
                                reg_newline,
                            ) || CHECK_CHAR_CLASSES(trans_i, tnfa, eflags, prev_c))
                        {
                            trans_i = trans_i.add(1);
                            continue;
                        }

                        if next_state.is_null() {
                            next_state = (*trans_i).state;
                            next_tags = (*trans_i).tags;
                        } else {
                            // BT_STACK_PUSH
                            if (*stack).next.is_null() {
                                let s = mem::tre_mem_alloc(
                                    mem_ctx,
                                    std::mem::size_of::<tre_backtrack_struct>(),
                                ) as tre_backtrack_t;
                                if s.is_null() {
                                    mem::tre_mem_destroy(mem_ctx);
                                    if !tags.is_null() {
                                        mem::xfree(tags as *mut c_void);
                                    }
                                    if !pmatch.is_null() {
                                        mem::xfree(pmatch as *mut c_void);
                                    }
                                    if !states_seen.is_null() {
                                        mem::xfree(states_seen as *mut c_void);
                                    }
                                    return REG_ESPACE as c_int;
                                }
                                (*s).prev = stack;
                                (*s).next = ptr::null_mut();
                                (*s).item.tags = mem::tre_mem_alloc(
                                    mem_ctx,
                                    std::mem::size_of::<c_int>() * (*tnfa).num_tags as usize,
                                ) as *mut c_int;
                                if (*s).item.tags.is_null() {
                                    mem::tre_mem_destroy(mem_ctx);
                                    if !tags.is_null() {
                                        mem::xfree(tags as *mut c_void);
                                    }
                                    if !pmatch.is_null() {
                                        mem::xfree(pmatch as *mut c_void);
                                    }
                                    if !states_seen.is_null() {
                                        mem::xfree(states_seen as *mut c_void);
                                    }
                                    return REG_ESPACE as c_int;
                                }
                                (*stack).next = s;
                                stack = s;
                            } else {
                                stack = (*stack).next;
                            }
                            (*stack).item.pos = pos;
                            (*stack).item.str_byte = str_byte;
                            (*stack).item.state = (*trans_i).state;
                            (*stack).item.state_id = (*trans_i).state_id;
                            (*stack).item.next_c = next_c;
                            for i in 0..(*tnfa).num_tags {
                                *(*stack).item.tags.offset(i as isize) = *tags.offset(i as isize);
                            }
                            // Apply tags
                            let mut tmp = (*trans_i).tags;
                            if !tmp.is_null() {
                                while *tmp >= 0 {
                                    if *tmp < (*tnfa).num_tags {
                                        *(*stack).item.tags.offset(*tmp as isize) = pos;
                                    }
                                    tmp = tmp.add(1);
                                }
                            }
                        }
                    }
                    trans_i = trans_i.add(1);
                }

                if !next_state.is_null() {
                    state = next_state;
                    if !next_tags.is_null() {
                        let mut nt = next_tags;
                        while *nt >= 0 {
                            *tags.offset(*nt as isize) = pos;
                            nt = nt.add(1);
                        }
                    }
                } else {
                    // goto backtrack
                    if !(*stack).prev.is_null() {
                        if (*(*stack).item.state).assertions & ASSERT_BACKREF != 0 {
                            *states_seen.offset((*stack).item.state_id as isize) = 0;
                        }
                        pos = (*stack).item.pos;
                        str_byte = (*stack).item.str_byte;
                        state = (*stack).item.state;
                        next_c = (*stack).item.next_c as tre_cint_t;
                        for i in 0..(*tnfa).num_tags {
                            *tags.offset(i as isize) = *(*stack).item.tags.offset(i as isize);
                        }
                        stack = (*stack).prev;
                    } else if match_eo < 0 {
                        if len >= 0 && pos >= len {
                            break;
                        }
                        if next_c == 0 {
                            break;
                        }
                        next_c = next_c_start;
                        str_byte = str_byte_start;
                        done = true;
                    } else {
                        break;
                    }
                }
            }
        }

        let ret = if match_eo >= 0 {
            REG_OK as c_int
        } else {
            REG_NOMATCH as c_int
        };
        *match_end_ofs = match_eo;

        mem::tre_mem_destroy(mem_ctx);
        if !tags.is_null() {
            mem::xfree(tags as *mut c_void);
        }
        if !pmatch.is_null() {
            mem::xfree(pmatch as *mut c_void);
        }
        if !states_seen.is_null() {
            mem::xfree(states_seen as *mut c_void);
        }

        ret
    }
}

unsafe fn libc_memcmp(s1: *const u8, s1_offset: usize, s2: *const u8, n: c_int) -> c_int {
    unsafe {
        if n <= 0 {
            return 0;
        }
        for i in 0..n as usize {
            let a = *s1.add(s1_offset + i);
            let b = *s2.add(i);
            if a < b {
                return -1;
            }
            if a > b {
                return 1;
            }
        }
        0
    }
}
