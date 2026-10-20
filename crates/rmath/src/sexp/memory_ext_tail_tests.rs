use crate::sexp::memory::ArenaBudget;

#[test]
fn transient_scratch_denies_chunk_tail_before_native_allocation() {
    let mut owner = crate::sexp::instance::RInstance::new_for_gc_tests();
    let before = owner.arena.total_bytes_allocated();
    owner.arena.set_budget(ArenaBudget::new(before + 1, 0));
    let attempts = crate::sexp::memory::buffer_allocation_attempts();
    // SAFETY: the fixture owns this stable instance; no field borrow or raw
    // payload view overlaps the strictly local transient allocation call.
    let pointer = unsafe { crate::sexp::memory_ext::R_alloc_in(&mut owner, 1, 1) };
    assert!(pointer.is_null());
    assert_eq!(crate::sexp::memory::buffer_allocation_attempts(), attempts);
    assert_eq!(owner.arena.workspace_bytes(), 0);
    assert_eq!(owner.arena.total_bytes_allocated(), before);
}

#[test]
fn transient_scratch_tail_charge_releases_at_its_original_watermark() {
    let mut owner = crate::sexp::instance::RInstance::new_for_gc_tests();
    let before = owner.arena.total_bytes_allocated();
    let chunk = std::mem::size_of::<u64>();
    owner.arena.set_budget(ArenaBudget::new(before + chunk, 0));
    // SAFETY: the fixture keeps its instance live and holds no overlapping
    // storage loans; the returned raw projection is never dereferenced here.
    let pointer = unsafe { crate::sexp::memory_ext::R_alloc_in(&mut owner, 1, 1) };
    assert!(!pointer.is_null());
    assert!(owner.arena.workspace_bytes() >= chunk);
    // SAFETY: zero is the original empty watermark for this exact instance.
    unsafe { crate::sexp::memory_ext::vmaxset_in(&mut owner, std::ptr::null_mut()) };
    assert_eq!(owner.arena.workspace_bytes(), 0);
    assert_eq!(owner.arena.total_bytes_allocated(), before);
}
