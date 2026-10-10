#![forbid(unsafe_code)]
//! Measurement of the original portable methods requests, without changing admission.
//! This native memory diagnostic is not the browser behavioral acceptance gate.

use crate::{
    mainutils::paths::RuntimePathPolicy,
    sexp::{
        RSession, SEXPTYPE,
        ffi::{NodeBody, SexprecCore},
        memory::ArenaBudget,
    },
};
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};

fn snapshot(session: &mut RSession, group: &str, phase: &str) {
    let measurements = session.with_arena(|arena| {
        let heap = arena.heap_identity();
        let mut kinds = BTreeMap::<String, usize>::new();
        let mut characters = HashMap::<(u16, Vec<u8>), (usize, usize)>::new();
        let mut payloads = HashSet::new();
        let mut payload_bytes = 0_usize;
        let mut character_payload_bytes = 0_usize;
        for pointer in arena.active_nodes() {
            let node = arena.node_token(pointer).expect("live arena identity");
            let header = heap.node_snapshot(&node).expect("copied live header");
            let kind = header.sxpinfo.type_of();
            *kinds.entry(format!("{kind:?}")).or_default() += 1;
            if let Some(payload) = heap.payload_lease(&node) {
                if payloads.insert(payload.id()) { payload_bytes += payload.logical_bytes(); }
                if kind == SEXPTYPE::CHARSXP {
                    let NodeBody::Vector(vector) = header.data else { panic!("character shape") };
                    let length = usize::try_from(vector.length).expect("character length");
                    let bytes = (0..length).map(|index| payload.byte_elt(index).expect("checked character byte")).collect();
                    // Actual BYTES/LATIN1/UTF8/ASCII encoding bits; other gp flags are not identity.
                    let encoding = header.sxpinfo.gp() & ((1 << 1) | (1 << 2) | (1 << 3) | (1 << 6));
                    let entry = characters.entry((encoding, bytes)).or_default();
                    entry.0 += 1;
                    entry.1 += payload.logical_bytes();
                    character_payload_bytes += payload.logical_bytes();
                }
            }
        }
        let unique_char_payload_bytes = characters.keys().map(|(_, bytes)| bytes.len() + 1).sum::<usize>();
        let duplicate_characters = characters.values().map(|(count, _)| count - 1).sum::<usize>();
        let mut repeated = characters.iter().filter(|(_, (count, _))| *count > 1).map(|((encoding, bytes), (count, charge))| {
            (*count, *encoding, bytes.len(), String::from_utf8_lossy(&bytes[..bytes.len().min(60)]).into_owned(), *charge)
        }).collect::<Vec<_>>();
        repeated.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.3.cmp(&b.3)));
        repeated.truncate(12);
        let active = arena.node_count();
        let free = arena.free_count();
        let headers = (active + free) * std::mem::size_of::<SexprecCore>();
        let charged = arena.total_bytes_allocated();
        json!({"group":group,"phase":phase,"active":active,"free":free,"charged_bytes":charged,
            "header_size":std::mem::size_of::<SexprecCore>(),"first_admission_refusal":arena.admission_refusal(),"calculated_slot_header_bytes":headers,
            "unique_live_payload_bytes":payload_bytes,"unattributed_charged_bytes":charged.checked_sub(headers + payload_bytes),
            "kinds":kinds,"character_unique_byte_encoding_identities":characters.len(),
            "character_duplicate_nodes":duplicate_characters,"character_payload_bytes":character_payload_bytes,
            "character_unique_minimum_payload_bytes":unique_char_payload_bytes,
            "duplicate_character_header_charge":duplicate_characters * std::mem::size_of::<SexprecCore>(),
            "most_repeated_characters":repeated})
    }).expect("original live session");
    eprintln!("[methods-allocation] {measurements}");
}

fn evaluate(session: &mut RSession, group: &str, phase: &str, code: &str) {
    let (result, output, visible) = session.eval_code_with_output_capture(code);
    let error = result.err().map(|error| error.message);
    eprintln!(
        "[methods-allocation] {}",
        json!({"group":group,"phase":phase,"error":error,"stdout":output.stdout,"stderr":output.stderr,"visible":visible})
    );
}

#[test]
fn portable_methods_original_requests_allocation_diagnostic() {
    let group = std::env::var("RPORT_METHODS_ALLOCATION_GROUP").unwrap_or_else(|_| "ANY".into());
    assert!(group == "ANY" || group == "inherited");
    let mut session = RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"));
    session.set_arena_budget(ArenaBudget::new(64 * 1024 * 1024, 500_000));
    // Diagnostic evaluation remains the native session's existing timer policy:
    // debug builds must not turn slower execution into a different memory frontier.
    // The immutable optimized browser receipts retain the actual 15s request policy.
    snapshot(&mut session, &group, "startup");
    if group == "ANY" {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../r-embed/tests/fixtures/gnu-compiled-captured.rds"
        ));
        let raw = bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        evaluate(
            &mut session,
            &group,
            "compiled_prefix",
            &format!("g <- unserialize(as.raw(c({raw}))); cat(g(),g(4))"),
        );
        session
            .with_arena(|arena| arena.clear_admission_refusal())
            .unwrap();
        snapshot(&mut session, &group, "before_original_request");
        evaluate(
            &mut session,
            &group,
            "original_request",
            "setGeneric('wild',function(x,y) standardGeneric('wild')); setMethod('wild',c('ANY','ANY'),function(x,y) 'fallback'); setMethod('wild','numeric',function(x,y) 'number'); cat(wild(2,NULL),wild(NULL,TRUE))",
        );
    } else {
        let mut bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../r-embed/tests/fixtures/gnu-bytecode-calls/identity-promise.rds"
        ))
        .to_vec();
        let original = [12_i32, 20, 0, 1]
            .into_iter()
            .flat_map(i32::to_be_bytes)
            .collect::<Vec<_>>();
        let offset = bytes
            .windows(original.len())
            .position(|window| window == original)
            .expect("original compiled fixture stream");
        bytes[offset + 4..offset + 8].copy_from_slice(&16_i32.to_be_bytes());
        let raw = bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        evaluate(
            &mut session,
            &group,
            "compiled_prefix",
            &format!("f<-unserialize(as.raw(c({raw})));identical(f(41L),quote(x))"),
        );
        evaluate(
            &mut session,
            &group,
            "serialized_prefix",
            "g<-unserialize(serialize(f,NULL));identical(g(99L),quote(x))",
        );
        evaluate(
            &mut session,
            &group,
            "disassembled_prefix",
            "invisible(capture.output(d<-compiler::disassemble(f)));identical(d[[2]][[2]],as.name('GETFUN.OP'))&&identical(d[[3]][[3]][[2]][[2]],as.name('LDCONST.OP'))",
        );
        session
            .with_arena(|arena| arena.clear_admission_refusal())
            .unwrap();
        snapshot(&mut session, &group, "before_original_request");
        evaluate(
            &mut session,
            &group,
            "original_request",
            "local({setClass('SelectA');setClass('SelectB',contains='SelectA');setGeneric('selectprobe',function(x)standardGeneric('selectprobe'));setMethod('selectprobe','SelectA',function(x)42L);m<-selectMethod('selectprobe','SelectB');identical(m(new('SelectB')),42L)&&identical(as.character(m@defined),'SelectA')})",
        );
    }
    snapshot(&mut session, &group, "after_original_request");
    evaluate(&mut session, &group, "full_gc", "gc()");
    snapshot(&mut session, &group, "after_full_gc");
    evaluate(&mut session, &group, "recovery", "1+1");
}
