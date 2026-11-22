#![warn(unused_unsafe)]
//! Message catalog loading for GNU gettext.
//!
//! Safe .mo (Machine Object) parser producing an owned `Catalog` with
//! byte-range entry references and a bounded linear lookup. The portable
//! `_nl_load_domain` adapter boxes the catalog behind the legacy raw
//! `loaded_l10nfile` cache slot.

use std::ffi::CStr;
use std::ops::Range;
use std::os::raw::c_ulong;
use std::ptr;

use super::plural_exp::PluralForms;
use super::types::{MO_MAGIC, MO_MAGIC_SWAPPED, cstr, loaded_l10nfile, ptr_mut};

pub(super) struct Catalog {
    bytes: Vec<u8>,
    entries: Vec<(Range<usize>, Range<usize>)>,
    plural: PluralForms,
}

impl Catalog {
    pub(super) fn lookup(&self, id: &CStr, plural_n: Option<c_ulong>) -> Option<&CStr> {
        let (_, trans) = self.entries.iter().find(|(original, _)| {
            let original = &self.bytes[original.clone()];
            original.split(|&byte| byte == 0).next() == Some(id.to_bytes())
        })?;
        let text = &self.bytes[trans.clone()];
        let index = plural_n.map_or(0, |n| self.plural.index(n));
        let form = text.split_inclusive(|&byte| byte == 0).nth(index)?;
        CStr::from_bytes_with_nul(form)
            .ok()
            .filter(|form| !form.is_empty())
    }
}

pub(super) fn parse_catalog(bytes: Vec<u8>) -> Option<Catalog> {
    if bytes.len() < 28 {
        return None;
    }
    let swapped = match u32::from_ne_bytes(bytes[..4].try_into().ok()?) {
        MO_MAGIC => false,
        MO_MAGIC_SWAPPED => true,
        _ => return None,
    };
    let word = |offset: usize| -> Option<u32> {
        let word = u32::from_ne_bytes(
            bytes
                .get(offset..offset.checked_add(4)?)?
                .try_into()
                .ok()?,
        );
        Some(if swapped { word.swap_bytes() } else { word })
    };
    let table = |offset: usize, count: usize| -> Option<Range<usize>> {
        let end = offset.checked_add(count.checked_mul(8)?)?;
        bytes.get(offset..end)?;
        Some(offset..end)
    };
    let entry = |offset: usize| -> Option<Range<usize>> {
        let len = word(offset)? as usize;
        let start = word(offset + 4)? as usize;
        let end = start.checked_add(len)?;
        let tail = end.checked_add(1)?;
        let text = bytes.get(start..tail)?;
        (text.last() == Some(&0)).then_some(start..tail)
    };
    let count = word(8)? as usize;
    let originals = table(word(12)? as usize, count)?;
    let translations = table(word(16)? as usize, count)?;
    let entries: Vec<_> = originals
        .step_by(8)
        .zip(translations.step_by(8))
        .map(|(o, t)| Some((entry(o)?, entry(t)?)))
        .collect::<Option<_>>()?;
    let metadata = entries
        .first()
        .filter(|(orig, _)| orig.len() == 1)
        .map_or(&[][..], |(_, trans)| {
            &bytes[trans.start..trans.end - 1]
        });
    let plural = PluralForms::from_metadata(metadata);
    Some(Catalog {
        bytes,
        entries,
        plural,
    })
}

/// Load the catalog into an owned value. The legacy cache holds a raw owning pointer.
///
/// # Safety
/// `domain_file` must be null or point to a live writable node with no loaded data,
/// and its filename must be null or a valid NUL-terminated path for the call.
pub unsafe fn _nl_load_domain(domain_file: *mut loaded_l10nfile) {
    // SAFETY: the caller supplies a live, exclusively writable node.
    let Some(file) = (unsafe { ptr_mut(domain_file) }) else {
        return;
    };
    // SAFETY: the node's filename satisfies the caller's C-string contract.
    let catalog = unsafe { cstr(file.filename) }
        .and_then(|path| path.to_str().ok())
        .and_then(|path| std::fs::read(path).ok())
        .and_then(parse_catalog);
    file.data = catalog.map_or(ptr::null(), |catalog| {
        Box::into_raw(Box::new(catalog)).cast_const().cast()
    });
    file.decided = 1;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::raw::c_char;

    /// Build a minimal two-entry catalog: metadata entry plus one message
    /// with three plural translations, optionally padded to unaligned tables.
    fn catalog_bytes(big_endian: bool) -> Vec<u8> {
        let originals: [&[u8]; 2] = [b"", b"bird\0birds"];
        let translations: [&[u8]; 2] = [
            b"Plural-Forms: nplurals=3; plural=n==1 ? 0 : n==2 ? 1 : 2;\n",
            b"one\0few\0many",
        ];
        let word = |value: u32| {
            if big_endian {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            }
        };
        let orig_table = 29usize; // one pad byte exercises unaligned table reads.
        let trans_table = orig_table + originals.len() * 8;
        let mut cursor = trans_table + translations.len() * 8;
        let mut orig_descs = Vec::new();
        let mut trans_descs = Vec::new();
        let mut strings = Vec::new();
        for (original, translation) in originals.iter().zip(translations.iter()) {
            orig_descs.extend_from_slice(&word(original.len() as u32));
            orig_descs.extend_from_slice(&word(cursor as u32));
            strings.extend_from_slice(original);
            strings.push(0);
            cursor += original.len() + 1;
            trans_descs.extend_from_slice(&word(translation.len() as u32));
            trans_descs.extend_from_slice(&word(cursor as u32));
            strings.extend_from_slice(translation);
            strings.push(0);
            cursor += translation.len() + 1;
        }
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&word(MO_MAGIC));
        bytes.extend_from_slice(&word(0)); // revision
        bytes.extend_from_slice(&word(originals.len() as u32));
        bytes.extend_from_slice(&word(orig_table as u32));
        bytes.extend_from_slice(&word(trans_table as u32));
        bytes.extend_from_slice(&word(0)); // hash table size
        bytes.extend_from_slice(&word(0)); // hash table offset
        bytes.push(0); // unaligned pad before original table
        bytes.extend_from_slice(&orig_descs);
        bytes.extend_from_slice(&trans_descs);
        bytes.extend_from_slice(&strings);
        bytes
    }

    #[test]
    fn test_catalog_parses_little_endian_and_forms() {
        let catalog = parse_catalog(catalog_bytes(false)).unwrap();
        assert_eq!(catalog.lookup(c"bird", Some(1)), Some(c"one"));
        assert_eq!(catalog.lookup(c"bird", Some(2)), Some(c"few"));
        assert_eq!(catalog.lookup(c"bird", Some(4)), Some(c"many"));
        assert!(catalog.lookup(c"missing", None).is_none());
    }

    #[test]
    fn test_catalog_parses_big_endian_and_forms() {
        let catalog = parse_catalog(catalog_bytes(true)).unwrap();
        assert_eq!(catalog.lookup(c"bird", Some(1)), Some(c"one"));
        assert_eq!(catalog.lookup(c"bird", Some(2)), Some(c"few"));
        assert_eq!(catalog.lookup(c"bird", Some(4)), Some(c"many"));
        assert!(catalog.lookup(c"missing", None).is_none());
    }

    #[test]
    fn test_catalog_rejects_truncated_input() {
        let bytes = catalog_bytes(false);
        assert!(parse_catalog(bytes[..27].to_vec()).is_none());
        assert!(parse_catalog(bytes[..bytes.len() - 1].to_vec()).is_none());
    }

    #[test]
    fn test_catalog_rejects_bad_table_offset() {
        let mut bytes = catalog_bytes(false);
        bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_catalog(bytes).is_none());
    }

    #[test]
    fn test_catalog_rejects_bad_entry_offset() {
        let mut bytes = catalog_bytes(false);
        // Translation descriptor 1 offset field: table at 45, entry 0 offset at +4.
        bytes[49..53].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_catalog(bytes).is_none());
    }

    #[test]
    fn test_load_domain_null_file() {
        let mut l10nfile = loaded_l10nfile {
            filename: ptr::null(),
            decided: 0,
            data: ptr::null(),
            next: ptr::null_mut(),
            successor: [ptr::null_mut()],
        };
        // SAFETY: stack-owned node is live and exclusively writable for the call.
        unsafe { _nl_load_domain(&mut l10nfile) };
        assert_eq!(l10nfile.decided, 1);
        assert!(l10nfile.data.is_null());
    }

    #[test]
    fn test_load_domain_missing_file() {
        let mut l10nfile = loaded_l10nfile {
            filename: b"/nonexistent/path/messages.mo\0".as_ptr() as *const c_char,
            decided: 0,
            data: ptr::null(),
            next: ptr::null_mut(),
            successor: [ptr::null_mut()],
        };
        // SAFETY: stack-owned node and literal NUL-terminated path for the call.
        unsafe { _nl_load_domain(&mut l10nfile) };
        assert_eq!(l10nfile.decided, 1);
        assert!(l10nfile.data.is_null());
    }
}
