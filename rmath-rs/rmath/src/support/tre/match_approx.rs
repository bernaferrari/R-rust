/*
  tre/match_approx.rs - TRE approximate regex matching engine

  Ported from tre-match-approx.c
*/

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::os::raw::{c_int, c_void};
use std::ptr;

use super::ast::*;
use super::mem;

const TRE_M_COST: usize = 0;
const TRE_M_NUM_INS: usize = 1;
const TRE_M_NUM_DEL: usize = 2;
const TRE_M_NUM_SUBST: usize = 3;
const TRE_M_NUM_ERR: usize = 4;
const TRE_M_LAST: usize = 5;
const TRE_M_MAX_DEPTH: usize = 3;

#[repr(C)]
struct tre_tnfa_approx_reach_t {
    state: *mut tre_tnfa_transition_t,
    pos: c_int,
    tags: *mut c_int,
    params: regaparams_t,
    depth: c_int,
    costs: [[c_int; TRE_M_LAST]; TRE_M_MAX_DEPTH + 1],
}

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
    _eflags: c_int,
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

unsafe fn tre_set_params(
    reach: *mut tre_tnfa_approx_reach_t,
    pa: *const c_int,
    default_params: regaparams_t,
) {
    unsafe {
        let value = *pa.offset(TRE_PARAM_DEPTH as isize);
        if value > (*reach).depth {
            for i in ((*reach).depth as usize + 1)..=value as usize {
                for j in 0..TRE_M_LAST {
                    (*reach).costs[i][j] = 0;
                }
            }
        }
        (*reach).depth = value;

        let mut v = *pa.offset(TRE_PARAM_COST_INS as isize);
        if v == TRE_PARAM_DEFAULT {
            (*reach).params.cost_ins = default_params.cost_ins;
        } else if v != TRE_PARAM_UNSET {
            (*reach).params.cost_ins = v;
        }

        v = *pa.offset(TRE_PARAM_COST_DEL as isize);
        if v == TRE_PARAM_DEFAULT {
            (*reach).params.cost_del = default_params.cost_del;
        } else if v != TRE_PARAM_UNSET {
            (*reach).params.cost_del = v;
        }

        v = *pa.offset(TRE_PARAM_COST_SUBST as isize);
        if v == TRE_PARAM_DEFAULT {
            (*reach).params.cost_subst = default_params.cost_subst;
        } else {
            (*reach).params.cost_subst = v;
        }

        v = *pa.offset(TRE_PARAM_COST_MAX as isize);
        if v == TRE_PARAM_DEFAULT {
            (*reach).params.max_cost = default_params.max_cost;
        } else if v != TRE_PARAM_UNSET {
            (*reach).params.max_cost = v;
        }

        v = *pa.offset(TRE_PARAM_MAX_INS as isize);
        if v == TRE_PARAM_DEFAULT {
            (*reach).params.max_ins = default_params.max_ins;
        } else if v != TRE_PARAM_UNSET {
            (*reach).params.max_ins = v;
        }

        v = *pa.offset(TRE_PARAM_MAX_DEL as isize);
        if v == TRE_PARAM_DEFAULT {
            (*reach).params.max_del = default_params.max_del;
        } else if v != TRE_PARAM_UNSET {
            (*reach).params.max_del = v;
        }

        v = *pa.offset(TRE_PARAM_MAX_SUBST as isize);
        if v == TRE_PARAM_DEFAULT {
            (*reach).params.max_subst = default_params.max_subst;
        } else if v != TRE_PARAM_UNSET {
            (*reach).params.max_subst = v;
        }

        v = *pa.offset(TRE_PARAM_MAX_ERR as isize);
        if v == TRE_PARAM_DEFAULT {
            (*reach).params.max_err = default_params.max_err;
        } else if v != TRE_PARAM_UNSET {
            (*reach).params.max_err = v;
        }
    }
}

pub unsafe fn tre_tnfa_run_approx(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: c_int,
    type_: tre_str_type_t,
    match_tags: *mut c_int,
    amatch: *mut regamatch_t,
    default_params: regaparams_t,
    eflags: c_int,
    match_end_ofs: *mut c_int,
) -> c_int {
    unsafe {
        let str_byte = string as *const u8;
        let mut pos: c_int = -1;
        let _pos_add_next: u32 = 1;
        let reg_notbol = eflags & REG_NOTBOL;
        let reg_noteol = eflags & REG_NOTEOL;
        let reg_newline = (*tnfa).cflags & REG_NEWLINE;

        let mut prev_c: tre_cint_t;
        let mut next_c: tre_cint_t = 0;

        let num_tags = if match_tags.is_null() {
            0
        } else {
            (*tnfa).num_tags
        };
        let mut prev_pos: c_int;

        let mut match_eo: c_int = -1;
        let mut match_costs = [c_int::MAX; TRE_M_LAST];

        // Allocate buffers
        let tag_bytes = std::mem::size_of::<c_int>() * num_tags as usize;
        let reach_bytes =
            std::mem::size_of::<tre_tnfa_approx_reach_t>() * (*tnfa).num_states as usize;
        let total_bytes = reach_bytes * 2 + ((*tnfa).num_states as usize * 2 + 1) * tag_bytes + 64;

        let buf = mem::xmalloc(total_bytes);
        if buf.is_null() {
            return REG_ESPACE as c_int;
        }
        ptr::write_bytes(buf as *mut u8, 0, total_bytes);

        let mut buf_offset = buf as *mut u8;
        let tmp_tags = buf_offset as *mut c_int;
        buf_offset = buf_offset.add(tag_bytes);
        buf_offset = align_ptr_approx(buf_offset);

        let reach = buf_offset as *mut tre_tnfa_approx_reach_t;
        buf_offset = buf_offset.add(reach_bytes);
        buf_offset = align_ptr_approx(buf_offset);

        let reach_next = buf_offset as *mut tre_tnfa_approx_reach_t;
        buf_offset = buf_offset.add(reach_bytes);
        buf_offset = align_ptr_approx(buf_offset);

        for i in 0..(*tnfa).num_states {
            (*reach.offset(i as isize)).tags = buf_offset as *mut c_int;
            buf_offset = buf_offset.add(tag_bytes);
            (*reach_next.offset(i as isize)).tags = buf_offset as *mut c_int;
            buf_offset = buf_offset.add(tag_bytes);
        }

        for i in 0..(*tnfa).num_states {
            (*reach.offset(i as isize)).pos = -2;
            (*reach_next.offset(i as isize)).pos = -2;
        }

        prev_pos = pos;
        // GET_NEXT_WCHAR
        prev_c = next_c;
        pos = 0;
        if len >= 0 && pos >= len {
            next_c = 0;
        } else if type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
            next_c = *str_byte as tre_cint_t;
        } else {
            next_c = 0;
        }

        loop {
            // Add initial states
            if match_costs[TRE_M_COST] > 0 {
                let mut trans = (*tnfa).initial;
                while !(*trans).state.is_null() {
                    let stateid = (*trans).state_id;
                    if (*reach_next.offset(stateid as isize)).pos < pos {
                        if (*trans).assertions != 0
                            && CHECK_ASSERTIONS(
                                (*trans).assertions,
                                pos,
                                prev_c,
                                next_c,
                                reg_notbol,
                                reg_noteol,
                                reg_newline,
                            )
                        {
                            trans = trans.offset(1);
                            continue;
                        }
                        (*reach_next.offset(stateid as isize)).state = (*trans).state;
                        (*reach_next.offset(stateid as isize)).pos = pos;

                        for i in 0..num_tags {
                            *(*reach_next.offset(stateid as isize))
                                .tags
                                .offset(i as isize) = -1;
                        }
                        if !(*trans).tags.is_null() {
                            let mut ti: c_int = 0;
                            while *(*trans).tags.offset(ti as isize) >= 0 {
                                if *(*trans).tags.offset(ti as isize) < num_tags {
                                    *(*reach_next.offset(stateid as isize))
                                        .tags
                                        .offset(*(*trans).tags.offset(ti as isize) as isize) = pos;
                                }
                                ti += 1;
                            }
                        }

                        (*reach_next.offset(stateid as isize)).params = default_params;
                        (*reach_next.offset(stateid as isize)).depth = 0;
                        for j in 0..TRE_M_LAST {
                            (*reach_next.offset(stateid as isize)).costs[0][j] = 0;
                        }
                        if !(*trans).params.is_null() {
                            tre_set_params(
                                &mut *reach_next.offset(stateid as isize),
                                (*trans).params,
                                default_params,
                            );
                        }

                        if (*trans).state == (*tnfa).final_ {
                            match_eo = pos;
                            for i in 0..num_tags {
                                *match_tags.offset(i as isize) = *(*reach_next
                                    .offset(stateid as isize))
                                .tags
                                .offset(i as isize);
                            }
                            for j in 0..TRE_M_LAST {
                                match_costs[j] = 0;
                            }
                        }
                    }
                    trans = trans.offset(1);
                }
            }

            // Handle inserts
            for id in 0..(*tnfa).num_states {
                if (*reach.offset(id as isize)).pos != prev_pos {
                    continue;
                }
                let depth = (*reach.offset(id as isize)).depth as usize;

                let mut cost = (*reach.offset(id as isize)).costs[depth][TRE_M_COST];
                if (*reach.offset(id as isize)).params.cost_ins != TRE_PARAM_UNSET {
                    cost += (*reach.offset(id as isize)).params.cost_ins;
                }
                if cost > (*reach.offset(id as isize)).params.max_cost {
                    continue;
                }
                if (*reach.offset(id as isize)).costs[depth][TRE_M_NUM_INS] + 1
                    > (*reach.offset(id as isize)).params.max_ins
                {
                    continue;
                }
                if (*reach.offset(id as isize)).costs[depth][TRE_M_NUM_ERR] + 1
                    > (*reach.offset(id as isize)).params.max_err
                {
                    continue;
                }

                let mut cost0 = cost;
                if depth > 0 {
                    cost0 = (*reach.offset(id as isize)).costs[0][TRE_M_COST];
                    if (*reach.offset(id as isize)).params.cost_ins != TRE_PARAM_UNSET {
                        cost0 += (*reach.offset(id as isize)).params.cost_ins;
                    } else {
                        cost0 += default_params.cost_ins;
                    }
                }

                if (*reach_next.offset(id as isize)).pos == pos
                    && cost0 >= (*reach_next.offset(id as isize)).costs[0][TRE_M_COST]
                {
                    continue;
                }

                (*reach_next.offset(id as isize)).state = (*reach.offset(id as isize)).state;
                (*reach_next.offset(id as isize)).pos = pos;
                for i in 0..num_tags {
                    *(*reach_next.offset(id as isize)).tags.offset(i as isize) =
                        *(*reach.offset(id as isize)).tags.offset(i as isize);
                }
                (*reach_next.offset(id as isize)).params = (*reach.offset(id as isize)).params;
                (*reach_next.offset(id as isize)).depth = (*reach.offset(id as isize)).depth;

                let copy_len = TRE_M_LAST * (depth + 1);
                for j in 0..copy_len {
                    let d = j / TRE_M_LAST;
                    let c = j % TRE_M_LAST;
                    (*reach_next.offset(id as isize)).costs[d][c] =
                        (*reach.offset(id as isize)).costs[d][c];
                }
                (*reach_next.offset(id as isize)).costs[depth][TRE_M_COST] = cost;
                (*reach_next.offset(id as isize)).costs[depth][TRE_M_NUM_INS] += 1;
                (*reach_next.offset(id as isize)).costs[depth][TRE_M_NUM_ERR] += 1;
                if depth > 0 {
                    (*reach_next.offset(id as isize)).costs[0][TRE_M_COST] = cost0;
                    (*reach_next.offset(id as isize)).costs[0][TRE_M_NUM_INS] += 1;
                    (*reach_next.offset(id as isize)).costs[0][TRE_M_NUM_ERR] += 1;
                }
            }

            // Handle deletes (BFS with deque)
            {
                let mut rb_size: usize = 256;
                let mut static_ringbuffer: [*mut tre_tnfa_approx_reach_t; 256] =
                    [ptr::null_mut(); 256];
                let mut ringbuffer: *mut *mut tre_tnfa_approx_reach_t =
                    static_ringbuffer.as_mut_ptr();
                let mut deque_start: usize = 0;
                let mut deque_end: usize = 0;

                for id in 0..(*tnfa).num_states {
                    if (*reach_next.offset(id as isize)).pos != pos {
                        continue;
                    }
                    *ringbuffer.offset(deque_end as isize) = reach_next.offset(id as isize);
                    deque_end += 1;
                    if deque_end >= rb_size {
                        rb_size += 512;
                        let larger_buf = mem::xmalloc(
                            std::mem::size_of::<*mut tre_tnfa_approx_reach_t>() * rb_size,
                        )
                            as *mut *mut tre_tnfa_approx_reach_t;
                        if larger_buf.is_null() {
                            mem::xfree(buf);
                            return REG_ESPACE as c_int;
                        }
                        if ringbuffer == static_ringbuffer.as_mut_ptr() {
                            ptr::copy_nonoverlapping(static_ringbuffer.as_ptr(), larger_buf, 256);
                        }
                        ringbuffer = larger_buf;
                    }
                }

                while deque_end != deque_start {
                    let reach_p = *ringbuffer.offset(deque_start as isize);
                    let _id = (reach_p as isize - reach_next as isize)
                        / std::mem::size_of::<tre_tnfa_approx_reach_t>() as isize;
                    let depth = (*reach_p).depth as usize;

                    let mut cost = (*reach_p).costs[depth][TRE_M_COST];
                    if (*reach_p).params.cost_del != TRE_PARAM_UNSET {
                        cost += (*reach_p).params.cost_del;
                    }

                    if cost > (*reach_p).params.max_cost
                        || (*reach_p).costs[depth][TRE_M_NUM_DEL] + 1 > (*reach_p).params.max_del
                        || (*reach_p).costs[depth][TRE_M_NUM_ERR] + 1 > (*reach_p).params.max_err
                    {
                        deque_start += 1;
                        if deque_start >= rb_size {
                            deque_start = 0;
                        }
                        continue;
                    }

                    let mut cost0 = cost;
                    if depth > 0 {
                        cost0 = (*reach_p).costs[0][TRE_M_COST];
                        if (*reach_p).params.cost_del != TRE_PARAM_UNSET {
                            cost0 += (*reach_p).params.cost_del;
                        } else {
                            cost0 += default_params.cost_del;
                        }
                    }

                    let mut trans = (*reach_p).state;
                    while !(*trans).state.is_null() {
                        let dest_id = (*trans).state_id;

                        if (*trans).assertions != 0
                            && CHECK_ASSERTIONS(
                                (*trans).assertions,
                                pos,
                                prev_c,
                                next_c,
                                reg_notbol,
                                reg_noteol,
                                reg_newline,
                            )
                        {
                            trans = trans.offset(1);
                            continue;
                        }

                        for i in 0..num_tags {
                            *tmp_tags.offset(i as isize) = *(*reach_p).tags.offset(i as isize);
                        }
                        if !(*trans).tags.is_null() {
                            let mut ti: c_int = 0;
                            while *(*trans).tags.offset(ti as isize) >= 0 {
                                if *(*trans).tags.offset(ti as isize) < num_tags {
                                    *tmp_tags.offset(*(*trans).tags.offset(ti as isize) as isize) =
                                        pos;
                                }
                                ti += 1;
                            }
                        }

                        if (*reach_next.offset(dest_id as isize)).pos == pos
                            && (cost0 > (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_COST]
                                || (cost0
                                    == (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_COST]
                                    && (match_tags.is_null()
                                        || tre_tag_order(
                                            num_tags,
                                            (*tnfa).tag_directions,
                                            tmp_tags,
                                            (*reach_next.offset(dest_id as isize)).tags,
                                        ) == 0)))
                        {
                            trans = trans.offset(1);
                            continue;
                        }

                        (*reach_next.offset(dest_id as isize)).state = (*trans).state;
                        (*reach_next.offset(dest_id as isize)).pos = pos;
                        for i in 0..num_tags {
                            *(*reach_next.offset(dest_id as isize))
                                .tags
                                .offset(i as isize) = *tmp_tags.offset(i as isize);
                        }

                        (*reach_next.offset(dest_id as isize)).params = (*reach_p).params;
                        if !(*trans).params.is_null() {
                            tre_set_params(
                                &mut *reach_next.offset(dest_id as isize),
                                (*trans).params,
                                default_params,
                            );
                        }

                        (*reach_next.offset(dest_id as isize)).depth = (*reach_p).depth;
                        let copy_len = TRE_M_LAST * (depth + 1);
                        for j in 0..copy_len {
                            let d = j / TRE_M_LAST;
                            let c = j % TRE_M_LAST;
                            (*reach_next.offset(dest_id as isize)).costs[d][c] =
                                (*reach_p).costs[d][c];
                        }
                        (*reach_next.offset(dest_id as isize)).costs[depth][TRE_M_COST] = cost;
                        (*reach_next.offset(dest_id as isize)).costs[depth][TRE_M_NUM_DEL] += 1;
                        (*reach_next.offset(dest_id as isize)).costs[depth][TRE_M_NUM_ERR] += 1;
                        if depth > 0 {
                            (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_COST] = cost0;
                            (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_NUM_DEL] += 1;
                            (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_NUM_ERR] += 1;
                        }

                        if (*trans).state == (*tnfa).final_
                            && (match_eo < 0
                                || match_costs[TRE_M_COST] > cost0
                                || (match_costs[TRE_M_COST] == cost0
                                    && num_tags > 0
                                    && *tmp_tags.offset(0) <= *match_tags.offset(0)))
                        {
                            match_eo = pos;
                            for j in 0..TRE_M_LAST {
                                match_costs[j] = (*reach_next.offset(dest_id as isize)).costs[0][j];
                            }
                            for i in 0..num_tags {
                                *match_tags.offset(i as isize) = *tmp_tags.offset(i as isize);
                            }
                        }

                        *ringbuffer.offset(deque_end as isize) =
                            reach_next.offset(dest_id as isize);
                        deque_end += 1;
                        if deque_end >= rb_size {
                            deque_end = 0;
                        }

                        trans = trans.offset(1);
                    }
                    deque_start += 1;
                    if deque_start >= rb_size {
                        deque_start = 0;
                    }
                }

                if ringbuffer != static_ringbuffer.as_mut_ptr() {
                    mem::xfree(ringbuffer as *mut c_void);
                }
            }

            // Check for end of string
            if len < 0 {
                if next_c == 0 {
                    break;
                }
            } else {
                if pos >= len {
                    break;
                }
            }

            prev_pos = pos;
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

            // Swap reach and reach_next
            let tmp = reach;
            let reach = reach_next;
            let reach_next = tmp;

            // Handle exact matches and substitutions
            for id in 0..(*tnfa).num_states {
                if (*reach.offset(id as isize)).pos < prev_pos {
                    continue;
                }
                let mut trans = (*reach.offset(id as isize)).state;
                while !(*trans).state.is_null() {
                    let dest_id = (*trans).state_id;
                    let depth = (*reach.offset(id as isize)).depth as usize;

                    let mut cost = (*reach.offset(id as isize)).costs[depth][TRE_M_COST];
                    let mut cost0 = (*reach.offset(id as isize)).costs[0][TRE_M_COST];
                    let mut err: c_int = 0;

                    if (*trans).assertions != 0
                        && (CHECK_ASSERTIONS(
                            (*trans).assertions,
                            pos,
                            prev_c,
                            next_c,
                            reg_notbol,
                            reg_noteol,
                            reg_newline,
                        ) || CHECK_CHAR_CLASSES(trans, tnfa, eflags, prev_c))
                    {
                        trans = trans.offset(1);
                        continue;
                    }

                    if (*trans).code_min > prev_c || (*trans).code_max < prev_c {
                        err = 1;
                        cost = (*reach.offset(id as isize)).costs[depth][TRE_M_COST];
                        if (*reach.offset(id as isize)).params.cost_subst != TRE_PARAM_UNSET {
                            cost += (*reach.offset(id as isize)).params.cost_subst;
                        }
                        if cost > (*reach.offset(id as isize)).params.max_cost {
                            trans = trans.offset(1);
                            continue;
                        }
                        if (*reach.offset(id as isize)).costs[depth][TRE_M_NUM_SUBST] + 1
                            > (*reach.offset(id as isize)).params.max_subst
                        {
                            trans = trans.offset(1);
                            continue;
                        }
                        if (*reach.offset(id as isize)).costs[depth][TRE_M_NUM_ERR] + 1
                            > (*reach.offset(id as isize)).params.max_err
                        {
                            trans = trans.offset(1);
                            continue;
                        }
                        cost0 = cost;
                        if depth > 0 {
                            cost0 = (*reach.offset(id as isize)).costs[0][TRE_M_COST];
                            if (*reach.offset(id as isize)).params.cost_subst != TRE_PARAM_UNSET {
                                cost0 += (*reach.offset(id as isize)).params.cost_subst;
                            } else {
                                cost0 += default_params.cost_subst;
                            }
                        }
                    }

                    for i in 0..num_tags {
                        *tmp_tags.offset(i as isize) =
                            *(*reach.offset(id as isize)).tags.offset(i as isize);
                    }
                    if !(*trans).tags.is_null() {
                        let mut ti: c_int = 0;
                        while *(*trans).tags.offset(ti as isize) >= 0 {
                            if *(*trans).tags.offset(ti as isize) < num_tags {
                                *tmp_tags.offset(*(*trans).tags.offset(ti as isize) as isize) = pos;
                            }
                            ti += 1;
                        }
                    }

                    if (*reach_next.offset(dest_id as isize)).pos == pos
                        && (cost0 > (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_COST]
                            || (cost0
                                == (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_COST]
                                && tre_tag_order(
                                    num_tags,
                                    (*tnfa).tag_directions,
                                    tmp_tags,
                                    (*reach_next.offset(dest_id as isize)).tags,
                                ) == 0))
                    {
                        trans = trans.offset(1);
                        continue;
                    }

                    (*reach_next.offset(dest_id as isize)).state = (*trans).state;
                    (*reach_next.offset(dest_id as isize)).pos = pos;
                    for i in 0..num_tags {
                        *(*reach_next.offset(dest_id as isize))
                            .tags
                            .offset(i as isize) = *tmp_tags.offset(i as isize);
                    }
                    (*reach_next.offset(dest_id as isize)).depth =
                        (*reach.offset(id as isize)).depth;

                    (*reach_next.offset(dest_id as isize)).params =
                        (*reach.offset(id as isize)).params;
                    if !(*trans).params.is_null() {
                        tre_set_params(
                            &mut *reach_next.offset(dest_id as isize),
                            (*trans).params,
                            default_params,
                        );
                    }

                    let copy_len = TRE_M_LAST * (depth + 1);
                    for j in 0..copy_len {
                        let d = j / TRE_M_LAST;
                        let c = j % TRE_M_LAST;
                        (*reach_next.offset(dest_id as isize)).costs[d][c] =
                            (*reach.offset(id as isize)).costs[d][c];
                    }
                    (*reach_next.offset(dest_id as isize)).costs[depth][TRE_M_COST] = cost;
                    (*reach_next.offset(dest_id as isize)).costs[depth][TRE_M_NUM_SUBST] += err;
                    (*reach_next.offset(dest_id as isize)).costs[depth][TRE_M_NUM_ERR] += err;
                    if depth > 0 {
                        (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_COST] = cost0;
                        (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_NUM_SUBST] += err;
                        (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_NUM_ERR] += err;
                    }

                    if (*trans).state == (*tnfa).final_
                        && (match_eo < 0
                            || cost0 < match_costs[TRE_M_COST]
                            || (cost0 == match_costs[TRE_M_COST]
                                && num_tags > 0
                                && *tmp_tags.offset(0) <= *match_tags.offset(0)))
                    {
                        match_eo = pos;
                        for j in 0..TRE_M_LAST {
                            match_costs[j] = (*reach_next.offset(dest_id as isize)).costs[0][j];
                        }
                        for i in 0..num_tags {
                            *match_tags.offset(i as isize) = *tmp_tags.offset(i as isize);
                        }
                    }

                    trans = trans.offset(1);
                }
            }
        }

        mem::xfree(buf);

        (*amatch).cost = match_costs[TRE_M_COST];
        (*amatch).num_ins = match_costs[TRE_M_NUM_INS];
        (*amatch).num_del = match_costs[TRE_M_NUM_DEL];
        (*amatch).num_subst = match_costs[TRE_M_NUM_SUBST];
        *match_end_ofs = match_eo;

        if match_eo >= 0 {
            REG_OK as c_int
        } else {
            REG_NOMATCH as c_int
        }
    }
}

unsafe fn align_ptr_approx(p: *mut u8) -> *mut u8 {
    unsafe {
        let align = std::mem::size_of::<usize>();
        let addr = p as usize;
        let rem = addr % align;
        if rem == 0 { p } else { p.add(align - rem) }
    }
}
