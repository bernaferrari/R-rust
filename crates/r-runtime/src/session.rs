use core::cell::Cell;
use core::marker::PhantomData;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, Ordering};

use alloc::vec::Vec;

use crate::gc::{Gc, Trace};
use crate::sexp::{Header, Sexp};

static SESSION_EXISTS: AtomicBool = AtomicBool::new(false);

/// Single-threaded runtime session. !Send + !Sync.
pub struct Session {
    pub(crate) roots: Vec<NonNull<Header>>,
    pub(crate) remembered_set: Vec<Sexp>,
    pub(crate) young_gen: Generation,
    pub(crate) old_gen: Generation,
    pub(crate) scratch_arena: ScratchArena,
    _not_send: PhantomData<*mut ()>,
}

/// GC generation.
pub struct Generation {
    pub(crate) objects: Vec<NonNull<Header>>,
    pub(crate) threshold: usize,
    pub(crate) used: usize,
}

/// Scratch memory arena for temporary allocations.
pub struct ScratchArena {
    blocks: Vec<&'static mut [u8]>,
    cursor: usize,
}

thread_local! {
    static CURRENT_SESSION: Cell<Option<NonNull<Session>>> = Cell::new(None);
}

impl Session {
    /// Create a new runtime session.
    /// There may only be one session per process.
    pub fn new() -> Result<Self, ()> {
        if SESSION_EXISTS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(());
        }

        Ok(Self {
            roots: Vec::with_capacity(1024),
            remembered_set: Vec::with_capacity(2048),
            young_gen: Generation {
                objects: Vec::with_capacity(4096),
                threshold: 8 * 1024 * 1024,
                used: 0,
            },
            old_gen: Generation {
                objects: Vec::with_capacity(65536),
                threshold: 64 * 1024 * 1024,
                used: 0,
            },
            scratch_arena: ScratchArena {
                blocks: Vec::new(),
                cursor: 0,
            },
            _not_send: PhantomData,
        })
    }

    #[inline(always)]
    pub fn current() -> &'static mut Session {
        CURRENT_SESSION.with(|cell| {
            let ptr = cell.get().expect("No active session");
            unsafe { ptr.as_mut() }
        })
    }

    #[inline]
    pub(crate) fn add_root(&mut self, header: NonNull<Header>) {
        self.roots.push(header);
    }

    #[inline]
    pub(crate) fn remove_root(&mut self, header: NonNull<Header>) {
        self.roots.retain(|&h| h != header);
    }

    #[inline(always)]
    pub(crate) fn remembered_set(&mut self) -> &mut Vec<Sexp> {
        &mut self.remembered_set
    }

    /// Run minor garbage collection.
    pub fn minor_gc(&mut self) {
        // Mark phase from roots and remembered set
        for root in &self.roots {
            unsafe {
                (*root.as_ptr()).gc_bits |= 0b00000100;
                self.mark_recursive(*root);
            }
        }

        for obj in &self.remembered_set {
            unsafe {
                (*obj.header() as *const Header as *mut Header).gc_bits |= 0b00000100;
                self.mark_recursive(NonNull::new_unchecked(
                    obj.header() as *const Header as *mut Header
                ));
            }
        }

        // Sweep young generation
        self.young_gen.objects.retain(|obj| {
            let marked = unsafe { (**obj).gc_bits & 0b00000100 != 0 };
            if !marked {
                unsafe {
                    (**obj).finalize();
                }
            }
            marked
        });

        // Clear marks
        for obj in &self.young_gen.objects {
            unsafe {
                (**obj).gc_bits &= !0b00000100;
            }
        }
    }

    #[inline(never)]
    fn mark_recursive(&mut self, header: NonNull<Header>) {
        unsafe {
            (*header.as_ptr()).trace();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        SESSION_EXISTS.store(false, Ordering::SeqCst);
    }
}

unsafe impl Trace for Header {
    #[inline]
    unsafe fn trace(&mut self) {
        if let Some(attrs) = self.attributes {
            let header = attrs.as_ptr().cast::<Header>();
            if (*header).gc_bits & 0b00000100 == 0 {
                (*header).gc_bits |= 0b00000100;
                (*header).trace();
            }
        }
    }
}

impl Header {
    #[inline]
    unsafe fn finalize(&mut self) {
        // Run finalizer if present
    }
}

unsafe impl !Send for Session {}
unsafe impl !Sync for Session {}
