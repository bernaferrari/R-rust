#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/dcf.c
//!
//! Provides `do_readDCF`, the `.Internal` implementation behind R's
//! `read.dcf()` function for parsing Debian Control Format files
//! (used for DESCRIPTION files in R packages).

use std::ffi::CString;
use std::fs;
use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::*;

/// Interpret `bytes` as UTF-8, replacing every invalid byte with `"<xx>"`
/// (lowercase hex).
///
/// Mirrors upstream dcf.c's `mkCharUTF8sub()` -> `reEnc3(s, "UTF-8",
/// "UTF-8", 1)`: DCF files are required to be UTF-8, so invalid byte
/// sequences are repaired by escaping each byte as `<xx>`, exactly as
/// `iconv(from = "UTF-8", to = "UTF-8", sub = "byte")` does (which the R
/// code paths in read.dcf()/write.dcf() also use).  The CE_UTF8 marking of
/// the upstream helper has no counterpart here: this port does not track
/// CHARSXP encodings (`Rf_mkChar` never sets encoding bits).
fn utf8_sub_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for &b in chunk.invalid() {
            out.push_str(&format!("<{b:02x}>"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// DCF line classification helpers
// ---------------------------------------------------------------------------

/// Check if a line is blank (empty or only whitespace).
fn is_blank_line(line: &str) -> bool {
    line.trim().is_empty()
}

/// Check if a line is a continuation line (starts with whitespace).
fn is_continuation_line(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

/// Check if a line matches the field pattern `FieldName: Value`.
/// Returns Some((field_name, offset)) where offset is the start of the value
/// (after the colon and any whitespace), or None if it doesn't match.
fn parse_field_line(line: &str) -> Option<(&str, usize)> {
    let colon_pos = line.find(':')?;
    let field_name = &line[..colon_pos];
    // Field name should not be empty and should not contain whitespace
    if field_name.is_empty() || field_name.contains(char::is_whitespace) {
        return None;
    }
    // Value starts after the colon; skip optional whitespace
    let rest = &line[colon_pos + 1..];
    let value_start = colon_pos + 1 + rest.len() - rest.trim_start().len();
    Some((field_name, value_start))
}

/// Check if a line is an "empty blank line" (only whitespace and a single dot).
fn is_eblank_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed == "."
}

/// Remove trailing whitespace from a string.
fn trim_trailing_whitespace(s: &mut String) {
    let len = s.trim_end().len();
    s.truncate(len);
}

/// Check if a field name is in the fold_excludes list.
unsafe fn field_is_foldable(field_name: &str, fold_excludes: SEXP) -> bool {
    unsafe {
        if fold_excludes.is_null() || fold_excludes == R_NilValue() {
            return true;
        }
        let n = LENGTH(fold_excludes);
        let cfield = CString::new(field_name).unwrap_or_default();
        let cfield_ptr = cfield.as_ptr();
        for i in 0..n as isize {
            let elt = STRING_ELT(fold_excludes, i as R_xlen_t);
            if elt.is_null() {
                continue;
            }
            let elt_ptr = CHAR(elt);
            if !elt_ptr.is_null() {
                let elt_str = std::ffi::CStr::from_ptr(elt_ptr).to_str().unwrap_or("");
                if elt_str == field_name {
                    return false;
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Internal matrix allocation helpers (no dims set -- we set them at the end)
// ---------------------------------------------------------------------------

/// Allocate a STRSXP vector of length `nrow * ncol`, all elements set to NA.
unsafe fn alloc_matrix_na(nrow: c_int, ncol: c_int) -> SEXP {
    unsafe {
        let len = (nrow as i64) * (ncol as i64);
        let mat = Rf_allocVector(SEXPTYPE::STRSXP, len as c_int);
        if mat.is_null() {
            return ptr::null_mut();
        }
        // Fill with NA_STRING (use R_NilValue as NA_STRING stub)
        for i in 0..len {
            SET_STRING_ELT(mat, i as R_xlen_t, R_NilValue());
        }
        mat
    }
}

/// Transfer all STRING_ELT values from source to destination.
unsafe fn transfer_vector(dest: SEXP, src: SEXP) {
    unsafe {
        let n = LENGTH(src);
        for i in 0..n as isize {
            SET_STRING_ELT(dest, i as R_xlen_t, STRING_ELT(src, i as R_xlen_t));
        }
    }
}

/// Copy a STRSXP matrix by rows (transpose: src is row-major by record,
/// dest is column-major by field).
unsafe fn copy_matrix_byrow(dest: SEXP, src: SEXP, src_nrow: c_int, src_ncol: c_int) {
    unsafe {
        for r in 0..src_nrow {
            for c in 0..src_ncol {
                let src_idx = (c * src_nrow + r) as R_xlen_t;
                let dest_idx = (r * src_ncol + c) as R_xlen_t;
                let val = STRING_ELT(src, src_idx);
                SET_STRING_ELT(dest, dest_idx, val);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// do_readDCF -- main entry point
// ---------------------------------------------------------------------------

/// Read a DCF (Debian Control Format) file.
///
/// R-level signature: `read.dcf(file, fields = NULL, all = FALSE)`
///
/// The implementation:
/// 1. Reads the file content from a file path (first argument as string)
/// 2. Parses DCF records separated by blank lines
/// 3. Handles continuation lines (indented with whitespace)
/// 4. Handles field folding (collapsing whitespace-only continuation lines)
/// 5. Returns a character matrix with field names as column names
pub unsafe fn do_readDCF(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // --- Extract arguments ---
        // args is a pairlist: (file, fields, fold_excludes)
        let file_sexp = CAR(args);
        let what = CDR(args);
        let fields_sexp = CAR(what);
        let fold_arg = CDR(what);
        let fold_excludes_sexp = CAR(fold_arg);

        // Get the file path from the first argument
        let filename = if !file_sexp.is_null() && TYPEOF(file_sexp) == SEXPTYPE::STRSXP {
            let charsxp = STRING_ELT(file_sexp, 0);
            if !charsxp.is_null() {
                let cptr = CHAR(charsxp);
                if !cptr.is_null() {
                    std::ffi::CStr::from_ptr(cptr)
                        .to_string_lossy()
                        .into_owned()
                } else {
                    return R_NilValue();
                }
            } else {
                return R_NilValue();
            }
        } else {
            return R_NilValue();
        };

        // Read the file content as raw bytes and repair invalid UTF-8 byte
        // sequences ("<xx>", as upstream dcf.c's mkCharUTF8sub() does), so
        // that field names and values become valid UTF-8 strings.
        let content_bytes = match fs::read(&filename) {
            Ok(b) => b,
            Err(_) => return R_NilValue(),
        };
        let content = utf8_sub_bytes(&content_bytes);

        // fields argument: a STRSXP vector of field names to extract
        // If empty (length 0), we use dynamic mode (all fields)
        let mut nwhat: c_int = 0;
        let mut dynwhat = false;
        let mut field_names: Vec<String> = Vec::new();

        if !fields_sexp.is_null() && TYPEOF(fields_sexp) == SEXPTYPE::STRSXP {
            nwhat = LENGTH(fields_sexp);
            if nwhat == 0 {
                dynwhat = true;
            } else {
                for i in 0..nwhat as isize {
                    let elt = STRING_ELT(fields_sexp, i as R_xlen_t);
                    if !elt.is_null() {
                        let cptr = CHAR(elt);
                        if !cptr.is_null() {
                            field_names.push(
                                std::ffi::CStr::from_ptr(cptr)
                                    .to_string_lossy()
                                    .into_owned(),
                            );
                        } else {
                            field_names.push(String::new());
                        }
                    } else {
                        field_names.push(String::new());
                    }
                }
            }
        } else {
            dynwhat = true;
        }

        // fold_excludes argument: STRSXP of field names whose values should NOT be folded
        // (left as-is, preserving exact whitespace)
        // We just pass this through for field_is_foldable checks

        // --- Parse DCF records ---
        // We accumulate field values in a 2D structure:
        //   records[row][field_index] = value
        let mut records: Vec<Vec<Option<String>>> = Vec::new();
        let mut current_record: Vec<Option<String>> = if nwhat > 0 {
            vec![None; nwhat as usize]
        } else {
            Vec::new()
        };

        let mut lastm: i32 = -1; // index of field currently being recorded
        let mut blank_skip = true;
        let mut field_skip = false;
        let mut field_fold = true;
        let mut n_eblanklines: usize = 0;

        let lines: Vec<&str> = content.lines().collect();

        for raw_line in &lines {
            let line = *raw_line;

            if is_blank_line(line) {
                // Blank line: end current record if we were recording one
                if !blank_skip {
                    // Push the current record and start a new one
                    records.push(std::mem::take(&mut current_record));
                    if nwhat > 0 {
                        current_record = vec![None; nwhat as usize];
                    } else {
                        current_record = Vec::new();
                    }
                    blank_skip = true;
                    lastm = -1;
                    field_skip = false;
                    field_fold = true;
                    n_eblanklines = 0;
                }
            } else if line.starts_with('#') {
                // Comment lines are ignored
            } else if is_continuation_line(line) {
                blank_skip = false;

                // Continuation line: wrong if at beginning of a record
                if lastm == -1 && !field_skip {
                    // Error in real R; here we skip
                    continue;
                }

                if lastm >= 0 {
                    let idx = lastm as usize;
                    if idx < current_record.len() {
                        // Check if this is an "empty blank line" (whitespace + dot)
                        if is_eblank_line(line) {
                            if field_fold {
                                n_eblanklines += 1;
                                continue;
                            }
                            // Non-foldable: store as-is
                            if let Some(ref mut val) = current_record[idx] {
                                val.push('\n');
                                val.push_str(line.trim());
                            }
                        } else {
                            if field_fold {
                                // Remove trailing whitespace, skip leading whitespace
                                let trimmed = line.trim();
                                if !trimmed.is_empty()
                                    && let Some(ref mut val) = current_record[idx]
                                {
                                    // Add newlines for any accumulated empty blank lines
                                    if !val.is_empty() {
                                        val.push('\n');
                                    }
                                    for _ in 0..n_eblanklines {
                                        val.push('\n');
                                    }
                                    n_eblanklines = 0;
                                    val.push_str(trimmed);
                                }
                            } else {
                                // Non-foldable: preserve as-is
                                if let Some(ref mut val) = current_record[idx] {
                                    val.push('\n');
                                    val.push_str(line);
                                }
                            }
                        }
                    }
                }
            } else if let Some((field_name, value_offset)) = parse_field_line(line) {
                // Regular field line
                blank_skip = false;

                // Try to match against known fields
                let mut matched_field_idx: Option<usize> = None;
                for m in 0..field_names.len() {
                    let whatlen = field_names[m].len();
                    if line.len() > whatlen
                        && line.chars().nth(whatlen) == Some(':')
                        && line[..whatlen] == field_names[m]
                    {
                        matched_field_idx = Some(m);
                        break;
                    } else {
                        lastm = -1;
                        field_skip = true;
                    }
                }

                if let Some(m) = matched_field_idx {
                    // Known field
                    lastm = m as i32;
                    field_skip = false;
                    field_fold = field_is_foldable(field_name, fold_excludes_sexp);
                    n_eblanklines = 0;

                    // Ensure current_record has enough slots
                    while current_record.len() <= m {
                        current_record.push(None);
                    }
                    let value = line[value_offset..].trim_end().to_string();
                    current_record[m] = Some(value);
                } else if dynwhat {
                    // Dynamic mode: add a new field
                    field_skip = false;
                    field_names.push(field_name.to_string());
                    nwhat = field_names.len() as c_int;
                    lastm = (nwhat - 1) as i32;
                    field_fold = field_is_foldable(field_name, fold_excludes_sexp);
                    n_eblanklines = 0;

                    // Pad existing records with None for the new field
                    for rec in &mut records {
                        while rec.len() < field_names.len() {
                            rec.push(None);
                        }
                    }
                    // Pad current_record too
                    while current_record.len() < field_names.len() {
                        current_record.push(None);
                    }
                    let value = line[value_offset..].trim_end().to_string();
                    current_record[lastm as usize] = Some(value);
                }
            }
            // Lines that don't match any pattern are silently skipped
            // (in real R this would be an error)
        }

        // Push the last record if non-empty
        if !blank_skip && !current_record.is_empty() {
            records.push(current_record);
        }

        // If no records found, return a 0-row matrix
        if records.is_empty() || field_names.is_empty() {
            let nfields = field_names.len();
            if nfields == 0 {
                // Return 0x0 matrix
                let mat = Rf_allocVector(SEXPTYPE::STRSXP, 0);
                return mat;
            }
            // Return 0 x nfields matrix
            let mat = Rf_allocVector(SEXPTYPE::STRSXP, 0);
            return mat;
        }

        let nfields = field_names.len();
        let nrows = records.len();

        // Build the result STRSXP: column-major (nrows * nfields)
        let total = (nrows * nfields) as c_int;
        let retval = Rf_allocVector(SEXPTYPE::STRSXP, total);
        if retval.is_null() {
            return R_NilValue();
        }
        let _retval_guard = protect(retval);

        // Fill with NA first
        for i in 0..total as isize {
            SET_STRING_ELT(retval, i as R_xlen_t, R_NilValue());
        }

        // Fill in values (column-major: column c, row r -> index r + c * nrows)
        for (r, record) in records.iter().enumerate() {
            for (c, field_val) in record.iter().enumerate() {
                if c < nfields {
                    let idx = (r + c * nrows) as R_xlen_t;
                    if let Some(val) = field_val {
                        let cs = CString::new(val.as_str()).unwrap_or_default();
                        let charsxp = Rf_mkChar(cs.as_ptr());
                        SET_STRING_ELT(retval, idx, charsxp);
                    }
                    // else: leave as NA
                }
            }
        }

        // Build dim attribute: integer vector c(nrows, nfields)
        let dims = Rf_allocVector(SEXPTYPE::INTSXP, 2);
        let _dims_guard = protect(dims);
        let dim_data = INTEGER(dims);
        if !dim_data.is_null() {
            *dim_data = nrows as c_int;
            *dim_data.add(1) = nfields as c_int;
        }

        // Build dimnames: list(NULL, field_names_vector)
        let dimnames = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _dimnames_guard = protect(dimnames);
        // First element: NULL (no row names)
        SET_VECTOR_ELT(dimnames, 0, R_NilValue());
        // Second element: STRSXP of field names
        let col_names = Rf_allocVector(SEXPTYPE::STRSXP, nfields as c_int);
        let _col_names_guard = protect(col_names);
        for (i, name) in field_names.iter().enumerate() {
            let cs = CString::new(name.as_str()).unwrap_or_default();
            let charsxp = Rf_mkChar(cs.as_ptr());
            SET_STRING_ELT(col_names, i as R_xlen_t, charsxp);
        }
        SET_VECTOR_ELT(dimnames, 1, col_names);

        // Set attributes on the result
        crate::eval::attrib_core::setAttrib(retval, crate::eval::attrib_core::R_DimSymbol(), dims);
        crate::eval::attrib_core::setAttrib(
            retval,
            crate::eval::attrib_core::R_DimNamesSymbol(),
            dimnames,
        );

        retval
    }
}

/// Helper: compute the flat index from lastm and nwhat.
/// In dynamic mode, lastm is directly the field index.
/// In fixed mode, lastm is the field index within the `what` array.
#[inline]
fn idx_from_lastm(lastm: i32, _nwhat: c_int, field_names_len: usize) -> usize {
    lastm as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn char_elt(s: SEXP, i: usize) -> String {
        unsafe {
            let p = CHAR(STRING_ELT(s, i as R_xlen_t));
            std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }

    #[test]
    fn utf8_sub_bytes_passes_valid_utf8_through() {
        assert_eq!(utf8_sub_bytes(b"plain ascii"), "plain ascii");
        assert_eq!(utf8_sub_bytes("caf\u{e9}".as_bytes()), "caf\u{e9}");
    }

    #[test]
    fn utf8_sub_bytes_escapes_invalid_bytes() {
        // lone 0xE9 (would start a 3-byte sequence): one byte, one escape
        assert_eq!(utf8_sub_bytes(b"caf\xe9 ok"), "caf<e9> ok");
        // truncated multi-byte sequence at end of input
        assert_eq!(utf8_sub_bytes(b"a\xf0\x9f"), "a<f0><9f>");
        // continuation bytes without a lead byte
        assert_eq!(utf8_sub_bytes(b"\x80\x81"), "<80><81>");
    }

    #[test]
    fn read_dcf_repairs_invalid_utf8() {
        let _session = crate::sexp::session::RSession::new();
        let mut path = std::env::temp_dir();
        path.push(format!(
            "rport-dcf-{}-{}.dcf",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        // "Package: caf\xc3\xa9" (valid UTF-8) and "Field: caf\xe9 ok"
        // (0xE9 is an invalid UTF-8 start byte here).
        let content = b"Package: caf\xc3\xa9\nField: caf\xe9 ok\n\n";
        std::fs::write(&path, content).unwrap();

        unsafe {
            let file = Rf_mkString(
                CString::new(path.to_str().unwrap())
                    .unwrap_or_default()
                    .as_ptr(),
            );
            let fields = Rf_allocVector(SEXPTYPE::STRSXP, 0); // dynamic mode
            let args = {
                let mut tail = R_NilValue();
                for item in [R_NilValue(), fields, file] {
                    tail = Rf_cons(item, tail);
                }
                tail
            };
            let res = do_readDCF(std::ptr::null_mut(), std::ptr::null_mut(), args, std::ptr::null_mut());
            assert_eq!(TYPEOF(res), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(res), 2);
            // column-major: 1 row, 2 fields
            assert_eq!(char_elt(res, 0), "caf\u{e9}");
            assert_eq!(char_elt(res, 1), "caf<e9> ok");

            // dim attribute is 1 x 2
            let dim = crate::eval::attrib_core::getAttrib(
                res,
                crate::eval::attrib_core::R_DimSymbol(),
            );
            assert_eq!(*INTEGER(dim), 1);
            assert_eq!(*INTEGER(dim).add(1), 2);
        }
        let _ = std::fs::remove_file(&path);
    }
}
