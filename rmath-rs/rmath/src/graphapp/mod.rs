//! Rust port of R's graphapp GUI library.
//!
//! GraphApp is a cross-platform GUI library originally written in C,
//! used by R for its console and graphics windows. This module provides
//! a faithful Rust translation of the public API with FFI-compatible
//! functions where needed.

pub mod bitmaps;
pub mod buttons;
pub mod clipboard;
pub mod context;
pub mod controls;
pub mod cursors;
pub mod dialogs;
pub mod drawing;
pub mod drawtext;
pub mod events;
pub mod fonts;
pub mod framebuffer;
pub mod gbuttons;
pub mod gdraw;
pub mod image;
pub mod init;
pub mod memory;
pub mod menus;
pub mod metafile;
pub mod objects;
pub mod printer;
pub mod rgb;
pub mod status;
pub mod strings;
pub mod tooltips;
pub mod types;
pub mod windows;
