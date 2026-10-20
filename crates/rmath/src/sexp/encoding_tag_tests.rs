use super::*;
use crate::sexp::{
    heap::PageMetadata,
    memory::{ArenaBudget, RArena},
};

fn assert_tag(kind: &str, gnu: i32, gp: u16) {
    let page = std::mem::size_of::<SexprecCore>() + PageMetadata::requested_array_bytes(1).unwrap();
    let mut arena = RArena::with_budget(ArenaBudget::new(page + 8, 1));
    let pointer = arena.alloc_charsxp(&[0xc3, 0xa9]);
    let node = arena.node_token(pointer).unwrap();
    let heap = node.heap_identity();
    let charge = arena.total_bytes_allocated();
    // SAFETY: this fixture owns the exact live character; marking only changes
    // its header metadata, while the sealed payload and its owner stay live.
    unsafe {
        mark_charsxp_encoding(pointer, "unknown");
        assert_eq!(getCharCE(pointer), 0);
        mark_charsxp_encoding(pointer, kind);
        assert_eq!(getCharCE(pointer), gnu);
    }
    assert_eq!(heap.node_snapshot(&node).unwrap().sxpinfo.gp(), gp);
    let payload = heap.payload_lease(&node).unwrap();
    assert_eq!(payload.byte_elt(0), Some(0xc3));
    assert_eq!(payload.byte_elt(1), Some(0xa9));
    assert_eq!(arena.total_bytes_allocated(), charge);
}

#[test]
fn original_gnu_encoding_tag_utf8_uses_one_without_changing_gp_bits() {
    assert_tag("UTF-8", 1, 1 << 3);
}

#[test]
fn original_gnu_encoding_tag_latin1_uses_two_without_changing_gp_bits() {
    assert_tag("latin1", 2, 1 << 2);
}

#[test]
fn original_gnu_encoding_tag_bytes_uses_three_without_changing_gp_bits() {
    assert_tag("bytes", 3, 1 << 1);
}

#[test]
fn original_gnu_encoding_tag_byte_width_uses_hex_columns() {
    let bytes = [0xe9u8, 0];
    // SAFETY: this bounded immutable span belongs to the fixture; Rstrwid
    // only reads its declared one-byte length and cannot retain the pointer.
    let columns =
        unsafe { crate::mainutils::printutils::Rstrwid(bytes.as_ptr().cast(), 1, 3, b'"' as i32) };
    assert_eq!(columns, 4);
}
