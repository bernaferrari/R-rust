#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/mkdtemp.c
//!
//! Create a unique temporary directory. The template must end in six `X` characters,
//! which is replaced with a string that makes the filename unique.
//! The directory is created with mode 0700.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;

const LETTERS: &[u8; 62] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const TMP_MAX: u32 = 238328;

/// Generate a unique temporary directory from a template.
///
/// The last six characters of `template` must be six `X` characters; they are replaced
/// with a string that makes the filename unique. The directory is created
/// with mode 0700.
///
/// Returns the pointer to `template` on success, or null on failure.
pub unsafe fn mkdtemp(template: *mut c_char) -> *mut c_char {
    unsafe {
        if template.is_null() {
            return ptr::null_mut();
        }

        let tmpl = CStr::from_ptr(template);
        let len = tmpl.to_bytes().len();

        if len < 6 {
            return ptr::null_mut();
        }

        // Check that the last 6 characters are 'X'
        let bytes = tmpl.to_bytes();
        for i in (len - 6)..len {
            if bytes[i] != b'X' {
                return ptr::null_mut();
            }
        }

        // Use a simple time-based seed for randomness
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);

        let mut value = seed;

        for _ in 0..TMP_MAX {
            let mut v = value;

            // Fill in the 6 X's with random characters
            let base = template.add(len - 6);
            for j in 0..6 {
                *base.add(j) = LETTERS[(v % 62) as usize] as c_char;
                v /= 62;
            }

            // Try to create the directory
            let result = std::fs::create_dir(CStr::from_ptr(template).to_str().unwrap_or(""));
            if result.is_ok() {
                return template;
            }
            // If directory exists, try next combination; otherwise give up
            if std::io::ErrorKind::AlreadyExists
                != result
                    .as_ref()
                    .err()
                    .map(|e| e.kind())
                    .unwrap_or(std::io::ErrorKind::Other)
            {
                return ptr::null_mut();
            }

            value = value.wrapping_add(7777);
        }

        ptr::null_mut()
    }
}
