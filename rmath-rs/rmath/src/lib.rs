//! rmath: Rust port of R's nmath statistical library
//!
//! This crate provides a drop-in replacement for R's libRmath.a,
//! implementing statistical math functions with C-compatible FFI.

// C-to-Rust translation conventions (R's nmath library uses C naming)
#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
// Enable all clippy lints including pedantic
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
// --- Pervasive C-port patterns (organized by category) ---
//
// This crate is a direct C-to-Rust translation of R's nmath statistical
// library. The following allows suppress lints that fire hundreds/thousands
// of times due to inherent C coding patterns. Safety/correctness lints
// (unwrap_used, transmutes, eq_op, etc.) are intentionally NOT suppressed.

// Numeric casts: pervasive in C math code (f64/i32/i64/usize interop)
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_ptr_alignment)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::bool_to_int_with_if)]
// Pointer operations: inherent to C FFI and memory allocation patterns
#![allow(clippy::ptr_as_ptr)]
#![allow(clippy::ptr_cast_constness)]
#![allow(clippy::ref_as_ptr)]
#![allow(clippy::borrow_as_ptr)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::ptr_eq)]
// Math code style: single-char variables, precise constants, float comparisons
#![allow(clippy::many_single_char_names)]
#![allow(clippy::similar_names)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::excessive_precision)]
#![allow(clippy::float_cmp)]
#![allow(clippy::approx_constant)]
// Documentation: C code doesn't follow Rust doc conventions
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_safety_doc)]
// C control flow idioms: direct translations from C if/else/match patterns
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_else)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::if_not_else)]
#![allow(clippy::comparison_chain)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::single_match_else)]
#![allow(clippy::match_wildcard_for_single_variants)]
#![allow(clippy::match_bool)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::manual_clamp)]
#![allow(clippy::manual_midpoint)]
#![allow(clippy::manual_swap)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::manual_assert)]
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::let_and_return)]
#![allow(clippy::nonminimal_bool)]
#![allow(clippy::unnested_or_patterns)]
#![allow(clippy::range_plus_one)]
#![allow(clippy::range_minus_one)]
#![allow(clippy::neg_cmp_op_on_partial_ord)]
// C expression/closure patterns
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::map_unwrap_or)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::unnecessary_map_or)]
// Naming and structure: C conventions don't follow Rust idioms
#![allow(clippy::wildcard_imports)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::used_underscore_binding)]
#![allow(clippy::used_underscore_items)]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::struct_field_names)]
#![allow(clippy::pub_underscore_fields)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]
#![allow(clippy::new_without_default)]
#![allow(clippy::missing_const_for_thread_local)]
// Function size: C translation produces long functions with many params
#![allow(clippy::too_many_lines)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::inline_always)]
// Miscellaneous pervasive C-port patterns
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::semicolon_if_nothing_returned)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::explicit_iter_loop)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::needless_late_init)]
#![allow(clippy::mut_mut)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::trivially_copy_pass_by_ref)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::no_effect_underscore_binding)]
#![allow(clippy::redundant_locals)]
#![allow(clippy::needless_lifetimes)]
#![allow(clippy::needless_continue)]
#![allow(clippy::print_literal)]
#![allow(clippy::no_mangle_with_rust_abi)]
#![allow(unsafe_op_in_unsafe_fn)]
#![allow(unknown_lints)]

pub mod appl;
pub mod constants;
pub mod dist;
pub mod dpq;
pub mod error;
pub mod fprec;
pub mod global_state;
pub mod rng;
pub mod special;
pub mod tzone;
#[allow(unused_variables, unused_assignments, unused_mut)]
pub mod tzone_strftime;
pub mod utils;
pub mod xdr;

pub mod android;
#[allow(dead_code, non_camel_case_types)]
pub mod eval;
#[allow(dead_code, non_camel_case_types)]
pub mod graphapp;
pub mod intl;
#[allow(dead_code, non_camel_case_types)]
pub mod mainutils;
#[allow(dead_code, non_camel_case_types)]
pub mod sexp;
#[allow(dead_code, non_camel_case_types)]
pub mod tre;
pub mod trio;
#[allow(dead_code, non_camel_case_types)]
pub mod unix;
