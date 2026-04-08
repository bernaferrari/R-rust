#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Bitmap conversion for GraphApp.

use super::types::*;

pub unsafe fn bitmaptoimage(bm: bitmap) -> image {
    if bm.is_null() {
        std::ptr::null_mut()
    } else {
        std::ptr::null_mut()
    }
}
