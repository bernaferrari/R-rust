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
// Older C-port tests assert panic paths without stable panic strings.
#![allow(clippy::should_panic_without_expect)]
#![allow(clippy::approx_constant)]
// Documentation: C code doesn't follow Rust doc conventions
#![allow(clippy::doc_markdown)]
// Safety docs: every unsafe fn in this C FFI port operates on raw SEXP pointers;
// adding 1600+ boilerplate "# Safety" sections adds no real safety value.
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
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
#![allow(clippy::collapsible_if)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::manual_range_patterns)]
#![allow(clippy::manual_clamp)]
#![allow(clippy::manual_midpoint)]
#![allow(clippy::manual_swap)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::manual_assert)]
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::manual_is_multiple_of)]
#![allow(clippy::let_and_return)]
#![allow(clippy::nonminimal_bool)]
#![allow(clippy::unnested_or_patterns)]
#![allow(clippy::range_plus_one)]
#![allow(clippy::range_minus_one)]
#![allow(clippy::neg_cmp_op_on_partial_ord)]
#![allow(clippy::wildcard_in_or_patterns)]
#![allow(clippy::never_loop)]
#![allow(clippy::while_immutable_condition)]
#![allow(clippy::mut_range_bound)]
#![allow(clippy::absurd_extreme_comparisons)]
// C expression/closure patterns
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::map_unwrap_or)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::unnecessary_map_or)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::ignored_unit_patterns)]
#![allow(clippy::unused_self)]
#![allow(clippy::assign_op_pattern)]
#![allow(clippy::identity_op)]
#![allow(clippy::manual_map)]
#![allow(clippy::option_option)]
#![allow(clippy::elidable_lifetime_names)]
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
#![allow(clippy::module_inception)]
#![allow(clippy::items_after_test_module)]
#![allow(clippy::derivable_impls)]
// Function size: C translation produces long functions with many params
#![allow(clippy::too_many_lines)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::inline_always)]
// Container and loop forms: retain C/Fortran-translated storage and iteration
// shapes where pointer identity, stable addresses, or source diffability matter.
#![allow(clippy::explicit_counter_loop)]
#![allow(clippy::vec_box)]
#![allow(clippy::useless_vec)]
#![allow(clippy::iter_cloned_collect)]
#![allow(clippy::stable_sort_primitive)]
#![allow(clippy::same_length_and_capacity)]
#![allow(clippy::manual_memcpy)]
#![allow(clippy::ptr_arg)]
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
#![allow(clippy::ptr_offset_by_literal)]
#![allow(clippy::cast_abs_to_unsigned)]
#![allow(clippy::manual_dangling_ptr)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::bool_comparison)]
#![allow(clippy::doc_overindented_list_items)]
#![allow(clippy::doc_lazy_continuation)]
#![allow(clippy::format_push_string)]
#![allow(clippy::useless_format)]
#![allow(clippy::explicit_auto_deref)]
#![allow(clippy::len_zero)]
#![allow(clippy::write_with_newline)]
#![allow(clippy::drop_non_drop)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::naive_bytecount)]
#![allow(unused_unsafe)]
#![allow(unknown_lints)]

pub mod appl;
pub mod constants;
pub mod dist;
pub mod dpq;
pub mod error;
pub mod fprec;
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
#[cfg(not(target_os = "android"))]
#[allow(dead_code, non_camel_case_types)]
pub mod graphapp;
#[cfg(not(target_os = "android"))]
pub mod intl;
#[allow(dead_code, non_camel_case_types)]
pub mod library;
#[allow(dead_code, non_camel_case_types)]
pub mod mainutils;
#[allow(dead_code, non_camel_case_types)]
pub mod modules;
pub use mainutils as main;
#[allow(unused, dead_code, non_camel_case_types)]
pub mod nmath;
#[allow(dead_code, non_camel_case_types)]
pub mod sexp;
pub use sexp::attrib_core;
#[allow(dead_code, non_camel_case_types)]
pub mod tre;
pub mod trio;
#[allow(dead_code, non_camel_case_types)]
pub mod unix;
