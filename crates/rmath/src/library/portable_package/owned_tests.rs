use super::*;
use crate::sexp::{RSession, memory::ArenaBudget};
use std::io::Write;

#[test]
fn portable_adapter_moves_error_messages_and_propagates_other_signals() {
    use crate::sexp::context::{RError, RSignal};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let anchor = session
            .owner_token()
            .unwrap()
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        for signal in [false, true] {
            let mut message = String::with_capacity(128);
            message.push_str("portable failure á中😀");
            let pointer = message.as_ptr();
            let error = execute::<()>(&anchor, |_| {
                if signal {
                    std::panic::panic_any(RSignal::Error { message })
                } else {
                    std::panic::panic_any(RError { message })
                }
            })
            .unwrap_err();
            assert_eq!(error.as_ptr(), pointer);
            assert_eq!(error, "portable failure á中😀");
        }
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            execute::<()>(&anchor, |_| std::panic::panic_any(RSignal::Break))
        }))
        .unwrap_err();
        assert!(matches!(
            *panic.downcast::<RSignal>().unwrap(),
            RSignal::Break
        ));
    });
}

fn compressed_record(decoded: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(decoded).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut record = Vec::with_capacity(4 + compressed.len());
    record.extend_from_slice(&(decoded.len() as u32).to_be_bytes());
    record.extend_from_slice(&compressed);
    record
}

fn allocated_bytes(anchor: &Sexp<'static>) -> usize {
    execute(anchor, |access| {
        access.with_arena(|arena| arena.total_bytes_allocated())
    })
    .unwrap()
}

fn reserve_available(anchor: &Sexp<'static>, bytes: usize) -> bool {
    execute(anchor, |access| {
        Ok(access
            .with_arena(|arena| arena.try_reserve_transient(bytes))?
            .is_some())
    })
    .unwrap()
}

#[test]
fn decoder_workspace_threshold_cleanup_and_full_gc_are_accounted() {
    let mut session = RSession::new_for_gc_tests();
    let anchor = session
        .owner_token()
        .unwrap()
        .node_factory()
        .allocate(|arena| Some(arena.alloc_vector(crate::sexp::SEXPTYPE::RAWSXP, 1)))
        .unwrap()
        .into_owned()
        .unwrap();
    let decoded = b"portable decoder accounting";
    let record = compressed_record(decoded);
    let input_workspace = decompressed_input_workspace(decoded.len() + 1, record.len()).unwrap();
    let workspace = input_workspace + PORTABLE_ZLIB_DECODER_SCRATCH;
    let baseline = allocated_bytes(&anchor);

    // A one-byte-short budget rejects before decoder construction. The same
    // record succeeds when the complete workspace is admitted.
    session.set_arena_budget(ArenaBudget::new(baseline + workspace - 1, 0));
    let denied = execute(&anchor, |access| {
        decode_record(access, &record, decoded.len()).map(|_| ())
    });
    assert!(
        denied
            .unwrap_err()
            .contains("portable package decompression workspace")
    );

    session.set_arena_budget(ArenaBudget::new(baseline + workspace, 0));
    let mut corrupt = record.clone();
    corrupt[4] = 0;
    let failed = execute(&anchor, |access| {
        decode_record(access, &corrupt, decoded.len()).map(|_| ())
    });
    assert!(failed.is_err());
    assert!(reserve_available(&anchor, workspace));

    // Keep the ticket alive across a full collection. It must continue to
    // exclude additional scratch admission, and release exactly its charge
    // when the decoded bytes leave the producer scope.
    let (actual, reservation) = execute(&anchor, |access| {
        let result = decode_record(access, &record, decoded.len())?;
        let _ = crate::sexp::gengc::full_gc();
        Ok(result)
    })
    .unwrap();
    assert_eq!(actual, decoded);
    assert_eq!(reservation.bytes(), input_workspace);

    let after_gc = allocated_bytes(&anchor);
    session.set_arena_budget(ArenaBudget::new(after_gc + workspace, 0));
    assert!(reserve_available(&anchor, PORTABLE_ZLIB_DECODER_SCRATCH));
    assert!(!reserve_available(
        &anchor,
        PORTABLE_ZLIB_DECODER_SCRATCH + 1
    ));
    drop(reservation);
    assert!(reserve_available(&anchor, workspace));
}
