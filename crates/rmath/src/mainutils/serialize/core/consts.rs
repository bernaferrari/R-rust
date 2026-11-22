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

/// REFSXP type for reference markers.
pub const REFSXP: c_int = 255;

/// Reference index packing macros.
pub const PACK_REF_INDEX_BIT: c_int = 0x40000000;

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
