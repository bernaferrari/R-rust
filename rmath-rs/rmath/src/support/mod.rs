//! R support libraries — standalone utility modules.
//!
//! XDR encoding, timezone handling, string formatting, internationalization,
//! regex (TRE), and GUI stubs (GraphApp). Ported from external C code that R bundles.

#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

pub mod cport;
pub mod graphapp;
pub mod intl;
pub mod tre;
pub mod trio;
pub mod tzone;
pub mod tzone_strftime;
pub mod xdr;
