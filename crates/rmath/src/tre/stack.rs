/*
  tre/stack.rs - Simple stack implementation

  Ported from tre-stack.c
*/

use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_void};
use std::ptr;

use crate::mainutils::grep::{RegexWorkspace, WorkspaceBuffer};
use crate::sexp::memory::WorkspaceReservation;

use super::mem;

pub const REG_OK: c_int = 0;
pub const REG_ESPACE: c_int = 12;

#[repr(C)]
#[derive(Clone, Copy)]
union tre_stack_item {
    voidptr_value: *mut c_void,
    int_value: c_int,
}

#[repr(C)]
pub struct tre_stack_rec {
    size: c_int,
    max_size: c_int,
    increment: c_int,
    ptr: c_int,
    stack: *mut tre_stack_item,
    owned_items: Option<WorkspaceBuffer<MaybeUninit<tre_stack_item>>>,
    descriptor_admission: Option<WorkspaceReservation>,
}

pub type tre_stack_t = *mut tre_stack_rec;

pub fn tre_stack_new(size: c_int, max_size: c_int, increment: c_int) -> tre_stack_t {
    // SAFETY: `s` is initialized before the raw pointer escapes.
    unsafe {
        if size <= 0 || max_size < size || increment <= 0 {
            return ptr::null_mut();
        }
        let Some(bytes) = (size as usize).checked_mul(std::mem::size_of::<tre_stack_item>()) else {
            return ptr::null_mut();
        };
        let s: tre_stack_t = mem::xmalloc(std::mem::size_of::<tre_stack_rec>()) as tre_stack_t;
        if s.is_null() {
            return ptr::null_mut();
        }
        let items = mem::xmalloc(bytes) as *mut tre_stack_item;
        if items.is_null() {
            mem::xfree(s as *mut c_void);
            return ptr::null_mut();
        }
        s.write(tre_stack_rec {
            size,
            max_size,
            increment,
            ptr: 0,
            stack: items,
            owned_items: None,
            descriptor_admission: None,
        });
        s
    }
}

/// Create a compiler stack whose descriptor and full item-buffer capacities
/// are admitted to the originating regex workspace before allocation.
pub(crate) fn tre_stack_new_owned(size: c_int, workspace: &RegexWorkspace) -> tre_stack_t {
    // SAFETY: `s` is initialized before the raw pointer escapes.
    unsafe {
        if size <= 0 {
            return ptr::null_mut();
        }
        let Some(descriptor_bytes) = mem::xallocation_size(std::mem::size_of::<tre_stack_rec>())
        else {
            return ptr::null_mut();
        };
        let Ok(descriptor_admission) = workspace.try_reservation(descriptor_bytes) else {
            return ptr::null_mut();
        };
        let Ok(mut items) = workspace.try_buffer::<MaybeUninit<tre_stack_item>>(size as usize)
        else {
            return ptr::null_mut();
        };
        if items
            .try_resize(size as usize, MaybeUninit::uninit())
            .is_err()
        {
            return ptr::null_mut();
        }
        let s = mem::xmalloc(std::mem::size_of::<tre_stack_rec>()) as tre_stack_t;
        if s.is_null() {
            return ptr::null_mut();
        }
        s.write(tre_stack_rec {
            size,
            max_size: c_int::MAX,
            increment: 0,
            ptr: 0,
            stack: ptr::null_mut(),
            owned_items: Some(items),
            descriptor_admission: Some(descriptor_admission),
        });
        s
    }
}

pub fn tre_stack_destroy(s: tre_stack_t) {
    // SAFETY: `s` is a live tre_stack_new allocation or null.
    unsafe {
        if s.is_null() {
            return;
        }
        if !(*s).stack.is_null() {
            mem::xfree((*s).stack as *mut c_void);
        }
        let descriptor_admission = (*s).descriptor_admission.take();
        ptr::drop_in_place(s);
        mem::xfree(s as *mut c_void);
        drop(descriptor_admission);
    }
}

pub fn tre_stack_num_objects(s: tre_stack_t) -> c_int {
    // SAFETY: `s` is a live stack or null.
    unsafe {
        if s.is_null() {
            return 0;
        }
        (*s).ptr
    }
}

fn tre_stack_push(s: tre_stack_t, value: tre_stack_item) -> c_int {
    // SAFETY: `s` is a live stack with capacity bookkeeping in `rec`.
    unsafe {
        let stack = &mut *s;
        if stack.ptr == stack.max_size {
            return REG_ESPACE;
        }
        if let Some(items) = &mut stack.owned_items {
            if !items.is_live() {
                return REG_ESPACE;
            }
            let index = stack.ptr as usize;
            if index == items.len() {
                let preferred = stack.ptr.saturating_mul(2).min(stack.max_size) as usize;
                if items.try_resize(preferred, MaybeUninit::uninit()).is_err()
                    && items.try_resize(index + 1, MaybeUninit::uninit()).is_err()
                {
                    return REG_ESPACE;
                }
            }
            items[index].write(value);
        } else {
            if stack.ptr == stack.size {
                let new_size = stack
                    .size
                    .saturating_add(stack.increment)
                    .min(stack.max_size);
                let Some(bytes) =
                    (new_size as usize).checked_mul(std::mem::size_of::<tre_stack_item>())
                else {
                    return REG_ESPACE;
                };
                let buffer = mem::xrealloc(stack.stack.cast(), bytes).cast::<tre_stack_item>();
                if buffer.is_null() {
                    return REG_ESPACE;
                }
                stack.stack = buffer;
                stack.size = new_size;
            }
            stack.stack.add(stack.ptr as usize).write(value);
        }
        stack.ptr += 1;
        REG_OK
    }
}

pub fn tre_stack_push_voidptr(s: tre_stack_t, value: *mut c_void) -> c_int {
    tre_stack_push(
        s,
        tre_stack_item {
            voidptr_value: value,
        },
    )
}

pub fn tre_stack_push_int(s: tre_stack_t, value: c_int) -> c_int {
    tre_stack_push(s, tre_stack_item { int_value: value })
}

fn tre_stack_pop(s: tre_stack_t) -> tre_stack_item {
    // SAFETY: `s` is a live stack with a nonzero object count.
    unsafe {
        let stack = &mut *s;
        stack.ptr -= 1;
        let index = stack.ptr as usize;
        if let Some(items) = &stack.owned_items {
            *items[index].assume_init_ref()
        } else {
            *stack.stack.add(index)
        }
    }
}

pub fn tre_stack_pop_voidptr(s: tre_stack_t) -> *mut c_void {
    // SAFETY: the stack contract stores a voidptr item for this pop.
    unsafe { tre_stack_pop(s).voidptr_value }
}

pub fn tre_stack_pop_int(s: tre_stack_t) -> c_int {
    // SAFETY: the stack contract stores an int item for this pop.
    unsafe { tre_stack_pop(s).int_value }
}

#[cfg(test)]
mod checked_growth_tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget};

    #[test]
    fn owner_stack_growth_is_admitted_recoverable_and_released() {
        let mut session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let ledger = owner
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let workspace = crate::mainutils::grep::regex_workspace();
        unsafe {
            let stack = tre_stack_new_owned(1, &workspace);
            assert!(!stack.is_null());
            assert!(ledger.workspace_bytes_for_test() > 0);
            assert_eq!(tre_stack_push_int(stack, 17), REG_OK);
            let admitted = ledger.workspace_bytes_for_test();

            owner
                .with_arena(|arena| {
                    let used = arena.total_bytes_allocated();
                    arena.set_budget(ArenaBudget::new(used + admitted, 0));
                })
                .unwrap();
            assert_eq!(tre_stack_push_int(stack, 23), REG_ESPACE);
            assert_eq!(tre_stack_num_objects(stack), 1);
            assert_eq!(ledger.workspace_bytes_for_test(), admitted);

            owner
                .with_arena(|arena| {
                    let used = arena.total_bytes_allocated();
                    arena.set_budget(ArenaBudget::new(used + admitted + 4096, 0));
                })
                .unwrap();
            assert_eq!(tre_stack_push_int(stack, 23), REG_OK);
            assert_eq!(tre_stack_num_objects(stack), 2);
            assert!(ledger.workspace_bytes_for_test() > admitted);
            let slack = tre_stack_new_owned(4, &workspace);
            assert!(!slack.is_null());
            assert_eq!(tre_stack_push_int(slack, 31), REG_OK);
            session.close();
            assert_eq!(
                tre_stack_push_int(slack, 37),
                REG_ESPACE,
                "revoked owner blocks writes even when existing capacity suffices"
            );
            assert_eq!(tre_stack_push_int(stack, 29), REG_ESPACE);
            tre_stack_destroy(slack);
            tre_stack_destroy(stack);
            assert_eq!(ledger.workspace_bytes_for_test(), 0);
        }
    }

    #[test]
    fn owner_stack_grows_past_the_old_compile_limit() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let ledger = owner
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let workspace = crate::mainutils::grep::regex_workspace();
        unsafe {
            let stack = tre_stack_new_owned(512, &workspace);
            assert!(!stack.is_null());
            for value in 0..10_241 {
                assert_eq!(tre_stack_push_int(stack, value), REG_OK);
            }
            assert_eq!(tre_stack_num_objects(stack), 10_241);
            for value in (0..10_241).rev() {
                assert_eq!(tre_stack_pop_int(stack), value);
            }
            tre_stack_destroy(stack);
        }
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    }

    #[test]
    fn parser_reports_scratch_stack_growth_allocation_denial() {
        let workspace = crate::mainutils::grep::RegexWorkspace::unbounded();
        let mut arena = mem::tre_mem_struct::new(workspace).unwrap();
        let pattern = [u32::from('a')];
        let (status, report) = mem::fault::observe(Some(2), || unsafe {
            // Descriptor + first item succeed; first growth allocation fails.
            let stack = tre_stack_new(1, 10240, 128);
            assert!(!stack.is_null());
            let mut context: crate::tre::ast::tre_parse_ctx_t = std::mem::zeroed();
            context.stack = stack;
            context.mem = &mut arena;
            context.re = pattern.as_ptr();
            context.len = pattern.len();
            context.cflags = crate::tre::ast::REG_EXTENDED;
            context.max_backref = -1;
            context.cur_max = 1;
            let status = crate::tre::parse::tre_parse(&mut context);
            tre_stack_destroy(stack);
            status
        });
        assert_eq!(
            status, REG_ESPACE,
            "actual parser discarded stack growth denial"
        );
        assert_eq!(report.remaining, 0);
    }
    #[test]
    fn parser_reports_each_work_stack_allocation_failure_and_recovers() {
        fn run(fail_at: Option<usize>) -> (c_int, mem::fault::Report) {
            let workspace = crate::mainutils::grep::RegexWorkspace::unbounded();
            let mut arena = mem::tre_mem_struct::new(workspace).unwrap();
            let pattern: Vec<u32> = "(a|b)*c".chars().map(u32::from).collect();
            mem::fault::observe(fail_at, || unsafe {
                let stack = tre_stack_new(1, 10240, 1);
                if stack.is_null() {
                    return REG_ESPACE;
                }
                let mut context: crate::tre::ast::tre_parse_ctx_t = std::mem::zeroed();
                context.stack = stack;
                context.mem = &mut arena;
                context.re = pattern.as_ptr();
                context.len = pattern.len();
                context.cflags = crate::tre::ast::REG_EXTENDED;
                context.max_backref = -1;
                context.cur_max = 1;
                let status = crate::tre::parse::tre_parse(&mut context);
                tre_stack_destroy(stack);
                status
            })
        }
        let (status, baseline) = run(None);
        assert_eq!(status, REG_OK);
        assert_eq!(baseline.remaining, 0);
        assert!(baseline.attempts > 2);
        for fail_at in 0..baseline.attempts {
            let (status, report) = run(Some(fail_at));
            assert_eq!(status, REG_ESPACE, "allocation {fail_at}");
            assert_eq!(report.remaining, 0, "allocation {fail_at}");
        }
        assert_eq!(run(None).0, REG_OK);
    }
}
