#![forbid(unsafe_code)]
//! Canonical implementation lives in the pure world
//! (value::support::number_parse); re-exported here until the old world's
//! deletion.

pub(crate) use crate::value::support::number_parse::{parse_number, ParsedNumber};
