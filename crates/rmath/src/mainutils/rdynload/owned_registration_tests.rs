//! Copied declarations and physical leases survive registry changes.
use super::*;
use crate::sexp::RSession;

unsafe extern "C" fn address_only() {}

unsafe fn c_table(info: *mut DllInfo, count: c_int, types: &[c_int]) {
    let name = CString::new("owned_registered").unwrap();
    let table = [
        R_CMethodDef {
            name: name.as_ptr(),
            fun: Some(address_only),
            num_args: count,
            types: types.as_ptr(),
        },
        R_CMethodDef {
            name: ptr::null(),
            fun: None,
            num_args: 0,
            types: ptr::null(),
        },
    ];
    unsafe {
        R_registerRoutines(info, table.as_ptr(), ptr::null(), ptr::null(), ptr::null());
    }
    // Both input name and table leave scope. No resolved descriptor borrows them.
}

#[test]
fn declaration_and_library_survive_reregistration_and_unload() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        add_dll(
            c"owned-path".as_ptr(),
            c"owned-package".as_ptr(),
            ptr::null_mut(),
        );
        let info = R_getDllInfo(c"owned-path".as_ptr());
        c_table(info, 2, &[13, 14]);
        let original = resolve_foreign_symbol(
            c"owned_registered".as_ptr(),
            c"owned-package".as_ptr(),
            R_C_SYM,
            ptr::null_mut(),
        )
        .unwrap();
        let physical = Rc::downgrade(&original._library);
        c_table(info, 1, &[16]);
        let replacement = resolve_foreign_symbol(
            c"owned_registered".as_ptr(),
            c"owned-package".as_ptr(),
            R_C_SYM,
            ptr::null_mut(),
        )
        .unwrap();
        assert_eq!(original.argument_types(), Some([13, 14].as_slice()));
        assert!(original.validate(R_C_SYM, 2).is_ok());
        assert!(original.validate(R_C_SYM, 1).is_err());
        assert!(original.validate(R_CALL_SYM, 2).is_err());
        assert_eq!(replacement.argument_types(), Some([16].as_slice()));
        assert!(replacement.validate(R_C_SYM, 1).is_ok());
        assert!(delete_dll(c"owned-path".as_ptr()));
        assert!(
            resolve_foreign_symbol(
                c"owned_registered".as_ptr(),
                c"owned-package".as_ptr(),
                R_C_SYM,
                ptr::null_mut()
            )
            .is_none()
        );
        assert!(physical.upgrade().is_some());
        drop(replacement);
        assert!(physical.upgrade().is_some());
        assert_eq!(original.argument_types(), Some([13, 14].as_slice()));
        drop(original);
        assert!(
            physical.upgrade().is_none(),
            "final lease must destroy the physical DLL exactly once"
        );
    });
}

#[test]
fn variadic_declaration_has_no_fabricated_type_array() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let info = R_getEmbeddingDllInfo();
        c_table(info, -7, &[13]);
        let routine = resolve_foreign_symbol(
            c"owned_registered".as_ptr(),
            c"(embedding)".as_ptr(),
            R_C_SYM,
            ptr::null_mut(),
        )
        .unwrap();
        assert_eq!(routine.argument_types(), None);
        for count in [0, 1, 64, usize::MAX] {
            assert!(routine.validate(R_C_SYM, count).is_ok());
        }
        assert!(routine.validate(R_EXTERNAL_SYM, 0).is_err());
    });
}

#[test]
fn registered_lookup_is_session_local_and_interface_specific() {
    let left = RSession::new_for_gc_tests();
    let right = RSession::new_for_gc_tests();
    let routine = left.with_active(|| unsafe {
        let info = R_getEmbeddingDllInfo();
        c_table(info, 0, &[]);
        assert!(
            resolve_foreign_symbol(
                c"owned_registered".as_ptr(),
                c"(embedding)".as_ptr(),
                R_EXTERNAL_SYM,
                ptr::null_mut()
            )
            .is_none()
        );
        resolve_foreign_symbol(
            c"owned_registered".as_ptr(),
            c"(embedding)".as_ptr(),
            R_C_SYM,
            ptr::null_mut(),
        )
        .unwrap()
    });
    right.with_active(|| unsafe {
        assert!(
            resolve_foreign_symbol(
                c"owned_registered".as_ptr(),
                c"".as_ptr(),
                R_C_SYM,
                ptr::null_mut()
            )
            .is_none()
        );
    });
    drop(left);
    assert!(routine.validate(R_C_SYM, 0).is_ok());
    drop(routine);
}
