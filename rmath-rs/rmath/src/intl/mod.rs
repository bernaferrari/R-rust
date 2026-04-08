//! GNU gettext internationalization (i18n) support.
//!
//! This module provides a Rust port of the GNU gettext `intl/` library,
//! used internally by R's message catalog system.
//!
//! The public API (`gettext`, `dgettext`, `dcgettext`, `ngettext`, `textdomain`,
//! `bindtextdomain`) is fully wired through to the `dcigettext` implementation.
//! Internal helper functions are ported from C and marked with `allow(dead_code)`
//! until the full gettext pipeline (MO file loading, plural forms) is exercised
//! by integration tests.

#![allow(dead_code)]

// ---------------------------------------------------------------------------
// Core types and global state
// ---------------------------------------------------------------------------
pub(crate) mod types;

// ---------------------------------------------------------------------------
// Platform and initialization
// ---------------------------------------------------------------------------
pub(crate) mod intl_compat;
pub(crate) mod intl_exports;
pub(crate) mod lock;
pub(crate) mod osdep;

// ---------------------------------------------------------------------------
// Locale name handling
// ---------------------------------------------------------------------------
pub(crate) mod explodename;
pub(crate) mod l10nflist;
pub(crate) mod langprefs;
pub(crate) mod localename;

// ---------------------------------------------------------------------------
// Domain management
// ---------------------------------------------------------------------------
pub(crate) mod bindtextdom;
pub(crate) mod finddomain;

// ---------------------------------------------------------------------------
// Message catalog loading and lookup
// ---------------------------------------------------------------------------
pub(crate) mod dcigettext;
pub(crate) mod loadmsgcat;

// ---------------------------------------------------------------------------
// Plural expression handling
// ---------------------------------------------------------------------------
pub(crate) mod plural_exp;
pub(crate) mod plural_parse;

// ---------------------------------------------------------------------------
// Printf implementation
// ---------------------------------------------------------------------------
pub(crate) mod printf;
pub(crate) mod printf_args;
pub(crate) mod printf_parse;
pub(crate) mod vasnprintf;

// ---------------------------------------------------------------------------
// String hashing and search
// ---------------------------------------------------------------------------
pub(crate) mod hash_string;
pub(crate) mod tsearch;

// ---------------------------------------------------------------------------
// Public API wrappers
// ---------------------------------------------------------------------------
pub(crate) mod dcgettext;
pub(crate) mod dcngettext;
pub(crate) mod dgettext;
pub(crate) mod dngettext;
pub(crate) mod gettext;
pub(crate) mod ngettext;
pub(crate) mod textdomain;
