use super::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Initial hash table size for write reference tracking.
pub const HASHSIZE: c_int = 1009;

/// Default serialize version (2 or 3).
pub const R_DEFAULT_SERIALIZE_VERSION: c_int = 3;

/// R version (4, 5, 0) packed as integer.
pub const R_VERSION_450: c_int = (4 << 16) | (5 << 8);

/// R version (2, 3, 0) packed as integer.
pub const R_VERSION_230: c_int = (2 << 16) | (3 << 8);

/// R version (3, 5, 0) packed as integer.
pub const R_VERSION_350: c_int = (3 << 16) | (5 << 8);

/// Writer R version in packed form.
pub const R_VERSION: c_int = crate::mainutils::compatibility_target::PACKED_VERSION;

/// Chunk size for vector I/O.
pub const CHUNK_SIZE: usize = 512;

/// Maximum codeset name length.
pub const R_CODESET_MAX: c_int = 256;

/// Initial reference read table size.
pub const INITIAL_REFREAD_TABLE_SIZE: c_int = 128;

/// REFSXP type for reference markers.
pub const REFSXP: c_int = 255;

/// Reference index packing macros.
pub const PACK_REF_INDEX_BIT: c_int = 0x40000000;

/// Cache size for lazy-load databases.
pub const NC: usize = 100;

/// Limit for lazy-load file caching.
pub const LEN_LIMIT: usize = 10 * 1048576;

/// Maximum element size for serialization.
pub const MAXELTSIZE: usize = 131072;

// Administrative SXP values used in the serialization protocol.
pub const NILVALUE_SXP: c_int = 254;
pub const GLOBALENV_SXP: c_int = 253;
pub const UNBOUNDVALUE_SXP: c_int = 252;
pub const MISSINGARG_SXP: c_int = 251;
pub const BASENAMESPACE_SXP: c_int = 250;
pub const NAMESPACESXP: c_int = 249;
pub const PACKAGESXP: c_int = 248;
pub const PERSISTSXP: c_int = 247;
pub const EMPTYENV_SXP: c_int = 242;
pub const BASEENV_SXP: c_int = 241;
pub const ALTREP_SXP: c_int = 238;

// Flag packing masks.
pub const IS_OBJECT_BIT_MASK: c_int = 1 << 8;
pub const HAS_ATTR_BIT_MASK: c_int = 1 << 9;
pub const HAS_TAG_BIT_MASK: c_int = 1 << 10;
pub const ENCODE_LEVELS: c_int = 1 << 12;
pub const DECODE_TYPE_MASK: c_int = 0xFF;
pub const GROWABLE_MASK: c_int = 1 << 5;

/// Maximum packed reference index.
pub const MAX_PACKED_INDEX: c_int = c_int::MAX >> 8;

/// Serialization format types.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum R_pstream_format_t {
    R_pstream_any_format = 0,
    R_pstream_ascii_format = 1,
    R_pstream_binary_format = 2,
    R_pstream_xdr_format = 3,
    R_pstream_asciihex_format = 4,
}

/// Opaque pointer for stream data.
pub type R_pstream_data_t = *mut c_void;

/// Structure for the R output persistent stream.
#[repr(C)]
pub struct R_outpstream_st {
    pub data: R_pstream_data_t,
    pub type_: R_pstream_format_t,
    pub version: c_int,
    pub OutChar: Option<unsafe extern "C" fn(*mut R_outpstream_st, c_int)>,
    pub OutBytes: Option<unsafe extern "C" fn(*mut R_outpstream_st, *const c_void, c_int)>,
    pub OutPersistHookFunc: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pub OutPersistHookData: SEXP,
}

/// Structure for the R input persistent stream.
#[repr(C)]
pub struct R_inpstream_st {
    pub data: R_pstream_data_t,
    pub type_: R_pstream_format_t,
    pub InChar: Option<unsafe extern "C" fn(*mut R_inpstream_st) -> c_int>,
    pub InBytes: Option<unsafe extern "C" fn(*mut R_inpstream_st, *mut c_void, c_int)>,
    pub InPersistHookFunc: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pub InPersistHookData: SEXP,
    pub native_encoding: [c_char; R_CODESET_MAX as usize],
    pub nat2nat_obj: *mut c_void,
    pub nat2utf8_obj: *mut c_void,
}

pub type R_outpstream_t = *mut R_outpstream_st;
pub type R_inpstream_t = *mut R_inpstream_st;

// ---------------------------------------------------------------------------
// Per-session lazy-load/read state
// ---------------------------------------------------------------------------

pub struct SerializeRuntimeState {
    pub used: usize,
    pub cache_names: [*mut c_char; NC],
    pub cache_ptrs: [*mut c_char; NC],
    pub read_item_depth: c_int,
}

impl Default for SerializeRuntimeState {
    fn default() -> Self {
        Self {
            used: 0,
            cache_names: [ptr::null_mut(); NC],
            cache_ptrs: [ptr::null_mut(); NC],
            read_item_depth: 0,
        }
    }
}

pub fn increment_read_item_depth() {
    with_required_current_instance(|instance| unsafe {
        (*instance).serialize_state.read_item_depth += 1;
    });
}

pub fn decrement_read_item_depth() {
    with_required_current_instance(|instance| unsafe {
        if (*instance).serialize_state.read_item_depth > 0 {
            (*instance).serialize_state.read_item_depth -= 1;
        }
    });
}

#[cfg(test)]
pub fn read_item_depth_for_test() -> c_int {
    with_required_current_instance(|instance| unsafe {
        (*instance).serialize_state.read_item_depth
    })
}

// ---------------------------------------------------------------------------
// Internal binary writer/reader (Vec<u8> based)
// ---------------------------------------------------------------------------
