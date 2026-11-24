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
use crate::mainutils::grep::RegexWorkspace;

use super::match_parallel::{apply_tags, filled};
use super::owned_arena::OwnedArena;

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
/// # Safety
///
/// `trans_i.neg_classes` must be null or point to a live 0-terminated class array (as built by
/// `tre_compile`).
unsafe fn CHECK_CHAR_CLASSES(
    trans_i: &tre_tnfa_transition_t,
    tnfa: &tre_tnfa_t,
    eflags: c_int,
    prev_c: tre_cint_t,
) -> bool {
    let assertions = trans_i.assertions;
    let icase = tnfa.cflags & REG_ICASE;
    ((assertions & ASSERT_CHAR_CLASS) != 0
        && icase == 0
        && !super::parse::tre_isctype(prev_c, trans_i.u.get_class()))
        || ((assertions & ASSERT_CHAR_CLASS) != 0
            && icase != 0
            && !super::parse::tre_isctype(super::parse::tre_tolower(prev_c), trans_i.u.get_class())
            && !super::parse::tre_isctype(super::parse::tre_toupper(prev_c), trans_i.u.get_class()))
        || ((assertions & ASSERT_CHAR_CLASS_NEG) != 0
            // SAFETY: pointers are live TRE objects forwarded under the enclosing function's `# Safety` contract.
            && unsafe { super::parse::tre_neg_char_classes_match(trans_i.neg_classes, prev_c, icase) } != 0)
}

/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; null is accepted only where the original C TRE API accepts it.
pub unsafe fn tre_tnfa_run_backtrack(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: c_int,
    type_: tre_str_type_t,
    match_tags: *mut c_int,
    eflags: c_int,
    match_end_ofs: *mut c_int,
) -> c_int {
    // SAFETY: forward the legacy pointer contracts unchanged.
    unsafe {
        run_backtrack(
            tnfa,
            string,
            len,
            type_,
            match_tags,
            eflags,
            match_end_ofs,
            &RegexWorkspace::unbounded(),
        )
    }
}

#[allow(clippy::if_same_then_else)]
/// # Safety
/// Same raw pointer requirements as [`tre_tnfa_run_backtrack`].
pub(super) unsafe fn run_backtrack(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: c_int,
    type_: tre_str_type_t,
    match_tags: *mut c_int,
    eflags: c_int,
    match_end_ofs: *mut c_int,
    workspace: &RegexWorkspace,
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

        let (Ok(tag_count), Ok(state_count)) = (
            usize::try_from((*tnfa).num_tags),
            usize::try_from((*tnfa).num_states),
        ) else {
            return REG_BADPAT;
        };
        let Ok(mut tags) = filled(workspace, tag_count, -1) else {
            return REG_ESPACE;
        };
        let Ok(mut pmatch) = filled(
            workspace,
            (*tnfa).num_submatches as usize,
            regmatch_t::default(),
        ) else {
            return REG_ESPACE;
        };
        let Ok(mut states_seen) = filled(workspace, state_count, false) else {
            return REG_ESPACE;
        };
        let Ok(mut arena) = OwnedArena::new(workspace.clone()) else {
            return REG_ESPACE;
        };
        let Ok(base) = arena.allocate(std::mem::size_of::<tre_backtrack_struct>()) else {
            return REG_ESPACE;
        };
        let mut stack: tre_backtrack_t = base.cast();
        (*stack).prev = ptr::null_mut();
        (*stack).next = ptr::null_mut();
        let tag_bytes = std::mem::size_of_val(&tags[..]);

        let mut prev_c: tre_cint_t = 0;
        let mut next_c: tre_cint_t = 0;

        // retry label
        loop {
            // Initialize tags and states_seen
            tags.fill(-1);
            if !match_tags.is_null() && match_eo < 0 {
                for i in 0..tags.len() {
                    match_tags.add(i).write(-1);
                }
            }
            states_seen.fill(false);

            state = ptr::null_mut();
            pos = pos_start;

            // GET_NEXT_WCHAR — for NUL-terminated strings, break if we've
            // gone past the terminator (the inner loop may break without
            // breaking the outer loop when it hits NUL).
            prev_c = next_c;
            if len < 0 && prev_c == 0 && pos_start >= 0 {
                if match_eo >= 0 {
                    break; // have a match, done
                } else {
                    break; // exhausted input, no match
                }
            }
            pos += 1;
            if len >= 0 && pos >= len {
                next_c = 0;
            } else if type_ == tre_str_type_t::STR_WIDE {
                next_c = *(string as *const tre_char_t).add(pos as usize);
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
                        let Ok(frame) = arena.allocate(std::mem::size_of::<tre_backtrack_struct>())
                        else {
                            return REG_ESPACE;
                        };
                        let Ok(row) = arena.allocate(tag_bytes) else {
                            return REG_ESPACE;
                        };
                        let frame: tre_backtrack_t = frame.cast();
                        (*frame).prev = stack;
                        (*frame).next = ptr::null_mut();
                        (*frame).item.tags = row.cast();
                        (*stack).next = frame;
                    }
                    stack = (*stack).next;
                    (*stack).item.pos = pos;
                    (*stack).item.str_byte = str_byte;
                    (*stack).item.state = (*trans_i).state;
                    (*stack).item.state_id = (*trans_i).state_id;
                    (*stack).item.next_c = next_c;
                    let row = std::slice::from_raw_parts_mut((*stack).item.tags, tags.len());
                    row.copy_from_slice(&tags);
                    apply_tags((*trans_i).tags, row, pos);
                }
                trans_i = trans_i.add(1);
            }

            apply_tags(next_tags, &mut tags, pos);

            if state.is_null() {
                // goto backtrack
                if !(*stack).prev.is_null() {
                    if (*(*stack).item.state).assertions & ASSERT_BACKREF != 0 {
                        let Some(seen) = states_seen.get_mut((*stack).item.state_id as usize)
                        else {
                            return REG_BADPAT;
                        };
                        *seen = false;
                    }
                    // BT_STACK_POP
                    pos = (*stack).item.pos;
                    str_byte = (*stack).item.str_byte;
                    state = (*stack).item.state;
                    next_c = (*stack).item.next_c as tre_cint_t;
                    tags.copy_from_slice(std::slice::from_raw_parts((*stack).item.tags, tag_count));
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
                                std::slice::from_raw_parts(
                                    ptr_or_dangling((*tnfa).tag_directions),
                                    tag_count,
                                ),
                                &tags,
                                std::slice::from_raw_parts(ptr_or_dangling(match_tags), tag_count),
                            ) != 0)
                    {
                        match_eo = pos;
                        if !match_tags.is_null() {
                            ptr::copy_nonoverlapping(tags.as_ptr(), match_tags, tags.len());
                        }
                    }
                    // goto backtrack
                    if !(*stack).prev.is_null() {
                        if (*(*stack).item.state).assertions & ASSERT_BACKREF != 0 {
                            let Some(seen) = states_seen.get_mut((*stack).item.state_id as usize)
                            else {
                                return REG_BADPAT;
                            };
                            *seen = false;
                        }
                        pos = (*stack).item.pos;
                        str_byte = (*stack).item.str_byte;
                        state = (*stack).item.state;
                        next_c = (*stack).item.next_c as tre_cint_t;
                        tags.copy_from_slice(std::slice::from_raw_parts(
                            (*stack).item.tags,
                            tag_count,
                        ));
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
                    let Ok(bt) = usize::try_from((*trans_i).u.get_backref()) else {
                        return REG_BADPAT;
                    };
                    let Some(prefix) = pmatch.get_mut(..=bt) else {
                        return REG_BADPAT;
                    };
                    // Get the substring to match against
                    super::regapi::tre_fill_pmatch(
                        prefix.len(),
                        prefix.as_mut_ptr(),
                        (*tnfa).cflags & !REG_NOSUB,
                        tnfa,
                        tags.as_ptr(),
                        pos,
                    );
                    let regmatch_t {
                        rm_so: so,
                        rm_eo: eo,
                    } = prefix[bt];
                    let bt_len = eo - so;

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
                    } else if type_ == tre_str_type_t::STR_WIDE && so >= 0 && bt_len >= 0 {
                        let base = string as *const tre_char_t;
                        result =
                            if std::slice::from_raw_parts(base.add(so as usize), bt_len as usize)
                                == std::slice::from_raw_parts(
                                    base.add(pos as usize),
                                    bt_len as usize,
                                )
                            {
                                0
                            } else {
                                1
                            };
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
                        let Some(&seen) = states_seen.get((*trans_i).state_id as usize) else {
                            return REG_BADPAT;
                        };
                        if empty_br_match != 0 && seen {
                            // goto backtrack
                            if !(*stack).prev.is_null() {
                                if (*(*stack).item.state).assertions & ASSERT_BACKREF != 0 {
                                    let Some(seen) =
                                        states_seen.get_mut((*stack).item.state_id as usize)
                                    else {
                                        return REG_BADPAT;
                                    };
                                    *seen = false;
                                }
                                pos = (*stack).item.pos;
                                str_byte = (*stack).item.str_byte;
                                state = (*stack).item.state;
                                next_c = (*stack).item.next_c as tre_cint_t;
                                tags.copy_from_slice(std::slice::from_raw_parts(
                                    (*stack).item.tags,
                                    tag_count,
                                ));
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
                        states_seen[(*trans_i).state_id as usize] = empty_br_match != 0;
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
                        } else if type_ == tre_str_type_t::STR_WIDE {
                            next_c = *(string as *const tre_char_t).add(pos as usize);
                        } else if type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
                            next_c = *str_byte.add(pos as usize) as tre_cint_t;
                        } else {
                            next_c = 0;
                        }
                    } else {
                        // goto backtrack
                        if !(*stack).prev.is_null() {
                            if (*(*stack).item.state).assertions & ASSERT_BACKREF != 0 {
                                let Some(seen) =
                                    states_seen.get_mut((*stack).item.state_id as usize)
                                else {
                                    return REG_BADPAT;
                                };
                                *seen = false;
                            }
                            pos = (*stack).item.pos;
                            str_byte = (*stack).item.str_byte;
                            state = (*stack).item.state;
                            next_c = (*stack).item.next_c as tre_cint_t;
                            tags.copy_from_slice(std::slice::from_raw_parts(
                                (*stack).item.tags,
                                tag_count,
                            ));
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
                                let Some(seen) =
                                    states_seen.get_mut((*stack).item.state_id as usize)
                                else {
                                    return REG_BADPAT;
                                };
                                *seen = false;
                            }
                            pos = (*stack).item.pos;
                            str_byte = (*stack).item.str_byte;
                            state = (*stack).item.state;
                            next_c = (*stack).item.next_c as tre_cint_t;
                            tags.copy_from_slice(std::slice::from_raw_parts(
                                (*stack).item.tags,
                                tag_count,
                            ));
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
                                let Some(seen) =
                                    states_seen.get_mut((*stack).item.state_id as usize)
                                else {
                                    return REG_BADPAT;
                                };
                                *seen = false;
                            }
                            pos = (*stack).item.pos;
                            str_byte = (*stack).item.str_byte;
                            state = (*stack).item.state;
                            next_c = (*stack).item.next_c as tre_cint_t;
                            tags.copy_from_slice(std::slice::from_raw_parts(
                                (*stack).item.tags,
                                tag_count,
                            ));
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
                    } else if type_ == tre_str_type_t::STR_WIDE {
                        next_c = *(string as *const tre_char_t).add(pos as usize);
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
                            ) || CHECK_CHAR_CLASSES(&*trans_i, &*tnfa, eflags, prev_c))
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
                                let Ok(frame) =
                                    arena.allocate(std::mem::size_of::<tre_backtrack_struct>())
                                else {
                                    return REG_ESPACE;
                                };
                                let Ok(row) = arena.allocate(tag_bytes) else {
                                    return REG_ESPACE;
                                };
                                let frame: tre_backtrack_t = frame.cast();
                                (*frame).prev = stack;
                                (*frame).next = ptr::null_mut();
                                (*frame).item.tags = row.cast();
                                (*stack).next = frame;
                            }
                            stack = (*stack).next;
                            (*stack).item.pos = pos;
                            (*stack).item.str_byte = str_byte;
                            (*stack).item.state = (*trans_i).state;
                            (*stack).item.state_id = (*trans_i).state_id;
                            (*stack).item.next_c = next_c;
                            let row =
                                std::slice::from_raw_parts_mut((*stack).item.tags, tags.len());
                            row.copy_from_slice(&tags);
                            apply_tags((*trans_i).tags, row, pos);
                        }
                    }
                    trans_i = trans_i.add(1);
                }

                if !next_state.is_null() {
                    state = next_state;
                    apply_tags(next_tags, &mut tags, pos);
                } else {
                    // goto backtrack
                    if !(*stack).prev.is_null() {
                        if (*(*stack).item.state).assertions & ASSERT_BACKREF != 0 {
                            let Some(seen) = states_seen.get_mut((*stack).item.state_id as usize)
                            else {
                                return REG_BADPAT;
                            };
                            *seen = false;
                        }
                        pos = (*stack).item.pos;
                        str_byte = (*stack).item.str_byte;
                        state = (*stack).item.state;
                        next_c = (*stack).item.next_c as tre_cint_t;
                        tags.copy_from_slice(std::slice::from_raw_parts(
                            (*stack).item.tags,
                            tag_count,
                        ));
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
            // Only an explicit retry advances to another starting position.
            if !done {
                break;
            }
        }

        let ret = if match_eo >= 0 {
            REG_OK as c_int
        } else {
            REG_NOMATCH as c_int
        };
        *match_end_ofs = match_eo;

        ret
    }
}

/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; null is accepted only where the original C TRE API accepts it.
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
