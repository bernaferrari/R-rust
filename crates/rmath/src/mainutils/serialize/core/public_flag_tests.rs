#![allow(unsafe_code)]
use super::*;
use crate::sexp::accessors::{S4_OBJECT_MASK, SET_S4_OBJECT, UNSET_S4_OBJECT};
use crate::sexp::session::RSession;

#[test]
fn rooted_gp_snapshots_keep_s4_for_every_serialized_vector_kind() {
    let session = RSession::new_for_gc_tests();
    for kind in [
        SEXPTYPE::LGLSXP,
        SEXPTYPE::INTSXP,
        SEXPTYPE::REALSXP,
        SEXPTYPE::CPLXSXP,
        SEXPTYPE::STRSXP,
        SEXPTYPE::VECSXP,
        SEXPTYPE::EXPRSXP,
        SEXPTYPE::RAWSXP,
    ] {
        let object = session.sexp(unsafe { Rf_allocVector3(kind, 2) }).unwrap();
        for marked in [false, true] {
            unsafe {
                if marked {
                    SET_S4_OBJECT(object.as_raw());
                } else {
                    UNSET_S4_OBJECT(object.as_raw());
                }
            }
            session.gc();
            let gp = unsafe { serialized_gp_bits(object.as_raw()) };
            assert_eq!(gp & i32::from(S4_OBJECT_MASK), if marked { 16 } else { 0 });
            assert_eq!(
                unsafe { LEVELS(object.as_raw()) },
                0,
                "do not change the general two-bit accessor"
            );
            let packed = unsafe { PackFlags(kind.as_c_int(), gp, 0, 0, 0) };
            assert_eq!((packed >> 12) & 16, if marked { 16 } else { 0 });
        }
    }
}

#[test]
fn full_gp_packing_retains_only_original_type_specific_neighbor_bits() {
    unsafe {
        let s4 = i32::from(S4_OBJECT_MASK);
        for kind in [
            SEXPTYPE::LGLSXP,
            SEXPTYPE::INTSXP,
            SEXPTYPE::REALSXP,
            SEXPTYPE::CPLXSXP,
            SEXPTYPE::STRSXP,
            SEXPTYPE::VECSXP,
            SEXPTYPE::EXPRSXP,
            SEXPTYPE::RAWSXP,
        ] {
            assert_eq!(
                PackFlags(kind.as_c_int(), s4 | GROWABLE_MASK, 0, 0, 0) >> 12,
                s4
            );
        }
        assert_eq!(
            PackFlags(SEXPTYPE::CLOSXP.as_c_int(), s4 | GROWABLE_MASK, 0, 0, 0) >> 12,
            s4 | GROWABLE_MASK
        );
        // GNU CHARSXP strips cache/hash bookkeeping and retains UTF-8 (bit3).
        assert_eq!(
            PackFlags(
                SEXPTYPE::CHARSXP.as_c_int(),
                (1 << 3) | (1 << 5) | 1,
                0,
                0,
                0
            ) >> 12,
            1 << 3
        );
    }
}
