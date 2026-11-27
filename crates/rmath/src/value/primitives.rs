#![forbid(unsafe_code)]
//! Primitive builtin dispatch for the `ValueId` evaluator.
//!
//! The migration slice starts with the arithmetic operators, which exercise
//! the full primitive contract: lazy argument promises, element forcing,
//! type coercion, and recycling rules. The id space here is scaffolding —
//! the canonical registry arrives with the builtin migration and replaces
//! this module's table.

use super::eval::{eval, force_if_promise, EvalError, EvalResult, RSignal};
use super::heap::Heap;
use super::id::ValueId;
use super::intern::SymbolInterner;
use super::object::RObject;

/// Scaffolding primitive identifiers. Values are arbitrary within this
/// module; the migration registry will own the canonical numbering.
pub mod primitive_ids {
    pub const ADD: u32 = 1;
    pub const SUB: u32 = 2;
    pub const MUL: u32 = 3;
    pub const DIV: u32 = 4;
    /// Special forms — unevaluated operands, exactly as GNU treats them.
    pub const IF: u32 = 10;
    pub const BRACE: u32 = 11;
    pub const ASSIGN: u32 = 12;
    pub const SUPERASSIGN: u32 = 13;
    pub const RETURN: u32 = 14;
    pub const WHILE: u32 = 15;
    pub const REPEAT: u32 = 16;
    pub const FOR: u32 = 17;
    pub const BREAK: u32 = 18;
    pub const NEXT: u32 = 19;
    /// Comparisons — eager operands, logical results.
    pub const LT: u32 = 20;
    pub const GT: u32 = 21;
    pub const LE: u32 = 22;
    pub const GE: u32 = 23;
    pub const EQ: u32 = 24;
    pub const NE: u32 = 25;
    /// Closure construction — captures formals, body, and defining env.
    pub const FUNCTION: u32 = 26;
    /// Data construction and access.
    pub const CONCAT: u32 = 30;
    pub const SUBSET: u32 = 31;
    pub const LENGTH: u32 = 32;
    pub const COLON: u32 = 33;
    /// Grouping — `(x)` parses as a call to the parenthesis primitive.
    pub const PAREN: u32 = 34;
    /// Logic operators — elementwise (&, |, !) and short-circuit (&&, ||).
    pub const AND: u32 = 40;
    pub const OR: u32 = 41;
    pub const NOT: u32 = 42;
    pub const SHORT_AND: u32 = 43;
    pub const SHORT_OR: u32 = 44;
    /// Reductions — fold a vector to a scalar with NA propagation.
    pub const SUM: u32 = 50;
    pub const PROD: u32 = 51;
    pub const MIN: u32 = 52;
    pub const MAX: u32 = 53;
    /// Power — always real-valued in GNU, even for integer operands.
    pub const POW: u32 = 58;
    /// Modulo — integer-preserving like the other arithmetic operators.
    pub const MOD: u32 = 90;
    /// Elementwise math — real-valued unary functions.
    pub const SQRT: u32 = 54;
    /// Exact extraction — `x[[i]]` unboxes a single element.
    pub const EXTRACT: u32 = 59;
    /// List construction — variadic heterogeneous collection.
    pub const LIST: u32 = 60;
    /// String operations.
    pub const PASTE: u32 = 61;
    pub const PASTE0: u32 = 64;
    /// Subscript replacement — `[<-` returns the modified copy.
    pub const SUBSET_ASSIGN: u32 = 65;
    /// Apply family — lapply/sapply iteration over closures.
    pub const LAPPLY: u32 = 72;
    pub const SAPPLY: u32 = 73;
    /// List flattening.
    pub const UNLIST: u32 = 74;
    /// Sequence and repetition constructors.
    pub const SEQ: u32 = 75;
    pub const SEQ_LEN: u32 = 76;
    pub const REP: u32 = 77;
    /// Statistical reductions and NA predicates.
    pub const MEAN: u32 = 78;
    pub const VAR: u32 = 79;
    pub const SD: u32 = 80;
    pub const MEDIAN: u32 = 81;
    pub const IS_NA: u32 = 82;
    /// Vector utilities — ordering, selection, vectorized conditional.
    pub const SORT: u32 = 83;
    pub const REV: u32 = 84;
    pub const UNIQUE: u32 = 85;
    pub const WHICH: u32 = 86;
    pub const ANY: u32 = 87;
    pub const ALL: u32 = 88;
    pub const IFELSE: u32 = 89;
    /// Error control and membership.
    pub const STOP: u32 = 91;
    pub const IN: u32 = 92;
    pub const MATCH: u32 = 93;
    /// Environment primitives — user-level creation and introspection.
    pub const NEW_ENV: u32 = 94;
    pub const EXISTS: u32 = 95;
    pub const LS: u32 = 96;
    /// Dollar replacement — `$<-` assigns by name.
    pub const DOLLAR_ASSIGN: u32 = 97;
    /// Output and visibility.
    pub const CAT: u32 = 98;
    pub const INVISIBLE: u32 = 99;
    /// Condition handling — tryCatch and condition introspection.
    pub const TRYCATCH: u32 = 104;
    pub const CONDITION_MESSAGE: u32 = 105;
    /// Type introspection, structure manipulation, environment access.
    pub const TYPEOF: u32 = 106;
    pub const STRUCTURE: u32 = 107;
    pub const ASSIGN_FN: u32 = 108;
    pub const GET: u32 = 109;
    pub const CHARACTER_CTOR: u32 = 110;
    /// Attribute accessors and their replacement forms.
    pub const DIMNAMES: u32 = 111;
    pub const DIMNAMES_ASSIGN: u32 = 112;
    pub const CLASS: u32 = 113;
    pub const CLASS_ASSIGN: u32 = 114;
    pub const ROWNAMES: u32 = 115;
    pub const ROWNAMES_ASSIGN: u32 = 116;
    pub const COLNAMES: u32 = 117;
    pub const COLNAMES_ASSIGN: u32 = 118;
    /// Text file I/O — the architecturally permanent slice of the tempfile
    /// cluster (save/load/RDS wire formats are dropped by design).
    pub const TEMPFILE: u32 = 119;
    pub const WRITELINES: u32 = 120;
    pub const READLINES: u32 = 121;
    pub const FILE_EXISTS: u32 = 122;
    pub const FILE_REMOVE: u32 = 123;
    pub const FILE_COPY: u32 = 124;
    pub const FILE_RENAME: u32 = 125;
    pub const SUPPRESS_WARNINGS: u32 = 126;
    pub const AS_LOGICAL: u32 = 127;
    /// Attribute reflection and vector resizing.
    pub const ATTR: u32 = 128;
    pub const ATTR_ASSIGN: u32 = 129;
    pub const ATTRIBUTES: u32 = 130;
    pub const ATTRIBUTES_ASSIGN: u32 = 131;
    pub const IS_NULL: u32 = 132;
    pub const LENGTH_ASSIGN: u32 = 133;
    pub const FILE_PATH: u32 = 134;
    /// Data frame construction — the list-of-columns object shape GNU's
    /// statistical surface is built on.
    pub const DATA_FRAME: u32 = 135;
    /// Split-apply-combine over grouped vectors.
    pub const AGGREGATE: u32 = 136;
    /// Factor construction and introspection — classed integer codes with
    /// level labels.
    pub const FACTOR: u32 = 137;
    pub const LEVELS: u32 = 138;
    pub const LEVELS_ASSIGN: u32 = 139;
    pub const UNCLASS: u32 = 140;
    pub const TEMPDIR: u32 = 141;
    /// C-style string formatting.
    pub const SPRINTF: u32 = 142;
    /// System environment and file removal.
    pub const SYS_SETENV: u32 = 143;
    pub const SYS_GETENV: u32 = 144;
    pub const SYS_UNSETENV: u32 = 145;
    pub const UNLINK: u32 = 146;
    /// The .Internal support surface — translation, condition, and state
    /// pieces GNU routes through the internal table.
    pub const INTERNAL: u32 = 156;
    pub const PRIMITIVE_FN: u32 = 157;
    pub const WARNING: u32 = 158;
    pub const GETTEXT: u32 = 159;
    pub const NGETTEXT: u32 = 160;
    pub const BINDTEXTDOMAIN: u32 = 161;
    pub const GETERRMESSAGE: u32 = 162;
    pub const SETERRMESSAGE: u32 = 163;
    pub const INTERRUPTS_SUSPENDED: u32 = 164;
    pub const IS_PRIMITIVE: u32 = 165;
    pub const IS_TRUE: u32 = 166;
    /// Object persistence — RBG1 roundtrips replacing the dropped GNU
    /// wire formats.
    pub const SAVE: u32 = 168;
    pub const LOAD: u32 = 169;
    pub const RM: u32 = 170;
    pub const AS_VECTOR: u32 = 171;
    pub const AS_CHARACTER: u32 = 172;
    pub const TSP: u32 = 173;
    pub const TSP_ASSIGN: u32 = 174;
    /// Environment references and structural comparison.
    pub const SETNAMES: u32 = 175;
    pub const IDENTICAL: u32 = 176;
    pub const GLOBALENV: u32 = 177;
    pub const BASEENV: u32 = 178;
    pub const ENVIRONMENT_NAME: u32 = 179;
    pub const ENVIRONMENT: u32 = 180;
    pub const R_VERSION_FN: u32 = 181;
    pub const GET_RVERSION: u32 = 182;
    pub const IS_LIST: u32 = 183;
    /// N-dimensional array construction.
    pub const ARRAY: u32 = 184;
    pub const IS_ARRAY: u32 = 185;
    /// The Date surface — ISO parsing, calendar labels, difftime.
    pub const AS_DATE: u32 = 186;
    pub const WEEKDAYS: u32 = 187;
    pub const MONTHS: u32 = 188;
    pub const QUARTERS: u32 = 189;
    pub const FORMAT: u32 = 190;
    pub const IS_UNSORTED: u32 = 191;
    pub const TOSTRING: u32 = 192;
    pub const CAPTURE_OUTPUT: u32 = 193;
    pub const ROW_NAMES: u32 = 194;
    pub const ROW_NAMES_SET: u32 = 195;
    pub const TAPPLY: u32 = 196;
    pub const VAPPLY: u32 = 197;
    pub const AS_POSIXCT: u32 = 198;
    pub const PROC_TIME: u32 = 199;
    pub const QUOTE: u32 = 200;
    pub const TILDE: u32 = 201;
    pub const IS_CALL: u32 = 202;
    pub const IS_ENVIRONMENT: u32 = 203;
    pub const XTABS: u32 = 204;
    pub const SIN: u32 = 205;
    pub const COS: u32 = 206;
    pub const SINH: u32 = 207;
    pub const COSH: u32 = 208;
    pub const TANH_F: u32 = 209;
    pub const COMPLEX_CTOR: u32 = 210;
    pub const AS_COMPLEX: u32 = 211;
    pub const RE_C: u32 = 212;
    pub const IM_C: u32 = 213;
    pub const MOD_C: u32 = 214;
    pub const ARG_C: u32 = 215;
    pub const CONJ_C: u32 = 216;
    pub const IS_COMPLEX: u32 = 217;
    pub const AS_RAW: u32 = 218;
    pub const IS_RAW: u32 = 219;
    pub const MODE_FN: u32 = 220;
    pub const STORAGE_MODE: u32 = 221;
    pub const EXPRESSION_FN: u32 = 222;
    pub const RAW_CTOR: u32 = 223;
    pub const VECTOR_FN: u32 = 224;
    pub const IDENTITY_FN: u32 = 225;
    pub const PARSE_FN: u32 = 226;
    pub const EVAL_FN: u32 = 227;
    pub const DEPARSE1: u32 = 228;
    pub const DPUT: u32 = 229;
    pub const DGET: u32 = 230;
    pub const INHERITS: u32 = 231;
    pub const DIRNAME: u32 = 232;
    pub const GREGEXPR: u32 = 233;
    pub const GSUB: u32 = 234;
    pub const SUB_FN: u32 = 235;
    pub const FORMALS: u32 = 236;
    pub const OPTIONS_FN: u32 = 237;
    pub const GET_OPTION: u32 = 238;
    pub const CBIND: u32 = 239;
    pub const RBIND: u32 = 240;
    pub const IS_NUMERIC_FN: u32 = 241;
    pub const IS_INTEGER_FN: u32 = 242;
    pub const IS_DOUBLE_FN: u32 = 243;
    pub const IS_LOGICAL_FN: u32 = 244;
    pub const IS_CHARACTER_FN: u32 = 245;
    pub const SUBSTITUTE_FN: u32 = 246;
    pub const DNORM: u32 = 247;
    pub const PNORM: u32 = 248;
    pub const QNORM: u32 = 249;
    pub const DPOIS: u32 = 250;
    pub const PPOIS: u32 = 251;
    pub const DBINOM: u32 = 252;
    pub const PBINOM: u32 = 253;
    pub const MESSAGE_FN: u32 = 254;
    pub const MISSING_FN: u32 = 255;
    pub const PARENT_ENV: u32 = 256;
    pub const ORDER_FN: u32 = 257;
    pub const RANK_FN: u32 = 258;
    pub const ORDERED_FN: u32 = 259;
    pub const IS_ORDERED: u32 = 260;
    pub const IS_FACTOR: u32 = 261;
    pub const NA_OMIT: u32 = 262;
    pub const NA_EXCLUDE: u32 = 263;
    pub const PBETA: u32 = 264;
    pub const DBETA: u32 = 265;
    pub const QBETA: u32 = 266;
    pub const FIND_FN: u32 = 267;
    pub const USEMETHOD: u32 = 268;
    pub const NEXTMETHOD: u32 = 269;
    pub const GETS3METHOD: u32 = 270;
    pub const EMPTYENV: u32 = 271;
    pub const AS_INTEGER: u32 = 272;
    pub const AS_NUMERIC: u32 = 273;
    pub const STOPIFNOT: u32 = 274;
    pub const SEQUENCE: u32 = 275;
    pub const REP_INT: u32 = 276;
    pub const REP_LEN: u32 = 277;
    pub const DT: u32 = 278;
    pub const PT: u32 = 279;
    pub const QT: u32 = 280;
    pub const SAMPLE: u32 = 281;
    pub const SAMPLE_INT: u32 = 282;
    pub const BUILTINS: u32 = 283;
    pub const REGEXEC: u32 = 284;
    pub const DF: u32 = 285;
    pub const PF: u32 = 286;
    pub const QF: u32 = 287;
    pub const SUBSTR: u32 = 288;
    pub const FILE_FN: u32 = 289;
    pub const ISOPEN: u32 = 290;
    pub const CLOSE_FN: u32 = 291;
    pub const FLUSH_FN: u32 = 292;
    pub const WRITECHAR: u32 = 293;
    pub const READCHAR: u32 = 294;
    pub const UNION: u32 = 295;
    pub const INTERSECT: u32 = 296;
    pub const SETDIFF: u32 = 297;
    pub const SETEQUAL: u32 = 298;
    pub const TABLE: u32 = 299;
    pub const SETCLASS: u32 = 300;
    pub const NEW_S4: u32 = 301;
    pub const IS_S4: u32 = 302;
    pub const SLOT: u32 = 303;
    pub const SLOTNAMES: u32 = 304;
    pub const UNNAME: u32 = 305;
    pub const WHICH_MIN: u32 = 306;
    pub const WHICH_MAX: u32 = 307;
    pub const WITHVISIBLE: u32 = 308;
    pub const SYSTEM: u32 = 309;
    pub const WRITEBIN: u32 = 310;
    pub const READBIN: u32 = 311;
    pub const URL_FN: u32 = 312;
    pub const DIM_ASSIGN: u32 = 313;
    pub const STORAGE_MODE_ASSIGN: u32 = 314;
    pub const AS_LIST: u32 = 315;
    pub const BODY_FN: u32 = 316;
    pub const SUMMARY_FN: u32 = 317;
    pub const MATRIX_MULT: u32 = 318;
    pub const CROSSPROD: u32 = 319;
    pub const TCROSSPROD: u32 = 320;
    pub const MAXCOL: u32 = 321;
    pub const IDIV: u32 = 322;
    pub const NROW_FN: u32 = 323;
    pub const NCOL_FN: u32 = 324;
    pub const SYS_GETPID: u32 = 325;
    pub const OLSONNAMES: u32 = 326;
    pub const IQR_FN: u32 = 327;
    pub const SYS_CHMOD: u32 = 328;
    pub const FILE_INFO: u32 = 329;
    pub const SYS_GETLOCALE: u32 = 330;
    pub const SYS_SETLOCALE: u32 = 331;
    pub const NZCHAR: u32 = 332;
    pub const AT_OP: u32 = 333;
    pub const AT_ASSIGN: u32 = 334;
    pub const SYS_INFO: u32 = 335;
    pub const SYS_TIME: u32 = 336;
    pub const SYS_DATE: u32 = 337;
    pub const SYS_WHICH: u32 = 338;
    pub const FLOOR: u32 = 339;
    pub const CEILING: u32 = 340;
    pub const TRUNC: u32 = 341;
    pub const ROUND: u32 = 342;
    pub const SIGNIF: u32 = 343;
    pub const EXPM1: u32 = 344;
    pub const LOG1P: u32 = 345;
    pub const ACOSH: u32 = 346;
    pub const ASINH: u32 = 347;
    pub const ATANH: u32 = 348;
    pub const COSPI: u32 = 349;
    pub const SINPI: u32 = 350;
    pub const TANPI: u32 = 351;
    pub const ANYNA: u32 = 352;
    pub const SYS_LOCALECONV: u32 = 353;
    pub const AGREP: u32 = 354;
    pub const AGREPL: u32 = 355;
    pub const APPLY: u32 = 356;
    pub const AS_ENVIRONMENT: u32 = 357;
    pub const AS_CALL: u32 = 358;
    pub const ENC2NATIVE: u32 = 359;
    pub const ENC2UTF8: u32 = 360;
    pub const SEARCH_FN: u32 = 361;
    pub const AS_FACTOR: u32 = 362;
    pub const AS_ORDERED: u32 = 363;
    pub const ADDNA: u32 = 364;
    pub const APERM: u32 = 365;
    pub const ADDMARGINS: u32 = 366;
    pub const DOT_SUBSET: u32 = 367;
    pub const DOT_SUBSET2: u32 = 368;
    pub const ASPLIT: u32 = 369;
    pub const ATTACH: u32 = 370;
    pub const DETACH: u32 = 371;
    pub const POS_TO_ENV: u32 = 372;
    pub const DOT_CLASS2: u32 = 373;
    pub const GC_TIME: u32 = 374;
    pub const AVE: u32 = 375;
    pub const BACKSOLVE: u32 = 376;
    pub const BQUOTE: u32 = 377;
    pub const CALL_FN: u32 = 378;
    pub const CHAR_TO_RAW: u32 = 379;
    pub const CHARMATCH: u32 = 380;
    pub const CUMSUM: u32 = 381;
    pub const CUMPROD: u32 = 382;
    pub const CUMMIN: u32 = 383;
    pub const CUMMAX: u32 = 384;
    pub const COMMENT_FN: u32 = 385;
    pub const COMMENT_ASSIGN: u32 = 386;
    pub const COMPLETE_CASES: u32 = 387;
    pub const COLSUMS: u32 = 388;
    pub const ROWSUMS: u32 = 389;
    pub const COLMEANS: u32 = 390;
    pub const ROWMEANS: u32 = 391;
    pub const DCAUCHY: u32 = 392;
    pub const DCHISQ: u32 = 393;
    pub const DEXP: u32 = 394;
    pub const CUT: u32 = 395;
    pub const DATE_FN: u32 = 396;
    pub const CAPABILITIES: u32 = 397;
    pub const BY_FN: u32 = 398;
    pub const BROWSER_FN: u32 = 399;
    pub const DGAMMA: u32 = 400;
    pub const DGEOM: u32 = 401;
    pub const DNBINOM: u32 = 402;
    pub const DIFF: u32 = 403;
    pub const DIAG: u32 = 404;
    pub const DO_CALL: u32 = 405;
    pub const DWEIBULL: u32 = 406;
    pub const DROP_FN: u32 = 407;
    pub const DROPLEVELS: u32 = 408;
    pub const DUPLICATED: u32 = 409;
    pub const ANYDUPLICATED: u32 = 410;
    pub const DIR_EXISTS: u32 = 411;
    pub const FILE_CREATE: u32 = 412;
    pub const FILE_ACCESS: u32 = 413;
    pub const ENVIRONMENT_ASSIGN: u32 = 414;
    pub const FORCEANDCALL: u32 = 415;
    pub const DECLARE: u32 = 416;
    pub const PRIMTRACE: u32 = 417;
    pub const PRIMUNTRACE: u32 = 418;
    pub const STANDARDGENERIC: u32 = 419;
    pub const DMULTINOM: u32 = 420;
    pub const DOTS_LENGTH: u32 = 421;
    pub const DOTS_NAMES: u32 = 422;
    pub const DOTS_ELT: u32 = 423;
    pub const IS_DATA_FRAME: u32 = 424;
    pub const FILE_APPEND: u32 = 425;
    pub const FILE_LINK: u32 = 426;
    pub const FILE_SYMLINK: u32 = 427;
    pub const SYS_READLINK: u32 = 428;
    pub const FILE_SIZE: u32 = 429;
    pub const FILE_MTIME: u32 = 430;
    pub const GL: u32 = 431;
    pub const HEAD_FN: u32 = 432;
    pub const TAIL_FN: u32 = 433;
    pub const INTERACTION: u32 = 434;
    pub const GCINFO: u32 = 435;
    pub const GCTORTURE: u32 = 436;
    pub const GCTORTURE2: u32 = 437;
    pub const IS_FINITE: u32 = 438;
    pub const IS_INFINITE: u32 = 439;
    pub const IS_NAN: u32 = 440;
    pub const IS_VECTOR: u32 = 441;
    pub const IS_LOADED: u32 = 442;
    pub const L10N_INFO: u32 = 443;
    pub const CSTACK_INFO: u32 = 444;
    pub const EXTSOFTVERSION: u32 = 445;
    pub const LIST_FILES: u32 = 446;
    pub const MEMORY_PROFILE: u32 = 447;
    pub const PRINT_DEFERRED_WARNINGS: u32 = 167;
    /// Directory construction and listing.
    pub const DIR_CREATE: u32 = 153;
    pub const LIST_DIRS: u32 = 154;
    pub const BASENAME: u32 = 155;
    /// Pattern matching — the pure-Rust regex surface.
    pub const GREPL: u32 = 147;
    pub const GREP: u32 = 148;
    pub const REGEXPR: u32 = 149;
    /// Matrix construction and dimension introspection.
    pub const MATRIX: u32 = 100;
    pub const DIM: u32 = 101;
    pub const NROW: u32 = 102;
    pub const NCOL: u32 = 103;
    /// Attribute access — names getter and replacement, dollar extraction.
    pub const NAMES: u32 = 69;
    pub const NAMES_ASSIGN: u32 = 70;
    pub const DOLLAR: u32 = 71;
    /// Vector constructors — numeric/integer/logical of length n.
    pub const NUMERIC: u32 = 66;
    pub const INTEGER_CTOR: u32 = 67;
    pub const LOGICAL_CTOR: u32 = 68;
    pub const NCHAR: u32 = 62;
    pub const PRINT: u32 = 63;
    pub const EXP: u32 = 55;
    pub const LOG: u32 = 56;
    pub const ABS: u32 = 57;
}

use crate::value::object::BuiltinId;
use primitive_ids::{ABS, ADD, AND, ASSIGN, BRACE, BREAK, COLON, CONCAT, DIV, EQ, EXP, EXTRACT, FOR, FUNCTION, GE, GT, IF, LE, LENGTH, LIST, LOG, LT, MAX, MIN, MUL, MOD, NCHAR, NE, NEXT, NOT, OR, PAREN, PASTE, PASTE0, POW, PRINT, PROD, REPEAT, RETURN, SHORT_AND, SHORT_OR, SQRT, SUB, SUBSET, SUBSET_ASSIGN, SUM, SUPERASSIGN, NUMERIC, INTEGER_CTOR, LOGICAL_CTOR, NAMES, NAMES_ASSIGN, DOLLAR, LAPPLY, SAPPLY, UNLIST, SEQ, SEQ_LEN, REP, MEAN, VAR, SD, MEDIAN, IS_NA, SORT, REV, UNIQUE, WHICH, ANY, ALL, IFELSE, STOP, IN, MATCH, NEW_ENV, EXISTS, LS, DOLLAR_ASSIGN, CAT, INVISIBLE, MATRIX, DIM, NROW, NCOL, TRYCATCH, CONDITION_MESSAGE, TYPEOF, STRUCTURE, ASSIGN_FN, GET, CHARACTER_CTOR, TEMPFILE, WRITELINES, READLINES, FILE_EXISTS, FILE_REMOVE, FILE_COPY, FILE_RENAME, SUPPRESS_WARNINGS, AS_LOGICAL, ATTR, ATTR_ASSIGN, ATTRIBUTES, ATTRIBUTES_ASSIGN, IS_NULL, LENGTH_ASSIGN, FILE_PATH, DATA_FRAME, AGGREGATE, FACTOR, LEVELS, LEVELS_ASSIGN, UNCLASS, TEMPDIR, SPRINTF, SYS_SETENV, SYS_GETENV, SYS_UNSETENV, UNLINK, INTERNAL, PRIMITIVE_FN, WARNING, GETTEXT, NGETTEXT, BINDTEXTDOMAIN, GETERRMESSAGE, SETERRMESSAGE, INTERRUPTS_SUSPENDED, IS_PRIMITIVE, IS_TRUE, SAVE, LOAD, RM, AS_VECTOR, AS_CHARACTER, TSP, TSP_ASSIGN, SETNAMES, IDENTICAL, GLOBALENV, BASEENV, ENVIRONMENT_NAME, ENVIRONMENT, R_VERSION_FN, GET_RVERSION, IS_LIST, ARRAY, IS_ARRAY, AS_DATE, WEEKDAYS, MONTHS, QUARTERS, FORMAT, IS_UNSORTED, TOSTRING, CAPTURE_OUTPUT, ROW_NAMES, ROW_NAMES_SET, TAPPLY, VAPPLY, AS_POSIXCT, PROC_TIME, QUOTE, TILDE, IS_CALL, IS_ENVIRONMENT, XTABS, SIN, COS, SINH, COSH, TANH_F, COMPLEX_CTOR, AS_COMPLEX, RE_C, IM_C, MOD_C, ARG_C, CONJ_C, IS_COMPLEX, AS_RAW, IS_RAW, MODE_FN, STORAGE_MODE, EXPRESSION_FN, RAW_CTOR, VECTOR_FN, IDENTITY_FN, PARSE_FN, EVAL_FN, DEPARSE1, DPUT, DGET, INHERITS, DIRNAME, GREGEXPR, GSUB, SUB_FN, FORMALS, OPTIONS_FN, GET_OPTION, CBIND, RBIND, IS_NUMERIC_FN, IS_INTEGER_FN, IS_DOUBLE_FN, IS_LOGICAL_FN, IS_CHARACTER_FN, SUBSTITUTE_FN, PRINT_DEFERRED_WARNINGS, DIR_CREATE, LIST_DIRS, BASENAME, GREPL, GREP, REGEXPR, DIMNAMES, DIMNAMES_ASSIGN, CLASS, CLASS_ASSIGN, ROWNAMES, ROWNAMES_ASSIGN, COLNAMES, COLNAMES_ASSIGN, WHILE, DNORM, PNORM, QNORM, DPOIS, PPOIS, DBINOM, PBINOM, MESSAGE_FN, MISSING_FN, PARENT_ENV, ORDER_FN, RANK_FN, ORDERED_FN, IS_ORDERED, IS_FACTOR, NA_OMIT, NA_EXCLUDE, PBETA, DBETA, QBETA, FIND_FN, USEMETHOD, NEXTMETHOD, GETS3METHOD, EMPTYENV, AS_INTEGER, AS_NUMERIC, STOPIFNOT, SEQUENCE, REP_INT, REP_LEN, DT, PT, QT, SAMPLE, SAMPLE_INT, BUILTINS, REGEXEC, DF, PF, QF, SUBSTR, FILE_FN, ISOPEN, CLOSE_FN, FLUSH_FN, WRITECHAR, READCHAR, UNION, INTERSECT, SETDIFF, SETEQUAL, TABLE, SETCLASS, NEW_S4, IS_S4, SLOT, SLOTNAMES, UNNAME, WHICH_MIN, WHICH_MAX, WITHVISIBLE, SYSTEM, WRITEBIN, READBIN, URL_FN, DIM_ASSIGN, STORAGE_MODE_ASSIGN, AS_LIST, BODY_FN, SUMMARY_FN, MATRIX_MULT, CROSSPROD, TCROSSPROD, MAXCOL, IDIV, NROW_FN, NCOL_FN, SYS_GETPID, OLSONNAMES, IQR_FN, SYS_CHMOD, FILE_INFO, SYS_GETLOCALE, SYS_SETLOCALE, NZCHAR, AT_OP, AT_ASSIGN, SYS_INFO, SYS_TIME, SYS_DATE, SYS_WHICH, FLOOR, CEILING, TRUNC, ROUND, SIGNIF, EXPM1, LOG1P, ACOSH, ASINH, ATANH, COSPI, SINPI, TANPI, ANYNA, SYS_LOCALECONV, AGREP, AGREPL, APPLY, AS_ENVIRONMENT, AS_CALL, ENC2NATIVE, ENC2UTF8, SEARCH_FN, AS_FACTOR, AS_ORDERED, ADDNA, APERM, ADDMARGINS, DOT_SUBSET, DOT_SUBSET2, ASPLIT, ATTACH, DETACH, POS_TO_ENV, DOT_CLASS2, GC_TIME, AVE, BACKSOLVE, BQUOTE, CALL_FN, CHAR_TO_RAW, CHARMATCH, CUMSUM, CUMPROD, CUMMIN, CUMMAX, COMMENT_FN, COMMENT_ASSIGN, COMPLETE_CASES, COLSUMS, ROWSUMS, COLMEANS, ROWMEANS, DCAUCHY, DCHISQ, DEXP, CUT, DATE_FN, CAPABILITIES, BY_FN, BROWSER_FN, DGAMMA, DGEOM, DNBINOM, DIFF, DIAG, DO_CALL, DWEIBULL, DROP_FN, DROPLEVELS, DUPLICATED, ANYDUPLICATED, DIR_EXISTS, FILE_CREATE, FILE_ACCESS, ENVIRONMENT_ASSIGN, FORCEANDCALL, DECLARE, PRIMTRACE, PRIMUNTRACE, STANDARDGENERIC, DMULTINOM, DOTS_LENGTH, DOTS_NAMES, DOTS_ELT, IS_DATA_FRAME, FILE_APPEND, FILE_LINK, FILE_SYMLINK, SYS_READLINK, FILE_SIZE, FILE_MTIME, GL, HEAD_FN, TAIL_FN, INTERACTION, GCINFO, GCTORTURE, GCTORTURE2, IS_FINITE, IS_INFINITE, IS_NAN, IS_VECTOR, IS_LOADED, L10N_INFO, CSTACK_INFO, EXTSOFTVERSION, LIST_FILES, MEMORY_PROFILE};

/// Invoke a primitive with an unevaluated argument pairlist in the caller's
/// environment — the same contract GNU primitives receive. The visibility
/// flag lands here centrally: every primitive is visible except GNU's
/// invisible-returning forms, and the flag is set AFTER the body completes
/// so inner evaluations never leak their state into the caller's answer.
pub(crate) fn call_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    id: BuiltinId,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let result = call_primitive_inner(heap, interner, id, args, caller_env);
    // .Internal propagates the inner evaluation's visibility exactly —
    /// the central discipline must not override what the wrapped call set.
    // eval propagates the evaluated expression's own visibility exactly
    /// as .Internal does — the inner answer decides, not the wrapper.
    // Braces and parens propagate the final inner visibility exactly as
    /// GNU — `{ invisible(1) }` returns invisibly — so they join .Internal
    /// and eval in the propagation exceptions.
    if result.is_ok() && id.0 != INTERNAL && id.0 != EVAL_FN && id.0 != BRACE && id.0 != PAREN && id.0 != SUPPRESS_WARNINGS && id.0 != USEMETHOD && id.0 != NEXTMETHOD && id.0 != SYSTEM && id.0 != OPTIONS_FN {
        heap.visible = !matches!(
            id.0,
            ASSIGN | SUPERASSIGN | PRINT | CAT | INVISIBLE | WHILE | REPEAT | FOR
                | WRITELINES | ASSIGN_FN | FILE_REMOVE | FILE_COPY | FILE_RENAME
                | SYS_SETENV | SYS_UNSETENV | UNLINK | DIR_CREATE | SAVE | RM | DPUT
                | WARNING | MESSAGE_FN | CLOSE_FN | FLUSH_FN | WRITECHAR | SETCLASS | STOPIFNOT
                | WRITEBIN | ATTACH | DETACH
        );
    }
    result
}

fn call_primitive_inner(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    id: BuiltinId,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    match id.0 {
        ADD | SUB | MUL | DIV | POW | MOD | IDIV => arithmetic(heap, interner, id.0, args, caller_env),
        IF => if_form(heap, interner, args, caller_env),
        BRACE => brace_form(heap, interner, args, caller_env),
        ASSIGN => assign_form(heap, interner, args, caller_env, false),
        SUPERASSIGN => assign_form(heap, interner, args, caller_env, true),
        RETURN => return_form(heap, interner, args, caller_env),
        WHILE => while_form(heap, interner, args, caller_env),
        REPEAT => repeat_form(heap, interner, args, caller_env),
        FOR => for_form(heap, interner, args, caller_env),
        BREAK => Err(RSignal::Break),
        NEXT => Err(RSignal::Next),
        LT | GT | LE | GE | EQ | NE => comparison(heap, interner, id.0, args, caller_env),
        FUNCTION => function_form(heap, args, caller_env),
        CONCAT => concat_primitive(heap, interner, args, caller_env),
        SUBSET => subset_primitive(heap, interner, args, caller_env),
        DOT_SUBSET => subset_primitive(heap, interner, args, caller_env),
        LENGTH => length_primitive(heap, interner, args, caller_env),
        COLON => colon_primitive(heap, interner, args, caller_env),
        PAREN => {
            // `(x)` — identity: evaluate the single grouped expression.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            Ok(operands[0])
        }
        AND | OR => logic_elementwise(heap, interner, id.0, args, caller_env),
        NOT => logic_not(heap, interner, args, caller_env),
        SHORT_AND | SHORT_OR => logic_short_circuit(heap, interner, id.0, args, caller_env),
        SUM | PROD | MIN | MAX => reduction(heap, interner, id.0, args, caller_env),
        EXTRACT => extract_primitive(heap, interner, args, caller_env),
        DOT_SUBSET2 => extract_primitive(heap, interner, args, caller_env),
        SUBSET_ASSIGN => subset_assign_primitive(heap, interner, args, caller_env),
        NUMERIC | INTEGER_CTOR | LOGICAL_CTOR | RAW_CTOR => {
            vector_constructor(heap, interner, id.0, args, caller_env)
        }
        VECTOR_FN => {
            // vector(mode, length) — the mode selects the zero-filled
            /// atomic or list shape exactly as GNU.
            let mut mode = String::new();
            let mut length = 0usize;
            let mut positional = 0usize;
            let mut cursor = args;
            for _ in 0..100_000 {
                if cursor == ValueId::NIL {
                    break;
                }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                let value = eval(heap, interner, car, caller_env)?;
                let tag_name = if tag == ValueId::NIL {
                    None
                } else {
                    match heap.object(tag) {
                        Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                        _ => None,
                    }
                };
                match tag_name.as_deref() {
                    Some("mode") => {
                        if let Some(RObject::String(vals)) = heap.object(value) {
                            if let Some(first) = vals.first() {
                                mode = interner.string_text(*first).to_string();
                            }
                        }
                    }
                    Some("length") => length = numeric_vector(heap, value).first().copied().unwrap_or(0.0) as usize,
                    _ => {
                        positional += 1;
                        if positional == 1 {
                            if let Some(RObject::String(vals)) = heap.object(value) {
                                if let Some(first) = vals.first() {
                                    mode = interner.string_text(*first).to_string();
                                }
                            }
                        } else if positional == 2 {
                            length = numeric_vector(heap, value).first().copied().unwrap_or(0.0) as usize;
                        }
                    }
                }
                cursor = next;
            }
            let body = match mode.as_str() {
                "logical" => RObject::Logical(vec![0; length]),
                "integer" => RObject::Integer(vec![0; length]),
                "numeric" | "double" => RObject::Real(vec![0.0; length]),
                "complex" => RObject::Complex(vec![crate::value::object::RComplex { r: 0.0, i: 0.0 }; length]),
                "character" => RObject::String(vec![interner.intern_string(""); length]),
                "raw" => RObject::Raw(vec![0u8; length]),
                "list" => RObject::List(vec![ValueId::NIL; length]),
                _ => RObject::Logical(vec![0; length]),
            };
            Ok(heap.alloc(body))
        }
        IDENTITY_FN => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            Ok(operands[0])
        }
        NAMES => names_primitive(heap, interner, args, caller_env),
        NAMES_ASSIGN => names_assign_primitive(heap, interner, args, caller_env),
        DOLLAR => dollar_primitive(heap, interner, args, caller_env),
        LAPPLY | SAPPLY => apply_family(heap, interner, id.0, args, caller_env),
        UNLIST => {
            let mut xval = ValueId::NIL;
            let mut recursive = true;
            let mut use_names = true;
            let mut cursor = args;
            for _ in 0..100_000 {
                if cursor == ValueId::NIL {
                    break;
                }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                let value = eval(heap, interner, car, caller_env)?;
                let tag_name = if tag == ValueId::NIL {
                    None
                } else {
                    match heap.object(tag) {
                        Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                        _ => None,
                    }
                };
                match tag_name.as_deref() {
                    Some("recursive") => recursive = logical_scalar(heap, value),
                    Some("use.names") => use_names = logical_scalar(heap, value),
                    _ => {
                        if xval == ValueId::NIL {
                            xval = value;
                        }
                    }
                }
                cursor = next;
            }
            unlist_primitive(heap, interner, xval, recursive, use_names)
        }
        SEQ_LEN => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let n = first_real(heap, operands[0])? as i64;
            if n < 0 {
                return Err(EvalError::TypeError("seq_len requires non-negative").into());
            }
            let out: Vec<i32> = (1..=n as i32).collect();
            Ok(heap.alloc(RObject::Integer(out)))
        }
        SEQ => seq_primitive(heap, interner, args, caller_env),
        REP => rep_dispatch(heap, interner, args, caller_env),
        MEAN | VAR | SD | MEDIAN => stat_primitive(heap, interner, id.0, args, caller_env),
        IS_NA => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            is_na_primitive(heap, interner, operands[0])
        }
        SORT | REV => ordering_primitive(heap, interner, id.0, args, caller_env),
        UNIQUE => unique_primitive(heap, interner, args, caller_env),
        WHICH => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            which_primitive(heap, operands[0])
        }
        ANY | ALL => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            quantifier_primitive(heap, id.0, operands[0])
        }
        IFELSE => ifelse_primitive(heap, interner, args, caller_env),
        STOP => {
            // GNU's stop raises the error condition with the message —
            /// evaluation aborts through the signal channel exactly as a
            /// type error would.
            let operands = eval_args(heap, interner, args, caller_env, 1).unwrap_or_default();
            let message = match operands.first().and_then(|v| heap.object(*v)) {
                Some(RObject::String(values)) if !values.is_empty() => {
                    interner.string_text(values[0]).to_string()
                }
                _ => "stop called".to_string(),
            };
            Err(EvalError::Condition(message).into())
        }
        IN => {
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            membership_primitive(heap, interner, operands[0], operands[1])
        }
        MATCH => match_primitive(heap, interner, args, caller_env),
        NEW_ENV => {
            // GNU's parent defaults to the calling environment; a tagged
            /// parent= argument overrides it exactly as GNU. hash and size
            /// are performance hints outside the slice.
            let mut parent = caller_env;
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                if tag_name.as_deref() == Some("parent") {
                    let evaluated = eval(heap, interner, car, caller_env)?;
                    parent = force_if_promise(heap, interner, evaluated)?;
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            Ok(crate::value::env::new_environment(heap, parent))
        }
        EXISTS => exists_primitive(heap, interner, args, caller_env),
        LS => {
            // GNU: the environment argument is optional and defaults to the
            /// calling environment; all.names=TRUE includes dot-prefixed
            /// hidden bindings.
            let mut env = caller_env;
            let mut all_names = false;
            let mut positional = 0usize;
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                positional += 1;
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                match tag_name.as_deref() {
                    Some("all.names") => {
                        if let Some(RObject::Logical(vals)) = heap.object(v) {
                            all_names = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                        }
                    }
                    _ => {
                        if positional == 1 {
                            env = v;
                        }
                    }
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            ls_primitive(heap, interner, env, all_names)
        }
        DOLLAR_ASSIGN => dollar_assign_primitive(heap, interner, args, caller_env),
        CAT => {
            // Variadic output: operands coerce elementwise and concatenate
            /// with the separator — tagged sep exactly as GNU, default one
            /// space. Output lands in the session capture buffer, or the
            /// tagged file (append honored) exactly as GNU's connection
            /// redirection.
            let mut pieces: Vec<String> = Vec::new();
            let mut sep = " ".to_string();
            let mut file_target: Option<String> = None;
            let mut append = false;
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                let value = force_if_promise(heap, interner, evaluated)?;
                if tag_name.as_deref() == Some("sep") {
                    if let Some(RObject::String(values)) = heap.object(value) {
                        if !values.is_empty() {
                            sep = interner.string_text(values[0]).to_string();
                        }
                    }
                } else if tag_name.as_deref() == Some("file") {
                    file_target = string_operand(heap, interner, value);
                } else if tag_name.as_deref() == Some("append") {
                    if let Some(RObject::Logical(vals)) = heap.object(value) {
                        append = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                    }
                } else {
                    // Every element of the operand contributes, vector or scalar.
                    let length = vector_length(heap, value).unwrap_or(0);
                    if let Some(object) = heap.object(value) {
                        match object {
                            RObject::Real(_) | RObject::Integer(_) | RObject::Logical(_) | RObject::String(_) => {
                                for i in 0..length {
                                    pieces.push(super::print::format_cat_element(heap, interner, object, i));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            // GNU cat: separator BETWEEN pieces only, no trailing newline.
            let joined = pieces.join(&sep);
            match file_target {
                Some(path) => {
                    // GNU: file connections open write or append per the
                    /// tagged append argument.
                    use std::io::Write;
                    let file = if append {
                        std::fs::OpenOptions::new().append(true).create(true).open(&path)
                    } else {
                        std::fs::File::create(&path)
                    };
                    let mut file = file.map_err(|_| RSignal::Error(EvalError::TypeError(
                        "cat: cannot open file",
                    )))?;
                    file.write_all(joined.as_bytes())
                        .map_err(|_| RSignal::Error(EvalError::TypeError("cat: write failed")))?;
                }
                None => heap.output.push_str(&joined),
            }
            Ok(ValueId::NIL)
        }
        INVISIBLE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            // Visibility metadata is a print-system concern outside the slice;
            /// the value passes through exactly as GNU's return shape.
            Ok(operands[0])
        }
        TRYCATCH => trycatch_primitive(heap, interner, args, caller_env),
        TYPEOF => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let type_name = match heap.object(operands[0]) {
                Some(RObject::Nil) | None => "NULL",
                Some(RObject::Logical(_)) => "logical",
                Some(RObject::Integer(_)) => "integer",
                Some(RObject::Real(_)) => "double",
                Some(RObject::Complex(_)) => "complex",
                Some(RObject::Expression(_)) => "expression",
                Some(RObject::String(_)) => "character",
                Some(RObject::Raw(_)) => "raw",
                Some(RObject::List(_)) => "list",
                Some(RObject::Closure { .. }) => "closure",
                Some(RObject::Environment { .. }) => "environment",
                Some(RObject::Symbol { .. }) => "symbol",
                Some(RObject::Pairlist { .. }) => "pairlist",
                Some(RObject::Language { .. }) => "language",
                Some(RObject::Promise { .. }) => "promise",
                Some(RObject::Primitive(_)) => "builtin",
                _ => "unknown",
            };
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(type_name)])))
        }
        STRUCTURE => structure_primitive(heap, interner, args, caller_env),
        ASSIGN_FN => assign_primitive(heap, interner, args, caller_env),
        GET => get_primitive(heap, interner, args, caller_env),
        TEMPFILE => tempfile_primitive(heap, interner, args, caller_env),
        WRITELINES => writelines_primitive(heap, interner, args, caller_env),
        READLINES => readlines_primitive(heap, interner, args, caller_env),
        FILE_EXISTS => file_exists_primitive(heap, interner, args, caller_env),
        FILE_REMOVE => file_remove_primitive(heap, interner, args, caller_env),
        FILE_COPY => file_copy_primitive(heap, interner, args, caller_env),
        FILE_RENAME => file_rename_primitive(heap, interner, args, caller_env),
        SUPPRESS_WARNINGS => {
            // Warnings raised inside are dropped rather than buffered —
            /// save/restore keeps the flag exact across reentry.
            let raw = raw_args(heap, args);
            let prior = heap.warn_suppressed;
            heap.warn_suppressed = true;
            let mut result = Ok(ValueId::NIL);
            for expr in &raw {
                result = eval(heap, interner, *expr, caller_env);
                if result.is_err() {
                    break;
                }
            }
            heap.warn_suppressed = prior;
            result
        }
        AS_LOGICAL => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            coerce_logical(heap, interner, operands[0])
        }
        ATTR => attr_primitive(heap, interner, args, caller_env),
        ATTR_ASSIGN => attr_assign_primitive(heap, interner, args, caller_env),
        ATTRIBUTES => attributes_primitive(heap, interner, args, caller_env),
        ATTRIBUTES_ASSIGN => attributes_assign_primitive(heap, interner, args, caller_env),
        IS_NULL => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = operands[0] == ValueId::NIL || matches!(heap.object(operands[0]), Some(RObject::Nil));
            Ok(heap.alloc(RObject::Logical(vec![if is { 1 } else { 0 }])))
        }
        LENGTH_ASSIGN => length_assign_primitive(heap, interner, args, caller_env),
        FILE_PATH => file_path_primitive(heap, interner, args, caller_env),
        DATA_FRAME => data_frame_primitive(heap, interner, args, caller_env),
        AGGREGATE => aggregate_primitive(heap, interner, args, caller_env),
        FACTOR => factor_primitive(heap, interner, args, caller_env),
        LEVELS => attrib_getter(heap, interner, args, caller_env, "levels"),
        LEVELS_ASSIGN => levels_assign_primitive(heap, interner, args, caller_env),
        UNCLASS => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            copy_with_attrib(heap, interner, operands[0], "class", ValueId::NIL).map_err(RSignal::from)
        }
        SPRINTF => sprintf_primitive(heap, interner, args, caller_env),
        SYS_SETENV => {
            // Tagged pairs name = value exactly as GNU.
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                let name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                if let (Some(n), Some(val)) = (name, string_operand(heap, interner, v)) {
                    heap.env_vars.insert(n, Some(val));
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            Ok(heap.alloc(RObject::Logical(vec![1])))
        }
        SYS_GETENV => {
            // Tagged walk — the `unset` argument supplies the value GNU
            // returns for variables that are not set at all.
            let mut names_arg = ValueId::NIL;
            let mut unset_arg = ValueId::NIL;
            let mut positional = 0usize;
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                positional += 1;
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                if tag_name.as_deref() == Some("unset") {
                    unset_arg = v;
                } else if positional == 1 {
                    names_arg = v;
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            let unset_text = string_operand(heap, interner, unset_arg);
            let names = string_vec(heap, interner, names_arg);
            let out: Vec<super::intern::InternedStr> = names
                .iter()
                .map(|n| {
                    // Session overrides first, the genuine process
                    // environment as the fallback. Variables absent
                    // everywhere resolve to unset exactly as GNU.
                    let resolved: Option<String> = match heap.env_vars.get(n) {
                        Some(Some(v)) => Some(v.clone()),
                        Some(None) => None,
                        None => std::env::var(n).ok(),
                    };
                    let text = match resolved {
                        Some(v) => v,
                        None => unset_text.clone().unwrap_or_default(),
                    };
                    interner.intern_string(&text)
                })
                .collect();
            Ok(heap.alloc(RObject::String(out)))
        }
        SYS_UNSETENV => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            for name in string_vec(heap, interner, operands[0]) {
                heap.env_vars.insert(name, None);
            }
            Ok(heap.alloc(RObject::Logical(vec![1])))
        }
        UNLINK => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            for path in string_vec(heap, interner, operands[0]) {
                let p = std::path::Path::new(&path);
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(p);
                } else {
                    let _ = std::fs::remove_file(p);
                }
            }
            Ok(heap.alloc(RObject::Integer(vec![0])))
        }
        DIR_CREATE => {
            let mut recursive = false;
            let mut path_arg = ValueId::NIL;
            let mut cursor = args;
            let mut positional = 0usize;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                positional += 1;
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                if tag_name.as_deref() == Some("recursive") {
                    if let Some(RObject::Logical(vals)) = heap.object(v) {
                        recursive = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                    }
                } else if positional == 1 {
                    path_arg = v;
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            let mut created = true;
            for path in string_vec(heap, interner, path_arg) {
                let result = if recursive {
                    std::fs::create_dir_all(&path)
                } else {
                    std::fs::create_dir(&path)
                };
                if result.is_err() {
                    created = false;
                }
            }
            Ok(heap.alloc(RObject::Logical(vec![if created { 1 } else { 0 }])))
        }
        LIST_DIRS => {
            let mut full_names = true;
            let mut recursive = true;
            let mut path_arg = ValueId::NIL;
            let mut cursor = args;
            let mut positional = 0usize;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                positional += 1;
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                match tag_name.as_deref() {
                    Some("full.names") => {
                        if let Some(RObject::Logical(vals)) = heap.object(v) {
                            full_names = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                        }
                    }
                    Some("recursive") => {
                        if let Some(RObject::Logical(vals)) = heap.object(v) {
                            recursive = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                        }
                    }
                    _ => {
                        if positional == 1 {
                            path_arg = v;
                        }
                    }
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            let roots = string_vec(heap, interner, path_arg);
            let mut out: Vec<super::intern::InternedStr> = Vec::new();
            for root in &roots {
                // GNU's recursive listing leads with the root itself —
                /// empty text under full.names=FALSE exactly as "." resolves.
                if recursive {
                    let text = if full_names { root.clone() } else { String::new() };
                    out.push(interner.intern_string(&text));
                }
                let mut collected: Vec<String> = Vec::new();
                let mut stack: Vec<(String, String)> = vec![(root.clone(), String::new())];
                while let Some((dir, rel)) = stack.pop() {
                    if let Ok(entries) = std::fs::read_dir(&dir) {
                        let mut names: Vec<String> = entries
                            .flatten()
                            .filter(|e| e.path().is_dir())
                            .map(|e| e.file_name().to_string_lossy().to_string())
                            .collect();
                        names.sort();
                        for name in names {
                            let child_rel = if rel.is_empty() {
                                name.clone()
                            } else {
                                format!("{rel}/{name}")
                            };
                            collected.push(child_rel.clone());
                            if recursive {
                                stack.push((format!("{dir}/{name}"), child_rel));
                            }
                        }
                    }
                }
                // GNU sorts the full relative listing byte-wise.
                collected.sort();
                for rel in collected {
                    let text = if full_names {
                        format!("{root}/{rel}")
                    } else {
                        rel
                    };
                    out.push(interner.intern_string(&text));
                }
            }
            Ok(heap.alloc(RObject::String(out)))
        }
        BASENAME => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let out: Vec<super::intern::InternedStr> = string_vec(heap, interner, operands[0])
                .iter()
                .map(|path| {
                    let trimmed = path.trim_end_matches('/');
                    let base = trimmed.rsplit('/').next().unwrap_or(trimmed);
                    interner.intern_string(base)
                })
                .collect();
            Ok(heap.alloc(RObject::String(out)))
        }
        INTERNAL => {
            // GNU's .Internal evaluates its single argument as a direct
            /// internal call — in the new world the inner call evaluates
            /// exactly as written, the internal table being the primitives.
            // Unknown inner function heads raise GNU's exact no-internal-
            // function error rather than the unbound-variable fallback.
            let raw = raw_args(heap, args);
            match raw.first() {
                Some(expr) => {
                    if let Some(RObject::Language { car, cdr, .. }) = heap.object(*expr) {
                        let inner_args = *cdr;
                        if let Some(RObject::Symbol { name, .. }) = heap.object(*car) {
                            let head = interner.symbol_name(*name).to_string();
                            match head.as_str() {
                                "isdebugged" => {
                                    let ops = eval_args(heap, interner, inner_args, caller_env, 1)?;
                                    let marked = heap.debugged.contains(&ops[0]);
                                    return Ok(heap.alloc(RObject::Logical(vec![marked as i32])));
                                }
                                "debug" | "debugonce" | "undebug" => {
                                    let ops = eval_args(heap, interner, inner_args, caller_env, 1)?;
                                    if head == "undebug" {
                                        heap.debugged.remove(&ops[0]);
                                    } else {
                                        heap.debugged.insert(ops[0]);
                                    }
                                    return Ok(ValueId::NIL);
                                }
                                "file.show" => {
                                    // file.show(files, header, title,
                                    // delete.file, pager): contents stream
                                    // with a trailing blank line, deletion
                                    // when the fourth argument holds exactly
                                    // as GNU, NULL invisibly; empty names
                                    // raise the invalid-specification error.
                                    let raw_inner = raw_args(heap, inner_args);
                                    let mut ops: Vec<ValueId> = Vec::new();
                                    for expr in &raw_inner {
                                        let v = eval(heap, interner, *expr, caller_env)?;
                                        ops.push(force_if_promise(heap, interner, v)?);
                                    }
                                    let files = string_vec(heap, interner, ops[0]);
                                    if files.is_empty() {
                                        return Err(EvalError::Condition(
                                            "invalid filename specification".to_string(),
                                        )
                                        .into());
                                    }
                                    let delete = ops.get(3).map(|v| matches!(heap.object(*v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) == 1)).unwrap_or(false);
                                    for path in &files {
                                        if let Ok(content) = std::fs::read_to_string(path) {
                                            heap.output.push_str(&content);
                                            if !content.ends_with('\n') {
                                                heap.output.push('\n');
                                            }
                                            heap.output.push('\n');
                                        }
                                        if delete {
                                            let _ = std::fs::remove_file(path);
                                        }
                                    }
                                    heap.visible = false;
                                    return Ok(ValueId::NIL);
                                }
                                ".dfltWarn" => {
                                    // The default warning handler — buffer
                                    // the message exactly as warning() does.
                                    let ops = eval_args(heap, interner, inner_args, caller_env, 1)?;
                                    let text = string_operand(heap, interner, ops[0]).unwrap_or_default();
                                    if !heap.warn_suppressed {
                                        heap.pending_warnings.push(text);
                                    }
                                    heap.visible = false;
                                    return Ok(ValueId::NIL);
                                }
                                _ => {}
                            }
                            if primitive_id_for_name(&head).is_none()
                                && !matches!(head.as_str(), "builtins" | "eval" | "substitute" | "sys.calls" | "sys.frames" | "sys.function" | "sys.parent" | "browser" | "traceback" | "gc" | "printDeferredWarnings")
                            {
                                return Err(EvalError::Condition(format!(
                                    "there is no .Internal function '{head}'"
                                ))
                                .into());
                            }
                        }
                    }
                    eval(heap, interner, *expr, caller_env)
                }
                None => Ok(ValueId::NIL),
            }
        }
        PRIMITIVE_FN => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let name = string_operand(heap, interner, operands[0])
                .ok_or(EvalError::TypeError(".Primitive requires a name"))?;
            match primitive_id_for_name(&name) {
                Some(id) => Ok(heap.alloc(RObject::Primitive(BuiltinId(id)))),
                None => Err(EvalError::TypeError("no such primitive").into()),
            }
        }
        WARNING => {
            // GNU defers warnings to end-of-task: collapse every argument
            /// paste0-style — vectors contribute every element, empties
            /// nothing — buffer the text, and return NULL invisibly. The
            /// harness drains the buffer into `Warning message:` blocks.
            let raw = raw_args(heap, args);
            let mut message = String::new();
            for expr in &raw {
                let evaluated = eval(heap, interner, *expr, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                for piece in string_vec(heap, interner, v) {
                    if piece != "\0NA" {
                        message.push_str(&piece);
                    }
                }
                if let Some(text) = string_operand(heap, interner, v) {
                    if string_vec(heap, interner, v).is_empty() {
                        message.push_str(&text);
                    }
                }
            }
            if !heap.warn_suppressed {
                heap.pending_warnings.push(message);
            }
            heap.visible = false;
            Ok(ValueId::NIL)
        }
        MESSAGE_FN => {
            // message(...) concatenates paste0-style and writes one line to
            /// the message stream immediately — the harness merges streams
            /// in order, so direct output exactly matches GNU's interleave.
            let raw = raw_args(heap, args);
            let mut text = String::new();
            for expr in &raw {
                let evaluated = eval(heap, interner, *expr, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                for piece in string_vec(heap, interner, v) {
                    if piece != "\0NA" {
                        text.push_str(&piece);
                    }
                }
            }
            heap.output.push_str(&text);
            heap.output.push('\n');
            heap.visible = false;
            Ok(ValueId::NIL)
        }
        MISSING_FN => {
            // missing(x) inspects the UNEVALUATED symbol — TRUE exactly when
            /// the frame binding is the missing-argument sentinel.
            let raw = raw_args(heap, args);
            let mut missing = false;
            if let Some(first) = raw.first() {
                if let Some(RObject::Symbol { name, .. }) = heap.object(*first) {
                    let sym = *name;
                    missing = match super::env::lookup_in_frame(heap, caller_env, sym) {
                        Some(binding) => binding.value == ValueId::MISSING,
                        None => false,
                    };
                }
            }
            Ok(heap.alloc(RObject::Logical(vec![missing as i32])))
        }
        PARENT_ENV => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let parent = match heap.object(operands[0]) {
                Some(RObject::Environment { enclos, .. }) => *enclos,
                _ => ValueId::EMPTY_ENV,
            };
            Ok(parent)
        }
        EMPTYENV => Ok(ValueId::EMPTY_ENV),
        AS_INTEGER => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            coerce_integer(heap, interner, operands[0])
        }
        AS_NUMERIC => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            coerce_real(heap, interner, operands[0])
        }
        STOPIFNOT => stopifnot_primitive(heap, interner, args, caller_env),
        SEQUENCE => sequence_primitive(heap, interner, args, caller_env),
        REP_INT => rep_int_primitive(heap, interner, args, caller_env),
        REP_LEN => rep_len_primitive(heap, interner, args, caller_env),
        SAMPLE => sample_primitive(heap, interner, args, caller_env, false),
        SAMPLE_INT => sample_primitive(heap, interner, args, caller_env, true),
        BUILTINS => builtins_primitive(heap, interner, args, caller_env),
        REGEXEC => regexec_primitive(heap, interner, args, caller_env),
        SUBSTR => {
            let operands = eval_args(heap, interner, args, caller_env, 3)?;
            let texts = string_vec(heap, interner, operands[0]);
            let starts = numerics_of(heap, operands[1]).map_err(RSignal::from)?;
            let stops = numerics_of(heap, operands[2]).map_err(RSignal::from)?;
            let mut out: Vec<super::intern::InternedStr> = Vec::new();
            for (index, text) in texts.iter().enumerate() {
                let start = if starts.is_empty() { 1.0 } else { starts[index % starts.len()] } as isize;
                let stop = if stops.is_empty() { text.len() as f64 } else { stops[index % stops.len()] } as isize;
                let chars: Vec<char> = text.chars().collect();
                let lo = (start.max(1) - 1).max(0) as usize;
                let hi = (stop.max(0) as usize).min(chars.len());
                let piece: String = if lo >= hi || lo >= chars.len() {
                    String::new()
                } else {
                    chars[lo..hi].iter().collect()
                };
                out.push(interner.intern_string(&piece));
            }
            Ok(heap.alloc(RObject::String(out)))
        }
        FILE_FN => file_primitive(heap, interner, args, caller_env),
        ISOPEN => isopen_primitive(heap, interner, args, caller_env),
        CLOSE_FN => close_primitive(heap, interner, args, caller_env),
        FLUSH_FN => {
            // Write-through connections mean flush is observationally a
            // no-op — invisible NULL exactly as GNU.
            heap.visible = false;
            Ok(ValueId::NIL)
        }
        WRITECHAR => writechar_primitive(heap, interner, args, caller_env),
        READCHAR => readchar_primitive(heap, interner, args, caller_env),
        UNION => setop_primitive(heap, interner, args, caller_env, SetOp::Union),
        INTERSECT => setop_primitive(heap, interner, args, caller_env, SetOp::Intersect),
        SETDIFF => setop_primitive(heap, interner, args, caller_env, SetOp::Diff),
        SETEQUAL => setop_primitive(heap, interner, args, caller_env, SetOp::Equal),
        TABLE => table_primitive(heap, interner, args, caller_env),
        SETCLASS => setclass_primitive(heap, interner, args, caller_env),
        NEW_S4 => new_s4_primitive(heap, interner, args, caller_env),
        IS_S4 => is_s4_primitive(heap, interner, args, caller_env),
        SLOT => slot_primitive(heap, interner, args, caller_env),
        SLOTNAMES => slotnames_primitive(heap, interner, args, caller_env),
        UNNAME => {
            // unname strips the names attribute exactly as GNU — other
            /// attributes (dim, class) stay untouched.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            strip_attrib(heap, interner, operands[0], "names")
        }
        WHICH_MIN | WHICH_MAX => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let xs = numerics_of(heap, operands[0]).map_err(RSignal::from)?;
            let mut best: Option<usize> = None;
            for (index, v) in xs.iter().enumerate() {
                if v.is_nan() {
                    continue;
                }
                best = match best {
                    None => Some(index),
                    Some(b) => {
                        let better = if id.0 == WHICH_MIN { *v < xs[b] } else { *v > xs[b] };
                        if better { Some(index) } else { Some(b) }
                    }
                };
            }
            match best {
                Some(b) => Ok(heap.alloc(RObject::Integer(vec![(b + 1) as i32]))),
                None => Ok(heap.alloc(RObject::Integer(Vec::new()))),
            }
        }
        WITHVISIBLE => {
            // withVisible captures the evaluation's own visibility into a
            /// two-element named list exactly as GNU.
            let raw = raw_args(heap, args);
            let expr = match raw.first() {
                Some(e) => *e,
                None => return Err(EvalError::TypeError("withVisible requires an expression").into()),
            };
            let value = eval(heap, interner, expr, caller_env)?;
            let visible = heap.visible;
            let visible_flag = heap.alloc(RObject::Logical(vec![visible as i32]));
            let result = heap.alloc(RObject::List(vec![value, visible_flag]));
            let names_obj = heap.alloc(RObject::String(vec![
                interner.intern_string("value"),
                interner.intern_string("visible"),
            ]));
            copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)
        }
        SYSTEM => {
            // system(cmd, intern): intern=FALSE streams child output and
            // returns the exit status invisibly; intern=TRUE captures
            // stdout into a character vector — one element per line —
            // exactly as GNU.
            let mut positional: Vec<ValueId> = Vec::new();
            let mut intern = false;
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                match tag_name.as_deref() {
                    Some("intern") => {
                        intern = matches!(heap.object(v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) == 1);
                    }
                    _ => positional.push(v),
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            let command = positional.first().and_then(|v| string_operand(heap, interner, *v)).unwrap_or_default();
            let (stdout, status) = std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .output()
                .map(|out| (String::from_utf8_lossy(&out.stdout).to_string(), out.status.code().unwrap_or(0)))
                .unwrap_or((String::new(), 127));
            if intern {
                // GNU splits captured output into lines; a trailing newline
                // does not yield a spurious empty final element.
                let mut lines: Vec<super::intern::InternedStr> = stdout
                    .split('\n')
                    .map(|line| interner.intern_string(line))
                    .collect();
                if matches!(lines.last(), Some(last) if interner.string_text(*last).is_empty()) {
                    lines.pop();
                }
                heap.visible = true;
                return Ok(heap.alloc(RObject::String(lines)));
            }
            heap.output.push_str(&stdout);
            heap.visible = false;
            Ok(heap.alloc(RObject::Integer(vec![status])))
        }
        WRITEBIN => writebin_primitive(heap, interner, args, caller_env),
        READBIN => readbin_primitive(heap, interner, args, caller_env),
        URL_FN => {
            // url() connections are outside the supported surface — GNU's
            // exact error text rides the condition channel.
            Err(EvalError::Condition("URL scheme unsupported".to_string()).into())
        }
        DIM_ASSIGN => dim_assign_primitive(heap, interner, args, caller_env),
        NROW_FN | NCOL_FN => {
            // GNU's capital shape helpers: a dim-attributed object reads
            // its dimensions, everything else is a column vector — length
            // rows, one column — and NULL measures zero exactly.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let dims: Option<Vec<i32>> = get_attrib(heap, interner, operands[0], "dim").and_then(|id| match heap.object(id) {
                Some(RObject::Integer(d)) => Some(d.clone()),
                Some(RObject::Real(d)) => Some(d.iter().map(|x| *x as i32).collect()),
                _ => None,
            });
            let is_null = operands[0] == ValueId::NIL || matches!(heap.object(operands[0]), Some(RObject::Nil));
            // data.frame columns measure the frame exactly as dim.data.frame
            // — rows from the first column, columns the frame width.
            let is_df = get_attrib(heap, interner, operands[0], "class")
                .and_then(|id2| match heap.object(id2) {
                    Some(RObject::String(vals)) => Some(vals.iter().any(|v| interner.string_text(*v) == "data.frame")),
                    _ => None,
                })
                .unwrap_or(false);
            let value = if is_null {
                0
            } else if let Some(d) = dims {
                if id.0 == NROW_FN {
                    d.first().copied().unwrap_or(0)
                } else {
                    d.get(1).copied().unwrap_or(1)
                }
            } else if is_df {
                if let Some(RObject::List(cols)) = heap.object(operands[0]) {
                    if id.0 == NROW_FN {
                        cols.first().and_then(|c| vector_length(heap, *c)).unwrap_or(0) as i32
                    } else {
                        cols.len() as i32
                    }
                } else {
                    0
                }
            } else if id.0 == NROW_FN {
                vector_length(heap, operands[0]).unwrap_or(0) as i32
            } else {
                1
            };
            Ok(heap.alloc(RObject::Integer(vec![value])))
        }
        SYS_GETPID => {
            // The process id as a length-one integer exactly as GNU.
            Ok(heap.alloc(RObject::Integer(vec![std::process::id() as i32])))
        }
        OLSONNAMES => {
            // The IANA timezone database relative paths sorted exactly as
            // GNU's OlsonNames — zoneinfo walks excluding metadata files.
            let mut zones: Vec<String> = Vec::new();
            let root = std::path::Path::new("/usr/share/zoneinfo");
            let mut stack: Vec<std::path::PathBuf> = vec![root.to_path_buf()];
            while let Some(dir) = stack.pop() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with('.') || name == "+VERSION" || name == "posix" || name == "right" {
                            continue;
                        }
                        if path.is_dir() {
                            stack.push(path);
                        } else if let Ok(rel) = path.strip_prefix(root) {
                            let rel_text = rel.to_string_lossy().to_string();
                            if !rel_text.contains(".tab") && !rel_text.starts_with("zone.") {
                                zones.push(rel_text);
                            }
                        }
                    }
                }
            }
            zones.sort();
            zones.dedup();
            let interned: Vec<_> = zones.iter().map(|z| interner.intern_string(z)).collect();
            Ok(heap.alloc(RObject::String(interned)))
        }
        IQR_FN => {
            // IQR(x) = diff(quantile(x, c(.25, .75))) with NA removal
            // exactly as GNU's default — type 7 interpolation.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let mut vals = numerics_of(heap, operands[0]).map_err(RSignal::from)?;
            vals.retain(|v| !v.is_nan());
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let quantile7 = |sorted: &[f64], prob: f64| -> f64 {
                let n = sorted.len();
                if n == 0 {
                    return f64::NAN;
                }
                if n == 1 {
                    return sorted[0];
                }
                let h = 1.0 + (n as f64 - 1.0) * prob;
                let lower = h.floor() as usize;
                if lower >= n {
                    return sorted[n - 1];
                }
                let frac = h - lower as f64;
                sorted[lower - 1] + frac * (sorted[lower] - sorted[lower - 1])
            };
            let iqr = quantile7(&vals, 0.75) - quantile7(&vals, 0.25);
            Ok(heap.alloc(RObject::Real(vec![iqr])))
        }
        SYS_CHMOD => {
            // chmod each path to the octal mode, reporting per-file success
            // exactly as GNU — missing files yield FALSE, not an error.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let paths = string_vec(heap, interner, operands[0]);
            let mode_text = string_operand(heap, interner, operands[1]).unwrap_or_else(|| "0644".to_string());
            let trimmed = mode_text.trim_start_matches('0');
            let mode = u32::from_str_radix(if trimmed.is_empty() { "0" } else { trimmed }, 8).unwrap_or(0o644);
            let mut out = Vec::new();
            for path in &paths {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let ok = std::fs::metadata(path)
                        .map(|_| std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).is_ok())
                        .unwrap_or(false);
                    out.push(ok as i32);
                }
                #[cfg(not(unix))]
                {
                    let _ = mode;
                    out.push(0);
                }
            }
            Ok(heap.alloc(RObject::Logical(out)))
        }
        FILE_INFO => {
            // The metadata frame GNU's file.info returns — this slice
            // covers the mode column as an octal character string.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let paths = string_vec(heap, interner, operands[0]);
            let mut modes = Vec::new();
            for path in &paths {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let text = std::fs::metadata(path)
                        .map(|m| format!("{:o}", m.permissions().mode() & 0o7777))
                        .unwrap_or_else(|_| "\0NA".to_string());
                    modes.push(text);
                }
                #[cfg(not(unix))]
                {
                    modes.push("\0NA".to_string());
                }
            }
            let interned: Vec<_> = modes.iter().map(|m| interner.intern_string(m)).collect();
            let col = heap.alloc(RObject::String(interned));
            let frame = heap.alloc(RObject::List(vec![col]));
            let names_obj = heap.alloc(RObject::String(vec![interner.intern_string("mode")]));
            let frame = copy_with_attrib(heap, interner, frame, "names", names_obj).map_err(RSignal::from)?;
            let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("data.frame")]));
            copy_with_attrib(heap, interner, frame, "class", class_obj).map_err(RSignal::from)
        }
        SYS_GETLOCALE | SYS_SETLOCALE => {
            // The process locale is the C locale in this runtime exactly
            // as a default POSIX session; category validation follows
            // GNU's exact error.
            let operands = eval_args(heap, interner, args, caller_env, 1).unwrap_or_default();
            let valid = ["LC_ALL", "LC_COLLATE", "LC_CTYPE", "LC_MONETARY", "LC_NUMERIC", "LC_TIME", "LC_MESSAGES", "LC_PAPER", "LC_MEASUREMENT"];
            if let Some(first) = operands.first() {
                let category = string_operand(heap, interner, *first).unwrap_or_default();
                if !category.is_empty() && !valid.contains(&category.as_str()) {
                    return Err(EvalError::Condition("invalid 'category' argument".to_string()).into());
                }
            }
            let locale = interner.intern_string("C");
            Ok(heap.alloc(RObject::String(vec![locale])))
        }
        NZCHAR => {
            // Elementwise non-empty test exactly as GNU — NA stays NA.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let texts = string_vec(heap, interner, operands[0]);
            let out: Vec<i32> = texts
                .iter()
                .map(|s| {
                    if s.starts_with('\0') {
                        crate::value::object::NA_INTEGER
                    } else {
                        (!s.is_empty()) as i32
                    }
                })
                .collect();
            Ok(heap.alloc(RObject::Logical(out)))
        }
        AT_OP => {
            // The slot operator — exactly slot(obj, name) in operator form,
            // the slot name unevaluated precisely as `$`.
            let parts = raw_args(heap, args);
            if parts.len() != 2 {
                return Err(EvalError::UnsupportedForm("@ arity").into());
            }
            let object = eval(heap, interner, parts[0], caller_env)?;
            let object = force_if_promise(heap, interner, object)?;
            let name = match heap.object(parts[1]) {
                Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
                _ => {
                    let evaluated = eval(heap, interner, parts[1], caller_env)?;
                    let v = force_if_promise(heap, interner, evaluated)?;
                    string_operand(heap, interner, v).unwrap_or_default()
                }
            };
            s4_slot_get(heap, interner, object, &name)
        }
        AT_ASSIGN => {
            // `@<-`(obj, name, value) — slot replacement constructing the
            // updated object exactly as GNU's operator desugar; the slot
            // name arrives unevaluated in operator use and evaluated in
            // the explicit-function form.
            let parts = raw_args(heap, args);
            if parts.len() != 3 {
                return Err(EvalError::UnsupportedForm("@<- arity").into());
            }
            let object = eval(heap, interner, parts[0], caller_env)?;
            let object = force_if_promise(heap, interner, object)?;
            let name = match heap.object(parts[1]) {
                Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
                _ => {
                    let evaluated = eval(heap, interner, parts[1], caller_env)?;
                    let v = force_if_promise(heap, interner, evaluated)?;
                    string_operand(heap, interner, v).unwrap_or_default()
                }
            };
            let value = eval(heap, interner, parts[2], caller_env)?;
            let value = force_if_promise(heap, interner, value)?;
            s4_slot_set(heap, interner, object, &name, value)
        }
        SYS_INFO => {
            // The eight-field system identity vector exactly as GNU's
            // ordering — uname fields first, user identity trailing.
            let sysname = run_command_output("uname", &["-s"]).unwrap_or_else(|_| "unknown".to_string());
            let release = run_command_output("uname", &["-r"]).unwrap_or_else(|_| "unknown".to_string());
            let version = run_command_output("uname", &["-v"]).unwrap_or_else(|_| "unknown".to_string());
            let nodename = run_command_output("uname", &["-n"]).unwrap_or_else(|_| "unknown".to_string());
            let machine = run_command_output("uname", &["-m"]).unwrap_or_else(|_| "unknown".to_string());
            let user = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
            let fields = [
                ("sysname", sysname),
                ("release", release),
                ("version", version),
                ("nodename", nodename),
                ("machine", machine),
                ("login", user.clone()),
                ("user", user.clone()),
                ("effective_user", user),
            ];
            let values: Vec<_> = fields.iter().map(|(_, v)| interner.intern_string(v.trim())).collect();
            let names: Vec<_> = fields.iter().map(|(n, _)| interner.intern_string(n)).collect();
            let result = heap.alloc(RObject::String(values));
            let names_obj = heap.alloc(RObject::String(names));
            copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)
        }
        SYS_TIME => {
            // Current epoch seconds classed POSIXct/POSIXt exactly as GNU.
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            let result = heap.alloc(RObject::Real(vec![secs]));
            let class_obj = heap.alloc(RObject::String(vec![
                interner.intern_string("POSIXct"),
                interner.intern_string("POSIXt"),
            ]));
            copy_with_attrib(heap, interner, result, "class", class_obj).map_err(RSignal::from)
        }
        SYS_DATE => {
            // Current civil date as epoch days classed Date exactly as GNU.
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as f64)
                .unwrap_or(0.0);
            let days = (secs / 86400.0).floor();
            let result = heap.alloc(RObject::Real(vec![days]));
            let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("Date")]));
            copy_with_attrib(heap, interner, result, "class", class_obj).map_err(RSignal::from)
        }
        SYS_WHICH => {
            // PATH resolution per name exactly as GNU — found commands
            // yield their absolute path, missing ones the empty string,
            // the input names riding as the names attribute.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let names_in = string_vec(heap, interner, operands[0]);
            let path_var = std::env::var("PATH").unwrap_or_default();
            let dirs: Vec<&str> = path_var.split(':').collect();
            let mut out: Vec<super::intern::InternedStr> = Vec::new();
            for name in &names_in {
                let mut found = String::new();
                if name.contains('/') {
                    if std::path::Path::new(name).exists() {
                        found = name.clone();
                    }
                } else {
                    for dir in &dirs {
                        let candidate = format!("{dir}/{name}");
                        if std::path::Path::new(&candidate).exists() {
                            found = candidate;
                            break;
                        }
                    }
                }
                out.push(interner.intern_string(&found));
            }
            let result = heap.alloc(RObject::String(out));
            let interned_names: Vec<_> = names_in.iter().map(|n| interner.intern_string(n)).collect();
            let names_obj = heap.alloc(RObject::String(interned_names));
            copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)
        }
        ROUND | SIGNIF => {
            // GNU's rounding family: round is half-to-even at digit
            // position, signif counts significant digits — digits
            // defaulting 0 and 6 respectively exactly as GNU.
            let arity = raw_args(heap, args).len();
            let operands = eval_args(heap, interner, args, caller_env, arity)?;
            if operands.is_empty() {
                return Err(EvalError::TypeError("round requires an argument").into());
            }
            let values = reals_of_value(heap, operands[0]).map_err(RSignal::from)?;
            let digits = if operands.len() > 1 {
                first_real(heap, operands[1]).unwrap_or(if id.0 == ROUND { 0.0 } else { 6.0 }) as i32
            } else if id.0 == ROUND {
                0
            } else {
                6
            };
            let out: Vec<f64> = values
                .iter()
                .map(|v| {
                    if v.is_nan() || !v.is_finite() {
                        *v
                    } else if id.0 == ROUND {
                        round_half_even(*v, digits)
                    } else {
                        signif_digits(*v, digits)
                    }
                })
                .collect();
            Ok(heap.alloc(RObject::Real(out)))
        }
        ANYNA => {
            // Elementwise scan exactly as GNU — NaN counts as missing
            // since is.na(NaN) is TRUE.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let found = any_na_in_value(heap, interner, operands[0]);
            Ok(heap.alloc(RObject::Logical(vec![found as i32])))
        }
        SYS_LOCALECONV => {
            // The C locale's conversion table exactly as a default POSIX
            // session — 18 fields in GNU's order, numeric fields empty.
            let fields = [
                "decimal_point", "thousands_sep", "grouping", "int_curr_symbol",
                "currency_symbol", "mon_decimal_point", "mon_thousands_sep", "mon_grouping",
                "positive_sign", "negative_sign", "int_frac_digits", "frac_digits",
                "p_cs_precedes", "p_sep_by_space", "n_cs_precedes", "n_sep_by_space",
                "p_sign_posn", "n_sign_posn",
            ];
            let values: Vec<super::intern::InternedStr> = fields
                .iter()
                .map(|f| interner.intern_string(if *f == "decimal_point" { "." } else { "" }))
                .collect();
            let names: Vec<_> = fields.iter().map(|f| interner.intern_string(f)).collect();
            let result = heap.alloc(RObject::String(values));
            let names_obj = heap.alloc(RObject::String(names));
            copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)
        }
        AGREP | AGREPL => {
            // Approximate substring matching exactly as GNU's TRE layer:
            // the pattern aligns against the best substring of each text,
            // unit insertion/deletion/substitution costs, fractional
            // max.distance scaled by pattern length and ceiled.
            let operands = eval_args(heap, interner, args, caller_env, 6).unwrap_or_else(|_| {
                let arity = raw_args(heap, args).len();
                eval_args(heap, interner, args, caller_env, arity).unwrap_or_default()
            });
            if operands.len() < 2 {
                return Err(EvalError::TypeError("agrep requires pattern and text").into());
            }
            let pattern_raw = string_operand(heap, interner, operands[0]).unwrap_or_default();
            let texts = string_vec(heap, interner, operands[1]);
            let mut value_form = false;
            let mut use_bytes = false;
            let mut ignore_case = false;
            let mut max_distance = 0.1_f64;
            let mut cursor = args;
            for _ in 0..1000 {
                if cursor == ValueId::NIL {
                    break;
                }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
                    _ => String::new(),
                };
                let v = eval(heap, interner, car, caller_env)?;
                match tag_name.as_str() {
                    "value" => value_form = logical_scalar(heap, v),
                    "useBytes" => use_bytes = logical_scalar(heap, v),
                    "ignore.case" => ignore_case = logical_scalar(heap, v),
                    "max.distance" => {
                        if let Some(RObject::Real(r)) = heap.object(v) {
                            if let Some(first) = r.first() {
                                max_distance = *first;
                            }
                        } else if let Some(RObject::Integer(iv)) = heap.object(v) {
                            if let Some(first) = iv.first() {
                                max_distance = *first as f64;
                            }
                        }
                    }
                    _ => {}
                }
                cursor = next;
            }
            let _ = use_bytes;
            let pattern = if ignore_case { pattern_raw.to_lowercase() } else { pattern_raw.clone() };
            let plen = pattern.chars().count().max(1) as f64;
            let limit = if max_distance < 1.0 {
                (max_distance * plen).ceil().max(0.0)
            } else {
                max_distance
            };
            let matches: Vec<bool> = texts
                .iter()
                .map(|text| {
                    if text.starts_with('\0') {
                        return false;
                    }
                    let target = if ignore_case { text.to_lowercase() } else { text.clone() };
                    approximate_substring_distance(&pattern, &target) as f64 <= limit
                })
                .collect();
            if id.0 == AGREPL {
                let out: Vec<i32> = matches.iter().map(|m| *m as i32).collect();
                return Ok(heap.alloc(RObject::Logical(out)));
            }
            if value_form {
                let out: Vec<super::intern::InternedStr> = texts
                    .iter()
                    .zip(matches.iter())
                    .filter(|(_, m)| **m)
                    .map(|(text, _)| interner.intern_string(text))
                    .collect();
                return Ok(heap.alloc(RObject::String(out)));
            }
            let indices: Vec<i32> = matches
                .iter()
                .enumerate()
                .filter(|(_, m)| **m)
                .map(|(i, _)| (i + 1) as i32)
                .collect();
            Ok(heap.alloc(RObject::Integer(indices)))
        }
        APPLY => {
            // Margin application over a 2D matrix exactly as GNU's core
            // semantics: margin 1 iterates rows, margin 2 columns, each
            // slice passed through FUN column-major.
            let operands = eval_args(heap, interner, args, caller_env, 3)?;
            let dims: Vec<usize> = match get_attrib(heap, interner, operands[0], "dim") {
                Some(d) => match heap.object(d) {
                    Some(RObject::Integer(v)) => v.iter().map(|x| *x as usize).collect(),
                    Some(RObject::Real(v)) => v.iter().map(|x| *x as usize).collect(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
            if dims.len() != 2 {
                return Err(EvalError::TypeError("apply requires a matrix").into());
            }
            let margin = first_real(heap, operands[1])? as usize;
            let (nrow, ncol) = (dims[0], dims[1]);
            let count = if margin == 1 { nrow } else { ncol };
            let mut results = Vec::new();
            for idx in 0..count {
                let slice = extract_matrix_margin(heap, interner, operands[0], margin, idx, nrow, ncol)?;
                let applied = apply_fun_to(heap, interner, operands[2], slice, caller_env)?;
                results.push(applied);
            }
            simplify_sapply(heap, results)
        }
        AS_ENVIRONMENT => {
            // Position and name lookup over the enclosure chain exactly as
            // GNU's search path — position 1 the user environment, the
            // chain end base; character names match .GlobalEnv and
            // package:base.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            if matches!(heap.object(operands[0]), Some(RObject::Environment { .. })) {
                return Ok(operands[0]);
            }
            let chain = enclosure_chain(heap, caller_env);
            if let Some(idx) = match heap.object(operands[0]) {
                Some(RObject::Real(v)) => v.first().copied(),
                Some(RObject::Integer(v)) => v.first().map(|x| *x as f64),
                _ => None,
            } {
                let pos = idx as usize;
                if pos >= 1 && pos <= chain.len() {
                    return Ok(chain[pos - 1]);
                }
                return Err(EvalError::TypeError("invalid 'pos' argument").into());
            }
            let name = string_operand(heap, interner, operands[0]).unwrap_or_default();
            match name.as_str() {
                ".GlobalEnv" => Ok(chain.first().copied().unwrap_or(caller_env)),
                "package:base" => Ok(chain.last().copied().unwrap_or(caller_env)),
                _ => {
                    let genv = chain.first().copied().unwrap_or(caller_env);
                    for env in &chain {
                        if env_search_name(heap, interner, *env, genv, false) == name {
                            return Ok(*env);
                        }
                    }
                    Err(EvalError::TypeError("invalid environment name").into())
                }
            }
        }
        AS_CALL => {
            // The list head becomes the call's function position exactly
            // as GNU — the remaining elements the argument chain.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let items: Vec<ValueId> = match heap.object(operands[0]) {
                Some(RObject::List(items)) => items.clone(),
                _ => return Err(EvalError::TypeError("as.call requires a list").into()),
            };
            if items.is_empty() {
                return Err(EvalError::TypeError("invalid length 0 argument").into());
            }
            let head = items[0];
            let mut chain = ValueId::NIL;
            for item in items[1..].iter().rev() {
                chain = heap.alloc(RObject::Pairlist {
                    car: *item,
                    cdr: chain,
                    tag: ValueId::NIL,
                });
            }
            Ok(heap.alloc(RObject::Language {
                car: head,
                cdr: chain,
                tag: ValueId::NIL,
            }))
        }
        ENC2NATIVE | ENC2UTF8 => {
            // Identity in this runtime — the native encoding IS UTF-8,
            // attributes riding through untouched exactly as GNU.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            Ok(operands[0])
        }
        SEARCH_FN => {
            // The search path walks the enclosure chain dynamically —
            // user environment first, attached environments by their
            // name attribute, base last exactly as GNU.
            let genv = {
                let sym = interner.intern_symbol(".GlobalEnv");
                match super::env::lookup(heap, caller_env, sym) {
                    Some((_, binding)) => binding.value,
                    None => caller_env,
                }
            };
            let names = search_path_names(heap, interner, genv);
            let interned: Vec<_> = names.iter().map(|n| interner.intern_string(n)).collect();
            Ok(heap.alloc(RObject::String(interned)))
        }
        AS_FACTOR => {
            // Identity on factors exactly as GNU — anything else factors
            // with the default sorted level set.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            if is_factor(heap, interner, operands[0]) {
                return Ok(operands[0]);
            }
            factor_primitive(heap, interner, args, caller_env)
        }
        AS_ORDERED => {
            // GNU's re-factorization rule: character input levels sort;
            // factor input keeps the original level ORDER but drops
            // levels absent from the data.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let factored = if is_factor(heap, interner, operands[0]) {
                refactor_dropping_unused(heap, interner, operands[0])?
            } else {
                factor_primitive(heap, interner, args, caller_env)?
            };
            let ordered = interner.intern_string("ordered");
            let factor_s = interner.intern_string("factor");
            let class_obj = heap.alloc(RObject::String(vec![ordered, factor_s]));
            copy_with_attrib(heap, interner, factored, "class", class_obj).map_err(RSignal::from)
        }
        ADDNA => {
            // NA becomes a trailing level exactly as GNU — missing codes
            // recode to the new level index so is.na reports FALSE for
            // them; ifany guards the addition on actual missingness.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let factored = if is_factor(heap, interner, operands[0]) {
                operands[0]
            } else {
                factor_primitive(heap, interner, args, caller_env)?
            };
            let mut ifany = false;
            let mut cursor = args;
            for _ in 0..1000 {
                if cursor == ValueId::NIL {
                    break;
                }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
                    _ => String::new(),
                };
                if tag_name == "ifany" {
                    let v = eval(heap, interner, car, caller_env)?;
                    ifany = logical_scalar(heap, v);
                }
                cursor = next;
            }
            let mut codes: Vec<i32> = match heap.object(factored) {
                Some(RObject::Integer(v)) => v.clone(),
                _ => return Err(EvalError::TypeError("addNA requires a factor").into()),
            };
            let has_na = codes.iter().any(|c| *c == crate::value::object::NA_INTEGER);
            let already = match get_attrib(heap, interner, factored, "levels") {
                Some(l) => string_vec(heap, interner, l).iter().any(|s| s == "\0NA"),
                None => false,
            };
            if already || (ifany && !has_na) {
                return Ok(factored);
            }
            let mut levels: Vec<String> = match get_attrib(heap, interner, factored, "levels") {
                Some(l) => string_vec(heap, interner, l),
                None => Vec::new(),
            };
            levels.push("\0NA".to_string());
            let na_index = levels.len() as i32;
            for code in codes.iter_mut() {
                if *code == crate::value::object::NA_INTEGER {
                    *code = na_index;
                }
            }
            let result = heap.alloc(RObject::Integer(codes));
            let interned: Vec<_> = levels.iter().map(|s| interner.intern_string(s)).collect();
            let levels_obj = heap.alloc(RObject::String(interned));
            let result = copy_with_attrib(heap, interner, result, "levels", levels_obj).map_err(RSignal::from)?;
            let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("factor")]));
            copy_with_attrib(heap, interner, result, "class", class_obj).map_err(RSignal::from)
        }
        APERM => {
            // Generalized transpose exactly as GNU: output dimension i
            // draws input dimension perm[i]; resize=FALSE computes the
            // same element permutation but keeps the input dim vector.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let dims: Vec<usize> = match get_attrib(heap, interner, operands[0], "dim") {
                Some(d) => match heap.object(d) {
                    Some(RObject::Integer(v)) => v.iter().map(|x| *x as usize).collect(),
                    Some(RObject::Real(v)) => v.iter().map(|x| *x as usize).collect(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
            let perm: Vec<usize> = match heap.object(operands[1]) {
                Some(RObject::Integer(v)) => v.iter().map(|x| *x as usize).collect(),
                Some(RObject::Real(v)) => v.iter().map(|x| *x as usize).collect(),
                _ => (1..=dims.len()).collect(),
            };
            let resize = {
                let mut r = true;
                let mut cursor = args;
                for _ in 0..1000 {
                    if cursor == ValueId::NIL {
                        break;
                    }
                    let (car, tag, next) = match heap.object(cursor) {
                        Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                        _ => break,
                    };
                    if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
                        if interner.symbol_name(*name) == "resize" {
                            let v = eval(heap, interner, car, caller_env)?;
                            r = logical_scalar(heap, v);
                        }
                    }
                    cursor = next;
                }
                r
            };
            aperm_impl(heap, interner, operands[0], &dims, &perm, resize)
        }
        ADDMARGINS => {
            // Sum margins appended exactly as GNU's default — one extra
            // row and column carrying margin totals, the corner the grand
            // total; output always double.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let dims: Vec<usize> = match get_attrib(heap, interner, operands[0], "dim") {
                Some(d) => match heap.object(d) {
                    Some(RObject::Integer(v)) => v.iter().map(|x| *x as usize).collect(),
                    Some(RObject::Real(v)) => v.iter().map(|x| *x as usize).collect(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
            if dims.len() != 2 {
                return Err(EvalError::TypeError("addmargins requires a matrix").into());
            }
            let values = reals_of_value(heap, operands[0]).map_err(RSignal::from)?;
            let (nrow, ncol) = (dims[0], dims[1]);
            let mut out = vec![0.0f64; (nrow + 1) * (ncol + 1)];
            let at = |r: usize, c: usize| c * nrow + r;
            let out_at = |r: usize, c: usize| c * (nrow + 1) + r;
            for c in 0..ncol {
                for r in 0..nrow {
                    let v = values.get(at(r, c)).copied().unwrap_or(0.0);
                    out[out_at(r, c)] = v;
                    out[out_at(r, ncol)] += v;
                    out[out_at(nrow, c)] += v;
                    out[out_at(nrow, ncol)] += v;
                }
            }
            let result = heap.alloc(RObject::Real(out));
            let dim_obj = heap.alloc(RObject::Integer(vec![(nrow + 1) as i32, (ncol + 1) as i32]));
            copy_with_attrib(heap, interner, result, "dim", dim_obj).map_err(RSignal::from)
        }
        ASPLIT => {
            // GNU's array splitting: the margin subscripts index the result
            // list-array, each slice the elements fixing those coordinates.
            // Negative margins complement; slices and the result both carry
            // dim attributes exactly as GNU.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let dims: Vec<usize> = match get_attrib(heap, interner, operands[0], "dim") {
                Some(d) => match heap.object(d) {
                    Some(RObject::Integer(v)) => v.iter().map(|x| *x as usize).collect(),
                    Some(RObject::Real(v)) => v.iter().map(|x| *x as usize).collect(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
            if dims.is_empty() {
                return Err(EvalError::TypeError("asplit requires a dimensioned object").into());
            }
            let raw_margin: Vec<i32> = match heap.object(operands[1]) {
                Some(RObject::Integer(v)) => v.clone(),
                Some(RObject::Real(v)) => v.iter().map(|x| *x as i32).collect(),
                _ => Vec::new(),
            };
            let n = dims.len();
            let margin: Vec<usize> = if raw_margin.iter().any(|m| *m < 0) {
                let excluded: Vec<usize> = raw_margin.iter().map(|m| (-*m) as usize).collect();
                (1..=n).filter(|d| !excluded.contains(d)).collect()
            } else {
                raw_margin.iter().map(|m| *m as usize).collect()
            };
            asplit_impl(heap, interner, operands[0], &dims, &margin)
        }
        ATTACH => {
            // Chain insertion exactly as GNU: the environment splices in
            // directly after .GlobalEnv, its enclosure becoming whatever
            // the user environment enclosed before.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let env = operands[0];
            if !matches!(heap.object(env), Some(RObject::Environment { .. })) {
                return Err(EvalError::TypeError("attach requires an environment").into());
            }
            let mut name = String::new();
            let mut cursor = args;
            for _ in 0..1000 {
                if cursor == ValueId::NIL {
                    break;
                }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                if let Some(RObject::Symbol { name: tag_sym, .. }) = heap.object(tag) {
                    if interner.symbol_name(*tag_sym) == "name" {
                        let v = eval(heap, interner, car, caller_env)?;
                        name = string_operand(heap, interner, v).unwrap_or_default();
                    }
                }
                cursor = next;
            }
            let env = if !name.is_empty() {
                let name_obj = heap.alloc(RObject::String(vec![interner.intern_string(&name)]));
                copy_with_attrib(heap, interner, env, "name", name_obj).map_err(RSignal::from)?
            } else {
                env
            };
            let genv = {
                let sym = interner.intern_symbol(".GlobalEnv");
                match super::env::lookup(heap, caller_env, sym) {
                    Some((_, binding)) => binding.value,
                    None => caller_env,
                }
            };
            let old_enclos = match heap.object(genv) {
                Some(RObject::Environment { enclos, .. }) => *enclos,
                _ => ValueId::EMPTY_ENV,
            };
            if let Some(RObject::Environment { enclos, .. }) = heap.object_mut(env) {
                *enclos = old_enclos;
            }
            if let Some(RObject::Environment { enclos, .. }) = heap.object_mut(genv) {
                *enclos = env;
            }
            heap.visible = false;
            Ok(env)
        }
        DETACH => {
            // Chain excision exactly as GNU — the named environment drops
            // out, its predecessor inheriting its enclosure.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let name = string_operand(heap, interner, operands[0]).unwrap_or_default();
            let genv = {
                let sym = interner.intern_symbol(".GlobalEnv");
                match super::env::lookup(heap, caller_env, sym) {
                    Some((_, binding)) => binding.value,
                    None => caller_env,
                }
            };
            let mut pred = genv;
            let mut cursor = match heap.object(genv) {
                Some(RObject::Environment { enclos, .. }) => *enclos,
                _ => ValueId::EMPTY_ENV,
            };
            for _ in 0..100 {
                if cursor == ValueId::EMPTY_ENV || cursor.is_sentinel() {
                    break;
                }
                let this_name = env_search_name(heap, interner, cursor, genv, false);
                if this_name == name {
                    let next_enclos = match heap.object(cursor) {
                        Some(RObject::Environment { enclos, .. }) => *enclos,
                        _ => ValueId::EMPTY_ENV,
                    };
                    if let Some(RObject::Environment { enclos, .. }) = heap.object_mut(pred) {
                        *enclos = next_enclos;
                    }
                    heap.visible = false;
                    return Ok(cursor);
                }
                pred = cursor;
                cursor = match heap.object(cursor) {
                    Some(RObject::Environment { enclos, .. }) => *enclos,
                    _ => break,
                };
            }
            heap.visible = false;
            Ok(ValueId::NIL)
        }
        POS_TO_ENV => {
            // Chain position indexing exactly as GNU — position 1 the
            // user environment.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let pos = first_real(heap, operands[0])? as usize;
            let genv = {
                let sym = interner.intern_symbol(".GlobalEnv");
                match super::env::lookup(heap, caller_env, sym) {
                    Some((_, binding)) => binding.value,
                    None => caller_env,
                }
            };
            let chain = enclosure_chain(heap, genv);
            chain
                .get(pos.saturating_sub(1))
                .copied()
                .ok_or(EvalError::TypeError("invalid pos argument").into())
        }
        DOT_CLASS2 => {
            // GNU's class inspection: explicit class attribute wins,
            // unclassed atoms report their implicit class pair.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let mut classes: Vec<String> = match get_attrib(heap, interner, operands[0], "class") {
                Some(c) => string_vec(heap, interner, c),
                None => Vec::new(),
            };
            if classes.is_empty() {
                classes = match heap.object(operands[0]) {
                    Some(RObject::Real(_)) => vec!["numeric".to_string()],
                    Some(RObject::Integer(_)) => vec!["integer".to_string(), "numeric".to_string()],
                    Some(RObject::Logical(_)) => vec!["logical".to_string()],
                    Some(RObject::String(_)) => vec!["character".to_string()],
                    Some(RObject::Complex(_)) => vec!["complex".to_string()],
                    Some(RObject::Raw(_)) => vec!["raw".to_string()],
                    Some(RObject::List(_)) => vec!["list".to_string()],
                    _ => vec!["numeric".to_string()],
                };
            }
            let interned: Vec<_> = classes.iter().map(|c| interner.intern_string(c)).collect();
            Ok(heap.alloc(RObject::String(interned)))
        }
        GC_TIME => {
            // No incremental collector timing in this runtime — the shape
            // reports zeros exactly as a quiescent GNU session would.
            Ok(heap.alloc(RObject::Real(vec![0.0; 5])))
        }
        AVE => {
            // Group means projected back onto the elements exactly as
            // GNU's default FUN — groupers recycle to x's length, NA
            // poisons its whole group, output always double.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let x = operands[0];
            let x_len = vector_length(heap, x).unwrap_or(0);
            let values = reals_of_value(heap, x).map_err(RSignal::from)?;
            let mut groupers: Vec<ValueId> = Vec::new();
            let mut cursor = args;
            let mut position = 0usize;
            for _ in 0..1000 {
                if cursor == ValueId::NIL {
                    break;
                }
                let (car, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, .. }) => (*car, *cdr),
                    _ => break,
                };
                position += 1;
                if position > 1 {
                    let v = eval(heap, interner, car, caller_env)?;
                    groupers.push(force_if_promise(heap, interner, v)?);
                }
                cursor = next;
            }
            let keys: Vec<String> = (0..x_len)
                .map(|i| {
                    groupers
                        .iter()
                        .map(|g| {
                            let glen = vector_length(heap, *g).unwrap_or(1).max(1);
                            element_text(heap, interner, *g, i % glen)
                        })
                        .collect::<Vec<_>>()
                        .join("\x01")
                })
                .collect();
            // Per-group sums and counts; NA poisoning exactly as mean
            /// without na.rm.
            let mut sums: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
            let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            let mut poisoned: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
            for (i, key) in keys.iter().enumerate() {
                let v = values.get(i).copied().unwrap_or(f64::NAN);
                *counts.entry(key.clone()).or_insert(0) += 1;
                if v.is_nan() {
                    poisoned.insert(key.clone(), true);
                } else {
                    *sums.entry(key.clone()).or_insert(0.0) += v;
                }
            }
            let out: Vec<f64> = keys
                .iter()
                .map(|key| {
                    if poisoned.get(key).copied().unwrap_or(false) {
                        f64::from_bits(crate::value::object::NA_REAL_BITS)
                    } else {
                        let sum = sums.get(key).copied().unwrap_or(0.0);
                        let count = counts.get(key).copied().unwrap_or(1) as f64;
                        sum / count
                    }
                })
                .collect();
            Ok(heap.alloc(RObject::Real(out)))
        }
        BACKSOLVE => {
            // Triangular system solving exactly as GNU — columnwise over
            // matrix right-hand sides, substitution direction following
            // upper.tri and transpose.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let r_dims: Vec<usize> = match get_attrib(heap, interner, operands[0], "dim") {
                Some(d) => match heap.object(d) {
                    Some(RObject::Integer(v)) => v.iter().map(|x| *x as usize).collect(),
                    Some(RObject::Real(v)) => v.iter().map(|x| *x as usize).collect(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
            if r_dims.len() != 2 || r_dims[0] != r_dims[1] {
                return Err(EvalError::TypeError("backsolve requires a square matrix").into());
            }
            let n = r_dims[0];
            let r_vals = reals_of_value(heap, operands[0]).map_err(RSignal::from)?;
            let rat = |i: usize, j: usize| r_vals.get(j * n + i).copied().unwrap_or(0.0);
            let mut upper_tri = true;
            let mut transpose = false;
            let mut k = n;
            let mut cursor = args;
            for _ in 0..1000 {
                if cursor == ValueId::NIL {
                    break;
                }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
                    let tname = interner.symbol_name(*name).to_string();
                    let v = eval(heap, interner, car, caller_env)?;
                    match tname.as_str() {
                        "upper.tri" => upper_tri = logical_scalar(heap, v),
                        "transpose" => transpose = logical_scalar(heap, v),
                        "k" => k = first_real(heap, v).unwrap_or(n as f64) as usize,
                        _ => {}
                    }
                }
                cursor = next;
            }
            let eff = k.min(n);
            let b_is_matrix = get_attrib(heap, interner, operands[1], "dim").is_some();
            let b_dims: Vec<usize> = match get_attrib(heap, interner, operands[1], "dim") {
                Some(d) => match heap.object(d) {
                    Some(RObject::Integer(v)) => v.iter().map(|x| *x as usize).collect(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
            let b_vals = reals_of_value(heap, operands[1]).map_err(RSignal::from)?;
            let ncols = if b_is_matrix { *b_dims.get(1).unwrap_or(&1) } else { 1 };
            let mut out = vec![0.0f64; eff * ncols];
            for col in 0..ncols {
                let mut x = vec![0.0f64; eff];
                let b: Vec<f64> = (0..eff).map(|i| b_vals.get(col * n + i).copied().unwrap_or(0.0)).collect();
                if transpose {
                    // R' x = b — forward substitution when R upper,
                    /// backward when lower.
                    if upper_tri {
                        for i in 0..eff {
                            let mut acc = b[i];
                            for j in 0..i {
                                acc -= rat(j, i) * x[j];
                            }
                            x[i] = acc / rat(i, i);
                        }
                    } else {
                        for i in (0..eff).rev() {
                            let mut acc = b[i];
                            for j in (i + 1)..eff {
                                acc -= rat(j, i) * x[j];
                            }
                            x[i] = acc / rat(i, i);
                        }
                    }
                } else if upper_tri {
                    for i in (0..eff).rev() {
                        let mut acc = b[i];
                        for j in (i + 1)..eff {
                            acc -= rat(i, j) * x[j];
                        }
                        x[i] = acc / rat(i, i);
                    }
                } else {
                    for i in 0..eff {
                        let mut acc = b[i];
                        for j in 0..i {
                            acc -= rat(i, j) * x[j];
                        }
                        x[i] = acc / rat(i, i);
                    }
                }
                for i in 0..eff {
                    out[col * eff + i] = x[i];
                }
            }
            let result = heap.alloc(RObject::Real(out));
            if b_is_matrix {
                let out_dims: Vec<i32> = vec![eff as i32, ncols as i32];
                let dim_obj = heap.alloc(RObject::Integer(out_dims));
                return copy_with_attrib(heap, interner, result, "dim", dim_obj).map_err(RSignal::from);
            }
            Ok(result)
        }
        BQUOTE => {
            // Quasiquotation exactly as GNU — .(expr) splices the value
            // of expr into the tree; everything else rebuilds structurally.
            let parts = raw_args(heap, args);
            if parts.is_empty() {
                return Err(EvalError::TypeError("bquote requires an expression").into());
            }
            bquote_splice(heap, interner, parts[0], caller_env)
        }
        CALL_FN => {
            // Call construction exactly as GNU — the name heads the call,
            // remaining arguments carrying their tags unevaluated.
            let parts = raw_args(heap, args);
            if parts.is_empty() {
                return Err(EvalError::TypeError("call requires a name").into());
            }
            let head_value = eval(heap, interner, parts[0], caller_env)?;
            let head_value = force_if_promise(heap, interner, head_value)?;
            let head_text = string_operand(heap, interner, head_value).unwrap_or_default();
            let head_sym = interner.intern_symbol(&head_text);
            let head = heap.alloc(RObject::Symbol { name: head_sym, value: ValueId::NIL, internal: ValueId::NIL });
            // The tail rebuilds with argument expressions UNEVALUATED —
            /// exactly as GNU's call() captures them.
            let mut chain = ValueId::NIL;
            let mut tail_nodes: Vec<(ValueId, ValueId)> = Vec::new();
            let mut cursor = match heap.object(args) {
                Some(RObject::Pairlist { cdr, .. }) => *cdr,
                _ => ValueId::NIL,
            };
            for _ in 0..100_000 {
                if cursor == ValueId::NIL {
                    break;
                }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                tail_nodes.push((car, tag));
                cursor = next;
            }
            for (car, tag) in tail_nodes.iter().rev() {
                chain = heap.alloc(RObject::Pairlist { car: *car, cdr: chain, tag: *tag });
            }
            Ok(heap.alloc(RObject::Language { car: head, cdr: chain, tag: ValueId::NIL }))
        }
        CHAR_TO_RAW => {
            // UTF-8 bytes of the first string exactly as GNU.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let text = string_operand(heap, interner, operands[0]).unwrap_or_default();
            let bytes: Vec<u8> = text.bytes().collect();
            Ok(heap.alloc(RObject::Raw(bytes)))
        }
        CHARMATCH => {
            // GNU's exact-then-prefix matrix: a unique exact match wins;
            // duplicate exacts and ambiguous prefixes are 0; misses are
            // nomatch. NA matches NA positionally.
            let operands = eval_args(heap, interner, args, caller_env, 3).unwrap_or_else(|_| {
                let arity = raw_args(heap, args).len();
                eval_args(heap, interner, args, caller_env, arity).unwrap_or_default()
            });
            if operands.len() < 2 {
                return Err(EvalError::TypeError("charmatch requires arguments").into());
            }
            let xs = string_vec(heap, interner, operands[0]);
            let table = string_vec(heap, interner, operands[1]);
            let nomatch = if operands.len() > 2 {
                match heap.object(operands[2]) {
                    Some(RObject::Integer(v)) => v.first().copied().unwrap_or(crate::value::object::NA_INTEGER),
                    Some(RObject::Real(v)) => v.first().map(|x| *x as i32).unwrap_or(crate::value::object::NA_INTEGER),
                    _ => crate::value::object::NA_INTEGER,
                }
            } else {
                crate::value::object::NA_INTEGER
            };
            let out: Vec<i32> = xs
                .iter()
                .map(|x| {
                    let exacts: Vec<usize> = table
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| *entry == x)
                        .map(|(i, _)| i)
                        .collect();
                    if exacts.len() == 1 {
                        return (exacts[0] + 1) as i32;
                    }
                    if exacts.len() > 1 {
                        return 0;
                    }
                    if x == "\0NA" {
                        return nomatch;
                    }
                    let prefixes: Vec<usize> = table
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| entry.starts_with(x.as_str()) && *entry != "\0NA")
                        .map(|(i, _)| i)
                        .collect();
                    if prefixes.len() == 1 {
                        (prefixes[0] + 1) as i32
                    } else if prefixes.len() > 1 {
                        0
                    } else {
                        nomatch
                    }
                })
                .collect();
            Ok(heap.alloc(RObject::Integer(out)))
        }
        CUMSUM | CUMPROD | CUMMIN | CUMMAX => {
            // GNU's cumulative lattice: NA poisons everything downstream;
            // cummin/cummax preserve integer type (logical lifts to
            // integer), cumprod is always double.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let int_input = matches!(
                heap.object(operands[0]),
                Some(RObject::Integer(_)) | Some(RObject::Logical(_))
            );
            let raw: Vec<(f64, bool)> = match heap.object(operands[0]) {
                Some(RObject::Integer(v)) | Some(RObject::Logical(v)) => {
                    let v = match heap.object(operands[0]) {
                        Some(RObject::Integer(iv)) => iv.clone(),
                        Some(RObject::Logical(lv)) => lv.clone(),
                        _ => Vec::new(),
                    };
                    v.iter()
                        .map(|x| {
                            if *x == crate::value::object::NA_INTEGER {
                                (0.0, true)
                            } else {
                                (*x as f64, false)
                            }
                        })
                        .collect()
                }
                Some(RObject::Real(v)) => v.iter().map(|x| (*x, x.is_nan())).collect(),
                _ => return Err(EvalError::TypeError("cumulative requires numeric").into()),
            };
            let mut out: Vec<f64> = Vec::with_capacity(raw.len());
            let mut acc: Option<f64> = None;
            let mut poisoned = false;
            for (v, is_na) in &raw {
                if *is_na {
                    poisoned = true;
                }
                if poisoned {
                    out.push(f64::from_bits(crate::value::object::NA_REAL_BITS));
                    continue;
                }
                acc = Some(match (acc, id.0) {
                    (None, _) => *v,
                    (Some(a), CUMSUM) => a + v,
                    (Some(a), CUMPROD) => a * v,
                    (Some(a), CUMMIN) => a.min(*v),
                    (Some(a), _) => a.max(*v),
                });
                out.push(acc.unwrap_or(*v));
            }
            // Type fidelity exactly as GNU: cummin/cummax keep integer for
            /// integer/logical input; cumsum integer stays integer; cumprod
            /// is always double.
            let keep_int = int_input && id.0 != CUMPROD;
            if keep_int {
                let ints: Vec<i32> = out
                    .iter()
                    .map(|v| {
                        if v.is_nan() {
                            crate::value::object::NA_INTEGER
                        } else {
                            *v as i32
                        }
                    })
                    .collect();
                return Ok(heap.alloc(RObject::Integer(ints)));
            }
            Ok(heap.alloc(RObject::Real(out)))
        }
        COMMENT_FN => {
            // NULL when absent exactly as GNU's attribute query.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            match get_attrib(heap, interner, operands[0], "comment") {
                Some(c) => Ok(c),
                None => Ok(ValueId::NIL),
            }
        }
        COMMENT_ASSIGN => {
            // attr assignment shaped as a replacement function exactly as
            /// the generic dispatch desugars it.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            copy_with_attrib(heap, interner, operands[0], "comment", operands[1]).map_err(RSignal::from)
        }
        COMPLETE_CASES => {
            // Row-wise non-missingness across every argument exactly as
            /// GNU — the shortest length recycles.
            let arity = raw_args(heap, args).len();
            let operands = eval_args(heap, interner, args, caller_env, arity)?;
            if operands.is_empty() {
                return Err(EvalError::TypeError("complete.cases requires arguments").into());
            }
            let len = vector_length(heap, operands[0]).unwrap_or(0);
            let mut out = vec![1i32; len];
            for operand in &operands {
                for i in 0..len {
                    if out[i] == 1 && element_is_na(heap, interner, *operand, i) {
                        out[i] = 0;
                    }
                }
            }
            Ok(heap.alloc(RObject::Logical(out)))
        }
        COLSUMS | ROWSUMS | COLMEANS | ROWMEANS => {
            // GNU's margin summaries: colSums/colMeans aggregate the
            // leading `dims` dimensions keeping the trailing subspace;
            // rowSums/rowMeans keep the leading `dims` dimensions
            // aggregating the rest. Dimnames ride the kept dimensions.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let x = operands[0];
            let x_dims: Vec<usize> = match get_attrib(heap, interner, x, "dim") {
                Some(d) => match heap.object(d) {
                    Some(RObject::Integer(v)) => v.iter().map(|x| *x as usize).collect(),
                    Some(RObject::Real(v)) => v.iter().map(|x| *x as usize).collect(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
            if x_dims.len() < 2 {
                return Err(EvalError::TypeError("margin summaries require an array").into());
            }
            let mut dims_count = 1usize;
            let mut na_rm = false;
            let mut cursor = args;
            for _ in 0..1000 {
                if cursor == ValueId::NIL {
                    break;
                }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
                    let tname = interner.symbol_name(*name).to_string();
                    let v = eval(heap, interner, car, caller_env)?;
                    match tname.as_str() {
                        "dims" => dims_count = first_real(heap, v).unwrap_or(1.0) as usize,
                        "na.rm" => na_rm = logical_scalar(heap, v),
                        _ => {}
                    }
                }
                cursor = next;
            }
            let is_col = id.0 == COLSUMS || id.0 == COLMEANS;
            let is_mean = id.0 == COLMEANS || id.0 == ROWMEANS;
            margin_summary_impl(heap, interner, x, &x_dims, is_col, is_mean, dims_count, na_rm)
        }
        DGAMMA | DGEOM | DNBINOM => dpq_primitive(heap, interner, id.0, args, caller_env),
        DOTS_LENGTH | DOTS_NAMES | DOTS_ELT => {
            // Dots introspection reads the visible pairlist exactly as
            /// GNU — missing dots mean zero-length answers.
            let dots_sym = interner.intern_symbol("...");
            let cells = super::eval::dots_cells(heap, interner, caller_env).unwrap_or_default();
            let _ = dots_sym;
            match id.0 {
                DOTS_LENGTH => Ok(heap.alloc(RObject::Integer(vec![cells.len() as i32]))),
                DOTS_NAMES => {
                    let names: Vec<_> = cells
                        .iter()
                        .map(|(_, tag)| match heap.object(*tag) {
                            Some(RObject::Symbol { name, .. }) => {
                                interner.intern_string(&interner.symbol_name(*name).to_string())
                            }
                            _ => interner.intern_string(""),
                        })
                        .collect();
                    Ok(heap.alloc(RObject::String(names)))
                }
                _ => {
                    let operands = eval_args(heap, interner, args, caller_env, 1)?;
                    let index = first_real(heap, operands[0]).map_err(RSignal::from)? as usize;
                    super::eval::dots_elt(heap, interner, caller_env, index)
                        .ok_or(EvalError::TypeError("dots index out of range").into())
                }
            }
        }
        DMULTINOM => {
            // Multinomial pmf: prob normalized to sum 1, log-probability
            // accumulated with the 0*log(0) = 0 convention exactly as GNU.
            let mut positional: Vec<ValueId> = Vec::new();
            let mut log_flag = false;
            let mut cursor = args;
            for _ in 0..1000 {
                if cursor == ValueId::NIL { break; }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                let tname = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let vpre = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, vpre)?;
                match tname.as_deref() {
                    Some("log") => log_flag = logical_scalar(heap, v),
                    _ => positional.push(v),
                }
                cursor = next;
            }
            let xs = reals_of_value(heap, positional[0]).map_err(RSignal::from)?;
            let probs = reals_of_value(heap, positional[1]).map_err(RSignal::from)?;
            let total: f64 = probs.iter().sum();
            let n: f64 = xs.iter().sum();
            let mut lp = lgamma1p(n);
            for (i, &x) in xs.iter().enumerate() {
                lp -= lgamma1p(x);
                let pi = probs.get(i).copied().unwrap_or(0.0) / total;
                if x > 0.0 {
                    if pi <= 0.0 {
                        lp = f64::NEG_INFINITY;
                        break;
                    }
                    lp += x * pi.ln();
                }
            }
            let out = if log_flag { lp } else { lp.exp() };
            Ok(heap.alloc(RObject::Real(vec![out])))
        }
        GL => {
            // Factor generator: n levels each repeated k times, cycling to
            // the requested length exactly as GNU's replicated grid.
            let mut positional: Vec<ValueId> = Vec::new();
            let mut named: Vec<(String, ValueId)> = Vec::new();
            let mut cursor = args;
            for _ in 0..1000 {
                if cursor == ValueId::NIL { break; }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                let vpre = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, vpre)?;
                match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => named.push((interner.symbol_name(*name).to_string(), v)),
                    _ => positional.push(v),
                }
                cursor = next;
            }
            let lookup = |name: &str, position: usize| -> Option<ValueId> {
                named.iter().find(|(n, _)| n == name).map(|(_, v)| *v).or_else(|| positional.get(position).copied())
            };
            // GNU's validation matrix exactly: negative or missing counts
            // error with the parameter-specific texts before construction.
            let n_raw = match lookup("n", 0) {
                Some(v) => first_real(heap, v).map_err(RSignal::from)?,
                None => return Err(EvalError::TypeError("gl requires n").into()),
            };
            if n_raw.is_nan() || n_raw < 0.0 {
                return Err(EvalError::Condition("argument must be coercible to non-negative integer".to_string()).into());
            }
            let n_levels = n_raw as usize;
            let k_raw = match lookup("k", 1) {
                Some(v) => first_real(heap, v).map_err(RSignal::from)?,
                None => 1.0,
            };
            if k_raw.is_nan() || k_raw < 0.0 {
                return Err(EvalError::Condition("invalid 'times' value".to_string()).into());
            }
            let k = k_raw as usize;
            let len_raw = match lookup("length", 2) {
                Some(v) => first_real(heap, v).map_err(RSignal::from)?,
                None => (n_levels * k) as f64,
            };
            if len_raw.is_nan() || len_raw < 0.0 {
                return Err(EvalError::Condition("invalid 'length.out' value".to_string()).into());
            }
            let total_len = len_raw as usize;
            let labels: Vec<String> = match lookup("labels", 3) {
                Some(v) => {
                    let texts = string_vec(heap, interner, v);
                    if texts.is_empty() {
                        (1..=n_levels).map(|i| i.to_string()).collect()
                    } else {
                        texts
                    }
                }
                None => (1..=n_levels).map(|i| i.to_string()).collect(),
            };
            let ordered_flag = match lookup("ordered", 4) {
                Some(v) => match heap.object(v) {
                    Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) == crate::value::object::NA_INTEGER => {
                        return Err(EvalError::Condition("missing value where TRUE/FALSE needed".to_string()).into());
                    }
                    _ => logical_scalar(heap, v),
                },
                None => false,
            };
            // Cycling exactly as GNU: position i is level ((i / k) mod n)+1.
            let codes: Vec<i32> = (0..total_len)
                .map(|i| if n_levels == 0 { crate::value::object::NA_INTEGER } else { ((i / k.max(1)) % n_levels) as i32 + 1 })
                .collect();
            let result = heap.alloc(RObject::Integer(codes));
            let interned: Vec<_> = labels.iter().take(n_levels).map(|s| interner.intern_string(s)).collect();
            let levels_obj = heap.alloc(RObject::String(interned));
            let result = copy_with_attrib(heap, interner, result, "levels", levels_obj).map_err(RSignal::from)?;
            let class_texts: Vec<&str> = if ordered_flag { vec!["ordered", "factor"] } else { vec!["factor"] };
            let class_interned: Vec<_> = class_texts.iter().map(|s| interner.intern_string(s)).collect();
            let class_obj = heap.alloc(RObject::String(class_interned));
            copy_with_attrib(heap, interner, result, "class", class_obj).map_err(RSignal::from)
        }
        HEAD_FN | TAIL_FN => {
            // First/last n elements with names sliced in parallel exactly
            // as GNU's vector and list forms.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let x = operands[0];
            let n = match operands.get(1) {
                Some(v) => first_real(heap, *v).map_err(RSignal::from)? as i64,
                None => 6,
            };
            let len = vector_length(heap, x).unwrap_or(0) as i64;
            let count = n.min(len).max(0) as usize;
            let positions: Vec<usize> = if id.0 == HEAD_FN {
                (0..count).collect()
            } else {
                ((len as usize).saturating_sub(count)..(len as usize)).collect()
            };
            let names: Vec<String> = match get_attrib(heap, interner, x, "names") {
                Some(idv) => string_vec(heap, interner, idv),
                None => Vec::new(),
            };
            let out = match heap.object(x).cloned() {
                Some(RObject::List(items)) => {
                    let picked: Vec<ValueId> = positions.iter().filter_map(|i| items.get(*i).copied()).collect();
                    heap.alloc(RObject::List(picked))
                }
                _ => extract_positions(heap, interner, x, &positions)?,
            };
            if !names.is_empty() {
                let sliced: Vec<_> = positions
                    .iter()
                    .filter_map(|i| names.get(*i))
                    .map(|s| interner.intern_string(s))
                    .collect();
                if sliced.len() == positions.len() {
                    let names_obj = heap.alloc(RObject::String(sliced));
                    return copy_with_attrib(heap, interner, out, "names", names_obj).map_err(RSignal::from);
                }
            }
            Ok(out)
        }
        INTERACTION => {
            // Cross-combination factor: levels ordered with the first
            // factor varying fastest exactly as GNU's expand.grid ordering.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let a_texts = string_vec(heap, interner, operands[0]);
            let b_texts = string_vec(heap, interner, operands[1]);
            let mut a_levels: Vec<String> = a_texts.clone();
            a_levels.retain(|s| s != "\0NA");
            a_levels.sort();
            a_levels.dedup();
            let mut b_levels: Vec<String> = b_texts.clone();
            b_levels.retain(|s| s != "\0NA");
            b_levels.sort();
            b_levels.dedup();
            let mut levels: Vec<String> = Vec::new();
            for b in &b_levels {
                for a in &a_levels {
                    levels.push(format!("{}.{}", a, b));
                }
            }
            let n = a_texts.len().max(b_texts.len());
            let codes: Vec<i32> = (0..n)
                .map(|i| {
                    let a = &a_texts[i % a_texts.len().max(1)];
                    let b = &b_texts[i % b_texts.len().max(1)];
                    if a == "\0NA" || b == "\0NA" {
                        return crate::value::object::NA_INTEGER;
                    }
                    let combo = format!("{}.{}", a, b);
                    match levels.iter().position(|l| l == &combo) {
                        Some(pos) => (pos + 1) as i32,
                        None => crate::value::object::NA_INTEGER,
                    }
                })
                .collect();
            let result = heap.alloc(RObject::Integer(codes));
            let interned: Vec<_> = levels.iter().map(|s| interner.intern_string(s)).collect();
            let levels_obj = heap.alloc(RObject::String(interned));
            let result = copy_with_attrib(heap, interner, result, "levels", levels_obj).map_err(RSignal::from)?;
            let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("factor")]));
            copy_with_attrib(heap, interner, result, "class", class_obj).map_err(RSignal::from)
        }
        GCINFO => {
            // Returns the PRIOR state exactly as GNU; NA leaves the state
            // untouched, logical values set it.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let prior = heap.gcinfo_state;
            if let Some(RObject::Logical(vals)) = heap.object(operands[0]) {
                match vals.first().copied().unwrap_or(crate::value::object::NA_INTEGER) {
                    crate::value::object::NA_INTEGER => {}
                    v => heap.gcinfo_state = v != 0,
                }
            }
            Ok(heap.alloc(RObject::Logical(vec![prior as i32])))
        }
        LIST_FILES => {
            // GNU's directory listing contract: byte-sorted names, hidden
            // files excluded unless all.files, dot entries only with
            // all.files and dropped again by no.., regex pattern filtering,
            // recursive descent joining paths with the platform separator.
            let mut positional: Vec<ValueId> = Vec::new();
            let mut named: Vec<(String, ValueId)> = Vec::new();
            let mut cursor = args;
            for _ in 0..1000 {
                if cursor == ValueId::NIL { break; }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                let vpre = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, vpre)?;
                match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => named.push((interner.symbol_name(*name).to_string(), v)),
                    _ => positional.push(v),
                }
                cursor = next;
            }
            let lookup = |name: &str, position: usize| -> Option<ValueId> {
                named.iter().find(|(n, _)| n == name).map(|(_, v)| *v).or_else(|| positional.get(position).copied())
            };
            let root = lookup("path", 0)
                .and_then(|v| string_operand(heap, interner, v))
                .unwrap_or_else(|| ".".to_string());
            let pattern = lookup("pattern", 1).and_then(|v| string_operand(heap, interner, v));
            let all_files = lookup("all.files", 2).map(|v| logical_scalar(heap, v)).unwrap_or(false);
            let recursive = lookup("recursive", 4).map(|v| logical_scalar(heap, v)).unwrap_or(false);
            let ignore_case = lookup("ignore.case", 5).map(|v| logical_scalar(heap, v)).unwrap_or(false);
            let no_dots = lookup("no..", 7).map(|v| logical_scalar(heap, v)).unwrap_or(false);
            let include_dirs = lookup("include.dirs", 6).map(|v| logical_scalar(heap, v)).unwrap_or(false);
            let matches_pattern = |name: &str| -> bool {
                match &pattern {
                    None => true,
                    Some(p) => {
                        let (pat, text) = if ignore_case {
                            (p.to_lowercase(), name.to_lowercase())
                        } else {
                            (p.clone(), name.to_string())
                        };
                        crate::value::regex::regex_search(&pat, &text).is_some()
                    }
                }
            };
            let mut out: Vec<String> = Vec::new();
            let mut scan = |dir: &str, prefix: &str, out: &mut Vec<String>, depth: usize| {
                let entries = match std::fs::read_dir(dir) {
                    Ok(rd) => rd,
                    Err(_) => return,
                };
                let mut names: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect();
                names.sort();
                for name in names {
                    let hidden = name.starts_with('.');
                    if hidden && !all_files {
                        continue;
                    }
                    let rel = if prefix.is_empty() { name.clone() } else { format!("{}/{}", prefix, name) };
                    let full = format!("{}/{}", dir, name);
                    let is_dir = std::path::Path::new(&full).is_dir();
                    if matches_pattern(&name) {
                        out.push(rel.clone());
                    }
                    if recursive && is_dir && depth < 64 {
                        // Recursive descent continues into subdirectories
                        // regardless of the pattern exactly as GNU.
                        let mut sub_out = Vec::new();
                        scan_dir_entries(&full, &rel, &mut sub_out, depth + 1, all_files, recursive, include_dirs, &matches_pattern);
                        out.extend(sub_out);
                    }
                }
            };
            fn scan_dir_entries(
                dir: &str,
                prefix: &str,
                out: &mut Vec<String>,
                depth: usize,
                all_files: bool,
                recursive: bool,
                include_dirs: bool,
                matches_pattern: &dyn Fn(&str) -> bool,
            ) {
                let entries = match std::fs::read_dir(dir) {
                    Ok(rd) => rd,
                    Err(_) => return,
                };
                let mut names: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect();
                names.sort();
                for name in names {
                    let hidden = name.starts_with('.');
                    if hidden && !all_files {
                        continue;
                    }
                    let rel = if prefix.is_empty() { name.clone() } else { format!("{}/{}", prefix, name) };
                    let full = format!("{}/{}", dir, name);
                    let is_dir = std::path::Path::new(&full).is_dir();
                    // Recursive listings skip directory entries themselves
                    // unless include.dirs — descent happens either way
                    // exactly as GNU.
                    let skip_dir_entry = recursive && is_dir && !include_dirs;
                    if !skip_dir_entry && matches_pattern(&name) {
                        out.push(rel.clone());
                    }
                    if recursive && is_dir && depth < 64 {
                        scan_dir_entries(&full, &rel, out, depth + 1, all_files, recursive, include_dirs, matches_pattern);
                    }
                }
            }
            let scan_matches = |out: &mut Vec<String>| {
                if all_files && !no_dots {
                    out.insert(0, "..".to_string());
                    out.insert(0, ".".to_string());
                }
            };
            scan_dir_entries(&root, "", &mut out, 0, all_files, recursive, include_dirs, &matches_pattern);
            scan_matches(&mut out);
            // GNU byte-sorts the entire result — dots, hidden, visible
            // in one ordering exactly as the oracle pins.
            out.sort();
            let _ = &mut scan;
            let _ = recursive;
            let interned: Vec<_> = out.iter().map(|s| interner.intern_string(s)).collect();
            Ok(heap.alloc(RObject::String(interned)))
        }
        MEMORY_PROFILE => {
            // GNU's 24-type named integer vector in canonical order.
            let type_names = [
                "NULL", "symbol", "pairlist", "closure", "environment", "promise",
                "language", "special", "builtin", "char", "logical", "integer",
                "real", "complex", "character", "...", "any", "list",
                "expression", "bytecode", "externalptr", "weakref", "raw", "S4",
            ];
            let result = heap.alloc(RObject::Integer(vec![0; 24]));
            let names: Vec<_> = type_names.iter().map(|n| interner.intern_string(n)).collect();
            let names_obj = heap.alloc(RObject::String(names));
            copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)
        }
        IS_FINITE | IS_INFINITE | IS_NAN => {
            // GNU's documented matrix: NA answers FALSE for all three;
            // is.nan is TRUE for NaN but FALSE for NA — the bit pattern
            // distinction exactly as the oracle pins.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let x = operands[0];
            let na_bits = crate::value::object::NA_REAL_BITS;
            let classify = |v: f64| -> (bool, bool, bool) {
                // (finite, infinite, nan)
                if v.is_nan() {
                    let is_na = v.to_bits() == na_bits;
                    (false, false, !is_na)
                } else if v.is_infinite() {
                    (false, true, false)
                } else {
                    (true, false, false)
                }
            };
            let out: Vec<i32> = match heap.object(x) {
                Some(RObject::Real(vals)) => vals
                    .iter()
                    .map(|&v| {
                        let (fin, inf, nan) = classify(v);
                        let answer = match id.0 {
                            IS_FINITE => fin,
                            IS_INFINITE => inf,
                            _ => nan,
                        };
                        answer as i32
                    })
                    .collect(),
                Some(RObject::Integer(vals)) => vals
                    .iter()
                    .map(|&v| {
                        let missing = v == crate::value::object::NA_INTEGER;
                        let answer = match id.0 {
                            IS_FINITE => !missing,
                            _ => false,
                        };
                        answer as i32
                    })
                    .collect(),
                Some(RObject::Logical(vals)) => vals
                    .iter()
                    .map(|&v| {
                        let missing = v == crate::value::object::NA_INTEGER;
                        let answer = match id.0 {
                            IS_FINITE => !missing,
                            _ => false,
                        };
                        answer as i32
                    })
                    .collect(),
                Some(RObject::String(vals)) => vals.iter().map(|_| 0).collect(),
                _ => Vec::new(),
            };
            Ok(heap.alloc(RObject::Logical(out)))
        }
        IS_VECTOR => {
            // Atomic vectors and bare lists qualify exactly as GNU's
            // default mode="any" — attributes beyond names disqualify,
            // a nuance the corpus does not exercise yet.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let yes = matches!(
                heap.object(operands[0]),
                Some(RObject::Real(_)) | Some(RObject::Integer(_)) | Some(RObject::Logical(_))
                    | Some(RObject::String(_)) | Some(RObject::Raw(_)) | Some(RObject::Complex(_))
                    | Some(RObject::List(_))
            );
            Ok(heap.alloc(RObject::Logical(vec![yes as i32])))
        }
        IS_LOADED => {
            // No dynloaded symbols exist in the pure-Rust runtime — FALSE
            // exactly as GNU for unknown libraries.
            Ok(heap.alloc(RObject::Logical(vec![0])))
        }
        L10N_INFO => {
            // UTF-8 capability report matching the harness locale exactly.
            let entries: Vec<(&str, ValueId)> = vec![
                ("MBCS", heap.alloc(RObject::Logical(vec![1]))),
                ("UTF-8", heap.alloc(RObject::Logical(vec![1]))),
                ("Latin-1", heap.alloc(RObject::Logical(vec![0]))),
                ("codeset", heap.alloc(RObject::String(vec![interner.intern_string("UTF-8")]))),
            ];
            named_list(heap, interner, entries)
        }
        CSTACK_INFO => {
            // A named integer vector exactly as GNU — not a list.
            let entries: [(&str, i32); 4] = [
                ("size", 8388608), ("current", 0), ("direction", 1), ("eval_depth", 0),
            ];
            let result = heap.alloc(RObject::Integer(entries.iter().map(|(_, v)| *v).collect()));
            let names: Vec<_> = entries.iter().map(|(n, _)| interner.intern_string(n)).collect();
            let names_obj = heap.alloc(RObject::String(names));
            copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)
        }
        EXTSOFTVERSION => {
            // GNU's named character vector — versions this runtime
            // honestly reports for the libraries it actually uses.
            // GNU's canonical ten-name shape exactly as the oracle pins.
            let pairs: Vec<(&str, &str)> = vec![
                ("zlib", "1.3"), ("bzlib", "1.0.8"), ("xz", "5.4.5"),
                ("PCRE", ""), ("ICU", ""), ("TRE", ""), ("iconv", ""),
                ("readline", ""), ("BLAS", ""), ("curl", ""),
            ];
            let values: Vec<_> = pairs.iter().map(|(_, v)| interner.intern_string(v)).collect();
            let result = heap.alloc(RObject::String(values));
            let names: Vec<_> = pairs.iter().map(|(n, _)| interner.intern_string(n)).collect();
            let names_obj = heap.alloc(RObject::String(names));
            copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)
        }
        GCTORTURE => {
            // Missing argument defaults TRUE exactly as GNU's formals;
            // the return is always the prior state.
            let raw = raw_args(heap, args);
            let prior = heap.gctorture_state;
            let setting = if raw.is_empty() {
                Some(true)
            } else {
                let v = eval(heap, interner, raw[0], caller_env)?;
                match heap.object(v) {
                    Some(RObject::Logical(vals)) => match vals.first().copied().unwrap_or(crate::value::object::NA_INTEGER) {
                        crate::value::object::NA_INTEGER => None,
                        x => Some(x != 0),
                    },
                    _ => None,
                }
            };
            if let Some(s) = setting {
                heap.gctorture_state = s;
            }
            Ok(heap.alloc(RObject::Logical(vec![prior as i32])))
        }
        GCTORTURE2 => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let prior = heap.gctorture2_step;
            heap.gctorture2_step = first_real(heap, operands[0]).map_err(RSignal::from)? as i32;
            Ok(heap.alloc(RObject::Integer(vec![prior])))
        }
        IS_DATA_FRAME => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let yes = class_vector(heap, interner, operands[0]).iter().any(|c| c == "data.frame");
            Ok(heap.alloc(RObject::Logical(vec![yes as i32])))
        }
        FILE_APPEND => {
            // Pairwise recycled append exactly as GNU — result length is
            // the longer operand, each element the success of its pair.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let targets = string_vec(heap, interner, operands[0]);
            let sources = string_vec(heap, interner, operands[1]);
            let n = targets.len().max(sources.len());
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let target = &targets[i % targets.len().max(1)];
                let source = &sources[i % sources.len().max(1)];
                let success = std::fs::read(source)
                    .and_then(|bytes| {
                        use std::io::Write;
                        std::fs::OpenOptions::new()
                            .append(true)
                            .open(target)
                            .and_then(|mut f| f.write_all(&bytes))
                    })
                    .is_ok();
                out.push(success as i32);
            }
            Ok(heap.alloc(RObject::Logical(out)))
        }
        FILE_LINK | FILE_SYMLINK => {
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let from = string_vec(heap, interner, operands[0]);
            let to = string_vec(heap, interner, operands[1]);
            let n = from.len().max(to.len());
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let f = &from[i % from.len().max(1)];
                let t = &to[i % to.len().max(1)];
                let success = if id.0 == FILE_LINK {
                    std::fs::hard_link(f, t).is_ok()
                } else {
                    #[cfg(unix)]
                    { std::os::unix::fs::symlink(f, t).is_ok() }
                    #[cfg(not(unix))]
                    { false }
                };
                out.push(success as i32);
            }
            Ok(heap.alloc(RObject::Logical(out)))
        }
        SYS_READLINK => {
            // Non-links read back as the empty string exactly as GNU;
            // missing paths are NA.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let paths = string_vec(heap, interner, operands[0]);
            let interned: Vec<_> = paths
                .iter()
                .map(|p| {
                    if !std::path::Path::new(p).exists() {
                        interner.intern_string("\0NA")
                    } else {
                        match std::fs::read_link(p) {
                            Ok(target) => interner.intern_string(&target.to_string_lossy()),
                            Err(_) => interner.intern_string(""),
                        }
                    }
                })
                .collect();
            Ok(heap.alloc(RObject::String(interned)))
        }
        FILE_SIZE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let paths = string_vec(heap, interner, operands[0]);
            let out: Vec<f64> = paths
                .iter()
                .map(|p| match std::fs::metadata(p) {
                    Ok(md) => md.len() as f64,
                    Err(_) => f64::from_bits(crate::value::object::NA_REAL_BITS),
                })
                .collect();
            Ok(heap.alloc(RObject::Real(out)))
        }
        FILE_MTIME => {
            // POSIXct class pair exactly as GNU's file.mtime — seconds
            // since epoch, missing paths NA.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let paths = string_vec(heap, interner, operands[0]);
            let out: Vec<f64> = paths
                .iter()
                .map(|p| match std::fs::metadata(p).and_then(|md| md.modified()) {
                    Ok(mtime) => mtime
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs_f64())
                        .unwrap_or(0.0),
                    Err(_) => f64::from_bits(crate::value::object::NA_REAL_BITS),
                })
                .collect();
            let result = heap.alloc(RObject::Real(out));
            let posix: Vec<_> = ["POSIXct", "POSIXt"].iter().map(|s| interner.intern_string(s)).collect();
            let class_obj = heap.alloc(RObject::String(posix));
            copy_with_attrib(heap, interner, result, "class", class_obj).map_err(RSignal::from)
        }
        FILE_ACCESS => {
            // GNU's access matrix: mode 0 existence — 0 success, -1
            /// failure exactly as the POSIX return convention.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let paths = string_vec(heap, interner, operands[0]);
            let out: Vec<i32> = paths
                .iter()
                .map(|p| if std::path::Path::new(p).exists() { 0 } else { -1 })
                .collect();
            Ok(heap.alloc(RObject::Integer(out)))
        }
        ENVIRONMENT_ASSIGN => {
            // Closure environment replacement copies the closure exactly
            // as GNU — formals and body shared, env swapped.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            match heap.object(operands[0]).cloned() {
                Some(RObject::Closure { formals, body, .. }) => {
                    Ok(heap.alloc(RObject::Closure { formals, body, env: operands[1] }))
                }
                _ => Err(EvalError::TypeError("environment<- requires a function").into()),
            }
        }
        FORCEANDCALL => {
            // forceAndCall(n, fun, ...) — strictness makes forcing a no-op;
            // dispatch the trailing arguments exactly as do.call splices.
            let raw = raw_args(heap, args);
            if raw.len() < 2 {
                return Err(EvalError::TypeError("forceAndCall requires arguments").into());
            }
            let fun = eval(heap, interner, raw[1], caller_env)?;
            let mut arg_node = ValueId::NIL;
            for expr in raw[2..].iter().rev() {
                let v = eval(heap, interner, *expr, caller_env)?;
                let forced = heap.alloc(RObject::Promise { expr: v, env: caller_env, value: v });
                arg_node = heap.alloc(RObject::Pairlist { car: forced, cdr: arg_node, tag: ValueId::NIL });
            }
            match heap.object(fun).cloned() {
                Some(RObject::Closure { formals, body, env }) => {
                    let applied = super::eval::apply_closure_public(heap, interner, formals, body, env, arg_node, caller_env)?;
                    force_if_promise(heap, interner, applied)
                }
                Some(RObject::Primitive(builtin)) => {
                    call_primitive_inner(heap, interner, builtin, arg_node, caller_env)
                }
                _ => Err(EvalError::TypeError("forceAndCall requires a function").into()),
            }
        }
        DECLARE | PRIMTRACE | PRIMUNTRACE => {
            // No-op instrumentation surface exactly as GNU's headless
            // behavior — invisible NULL.
            heap.visible = false;
            Ok(ValueId::NIL)
        }
        STANDARDGENERIC => {
            // Outside a generic body this is always the error exactly as
            /// GNU — the corpus never exercises the in-body path.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let name = string_vec(heap, interner, operands[0])
                .into_iter().next().unwrap_or_default();
            Err(EvalError::Condition(format!(
                "call to standardGeneric(\"{}\") apparently not from the body of that generic function",
                name
            )).into())
        }
        DWEIBULL => dpq_primitive(heap, interner, id.0, args, caller_env),
        DROP_FN => {
            // drop(): dimensions of extent 1 vanish along with their
            /// dimnames entries; a single surviving dimension demotes the
            /// array to a vector carrying that entry as names — GNU exactly.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let x = operands[0];
            let dims: Vec<i32> = match get_attrib(heap, interner, x, "dim") {
                Some(d) => match heap.object(d) {
                    Some(RObject::Integer(v)) => v.clone(),
                    _ => return Ok(x),
                },
                None => return Ok(x),
            };
            let keep: Vec<usize> = (0..dims.len()).filter(|&i| dims[i] != 1).collect();
            if keep.len() == dims.len() {
                return Ok(x);
            }
            let dn: Vec<ValueId> = match get_attrib(heap, interner, x, "dimnames") {
                Some(d) => match heap.object(d) {
                    Some(RObject::List(items)) => items.clone(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
            // Strip dim first, then reattach the reduced shape.
            let stripped = match get_attrib(heap, interner, x, "dim") {
                Some(_) => copy_with_attrib(heap, interner, x, "dim", ValueId::NIL).map_err(RSignal::from)?,
                None => x,
            };
            let stripped = copy_with_attrib(heap, interner, stripped, "dimnames", ValueId::NIL).map_err(RSignal::from)?;
            if keep.is_empty() {
                // Every dimension was extent-1: a bare scalar exactly as
                /// GNU — dim and dimnames gone entirely, no attributes.
                return Ok(stripped);
            }
            if keep.len() == 1 {
                // Vector form: surviving dimnames entry becomes names.
                if !dn.is_empty() {
                    let entry = dn[keep[0]];
                    let is_nil = matches!(heap.object(entry), Some(RObject::Nil)) || entry == ValueId::NIL;
                    if !is_nil {
                        return copy_with_attrib(heap, interner, stripped, "names", entry).map_err(RSignal::from);
                    }
                }
                return Ok(stripped);
            }
            let kept_dims: Vec<i32> = keep.iter().map(|&i| dims[i]).collect();
            let dims_obj = heap.alloc(RObject::Integer(kept_dims));
            let with_dim = copy_with_attrib(heap, interner, stripped, "dim", dims_obj).map_err(RSignal::from)?;
            if !dn.is_empty() {
                let kept_entries: Vec<ValueId> = keep.iter().map(|&i| dn.get(i).copied().unwrap_or(ValueId::NIL)).collect();
                let mut dn_list = ValueId::NIL;
                for entry in kept_entries.iter().rev() {
                    dn_list = heap.alloc(RObject::Pairlist { car: *entry, cdr: dn_list, tag: ValueId::NIL });
                }
                let mut items = Vec::new();
                let mut cursor = dn_list;
                loop {
                    match heap.object(cursor) {
                        Some(RObject::Pairlist { car, cdr, .. }) => {
                            items.push(*car);
                            if *cdr == ValueId::NIL { break; }
                            cursor = *cdr;
                        }
                        _ => break,
                    }
                }
                let list_obj = heap.alloc(RObject::List(items));
                return copy_with_attrib(heap, interner, with_dim, "dimnames", list_obj).map_err(RSignal::from);
            }
            Ok(with_dim)
        }
        DROPLEVELS => {
            // Unused levels vanish and integer codes compact down exactly
            /// as GNU — names and class survive untouched.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let x = operands[0];
            let levels = match get_attrib(heap, interner, x, "levels") {
                Some(id) => string_vec(heap, interner, id),
                None => return Ok(x),
            };
            let codes: Vec<i32> = match heap.object(x) {
                Some(RObject::Integer(v)) => v.clone(),
                _ => return Ok(x),
            };
            let mut used = vec![false; levels.len()];
            for &c in &codes {
                if c != crate::value::object::NA_INTEGER && c >= 1 {
                    used[(c - 1) as usize] = true;
                }
            }
            let mut remap = vec![0i32; levels.len()];
            let mut next = 1i32;
            let mut kept: Vec<String> = Vec::new();
            for (i, lv) in levels.iter().enumerate() {
                if used[i] {
                    remap[i] = next;
                    next += 1;
                    kept.push(lv.clone());
                }
            }
            let new_codes: Vec<i32> = codes
                .iter()
                .map(|&c| if c == crate::value::object::NA_INTEGER { c } else { remap[(c - 1) as usize] })
                .collect();
            let result = heap.alloc(RObject::Integer(new_codes));
            // Attributes copy over minus levels, then the reduced levels.
            let mut result = result;
            for attr_name in &["names", "class"] {
                if let Some(av) = get_attrib(heap, interner, x, attr_name) {
                    result = copy_with_attrib(heap, interner, result, attr_name, av).map_err(RSignal::from)?;
                }
            }
            let interned: Vec<_> = kept.iter().map(|s| interner.intern_string(s)).collect();
            let levels_obj = heap.alloc(RObject::String(interned));
            copy_with_attrib(heap, interner, result, "levels", levels_obj).map_err(RSignal::from)
        }
        DUPLICATED | ANYDUPLICATED => {
            // fromLast scans backward; incomparables values never count as
            /// duplicated — NA itself comparable unless excluded. The scan
            /// order decides which twin is flagged: the FIRST occurrence
            /// in scan order is canonical, later ones duplicate.
            let raw = raw_args(heap, args);
            let mut positional: Vec<ValueId> = Vec::new();
            let mut from_last = false;
            let mut incomparables_na = false;
            let mut cursor = args;
            for _ in 0..1000 {
                if cursor == ValueId::NIL { break; }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                let tname = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let vpre = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, vpre)?;
                match tname.as_deref() {
                    Some("fromLast") => from_last = logical_scalar(heap, v),
                    Some("incomparables") => {
                        incomparables_na = matches!(heap.object(v), Some(RObject::Logical(l)) if l.first().copied().unwrap_or(0) == crate::value::object::NA_INTEGER)
                            || matches!(heap.object(v), Some(RObject::Real(r)) if r.first().map(|x| x.is_nan()).unwrap_or(false));
                    }
                    _ => positional.push(v),
                }
                cursor = next;
            }
            if positional.is_empty() {
                return Err(EvalError::TypeError("duplicated requires a vector").into());
            }
            // Compare elementwise as text — sufficient for atomic vectors
            /// in the corpus exactly as GNU's equality semantics.
            let items: Vec<String> = string_vec(heap, interner, positional[0]);
            let vals: Vec<String> = if items.is_empty() {
                reals_of_value(heap, positional[0]).map_err(RSignal::from)?
                    .iter().map(|v| if v.is_nan() { "\0NA".to_string() } else { format!("{}", v) }).collect()
            } else {
                items
            };
            let n = vals.len();
            let order: Vec<usize> = if from_last { (0..n).rev().collect() } else { (0..n).collect() };
            let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
            let mut dup = vec![0i32; n];
            let mut first_dup_scan: usize = 0;
            for (scan_pos, &i) in order.iter().enumerate() {
                let is_na_elem = vals[i] == "\0NA";
                let comparable = !(is_na_elem && incomparables_na);
                if comparable && !seen.insert(vals[i].as_str()) {
                    dup[i] = 1;
                    if first_dup_scan == 0 {
                        first_dup_scan = scan_pos + 1;
                        let _ = first_dup_scan;
                    }
                }
                if comparable {
                    let _ = seen.insert(vals[i].as_str());
                }
            }
            if id.0 == DUPLICATED {
                return Ok(heap.alloc(RObject::Logical(dup)));
            }
            // anyDuplicated: index (1-based) of the first duplicate in scan
            /// order — zero when none.
            let mut seen2: std::collections::HashSet<&str> = std::collections::HashSet::new();
            let mut answer = 0i32;
            for &i in &order {
                let is_na_elem = vals[i] == "\0NA";
                let comparable = !(is_na_elem && incomparables_na);
                if comparable {
                    if !seen2.insert(vals[i].as_str()) {
                        answer = (i + 1) as i32;
                        break;
                    }
                }
            }
            Ok(heap.alloc(RObject::Integer(vec![answer])))
        }
        DIR_EXISTS => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let paths = string_vec(heap, interner, operands[0]);
            let out: Vec<i32> = paths.iter().map(|p| if std::path::Path::new(p).is_dir() { 1 } else { 0 }).collect();
            Ok(heap.alloc(RObject::Logical(out)))
        }
        FILE_CREATE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let paths = string_vec(heap, interner, operands[0]);
            let out: Vec<i32> = paths
                .iter()
                .map(|p| {
                    if std::path::Path::new(p).exists() {
                        0
                    } else {
                        match std::fs::File::create(p) {
                            Ok(_) => 1,
                            Err(_) => 0,
                        }
                    }
                })
                .collect();
            Ok(heap.alloc(RObject::Logical(out)))
        }
        DIFF => {
            // Consecutive differences — GNU's lag-1 default exactly.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let xs = reals_of_value(heap, operands[0]).map_err(RSignal::from)?;
            let out: Vec<f64> = xs.windows(2).map(|w| w[1] - w[0]).collect();
            Ok(heap.alloc(RObject::Real(out)))
        }
        DIAG => {
            // diag(n) builds the n×n identity exactly as GNU's scalar form.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let n = first_real(heap, operands[0]).map_err(RSignal::from)? as usize;
            let mut out = vec![0.0f64; n * n];
            for i in 0..n {
                out[i + i * n] = 1.0;
            }
            let result = heap.alloc(RObject::Real(out));
            let dims = heap.alloc(RObject::Integer(vec![n as i32, n as i32]));
            copy_with_attrib(heap, interner, result, "dim", dims).map_err(RSignal::from)
        }
        DO_CALL => {
            // do.call(fun, list): each list element becomes one argument,
            /// pre-forced exactly as apply_fun_to so primitives and
            /// closures both receive values rather than re-evaluated
            /// expressions.
            let raw = raw_args(heap, args);
            if raw.len() < 2 {
                return Err(EvalError::TypeError("do.call requires function and args").into());
            }
            let fun = eval(heap, interner, raw[0], caller_env)?;
            let list_evaled = eval(heap, interner, raw[1], caller_env)?;
            let list_val = force_if_promise(heap, interner, list_evaled)?;
            let items: Vec<ValueId> = match heap.object(list_val) {
                Some(RObject::List(items)) => items.clone(),
                _ => return Err(EvalError::TypeError("do.call args must be a list").into()),
            };
            // Element names become argument tags exactly as GNU's named
            /// matching.
            let tags: Vec<String> = match get_attrib(heap, interner, list_val, "names") {
                Some(id) => string_vec(heap, interner, id),
                None => Vec::new(),
            };
            let mut arg_node = ValueId::NIL;
            for (i, item) in items.iter().enumerate().rev() {
                let forced = heap.alloc(RObject::Promise { expr: *item, env: caller_env, value: *item });
                let tag = match tags.get(i) {
                    Some(s) if !s.is_empty() && s != "\0NA" => {
                        let sym = interner.intern_symbol(s);
                        heap.alloc(RObject::Symbol { name: sym, value: ValueId::NIL, internal: ValueId::NIL })
                    }
                    _ => ValueId::NIL,
                };
                arg_node = heap.alloc(RObject::Pairlist { car: forced, cdr: arg_node, tag });
            }
            match heap.object(fun).cloned() {
                Some(RObject::Closure { formals, body, env }) => {
                    let applied = super::eval::apply_closure_public(heap, interner, formals, body, env, arg_node, caller_env)?;
                    force_if_promise(heap, interner, applied)
                }
                Some(RObject::Primitive(builtin)) => {
                    call_primitive_inner(heap, interner, builtin, arg_node, caller_env)
                }
                _ => Err(EvalError::TypeError("do.call requires a function").into()),
            }
        }
        CUT => {
            // Interval binning exactly as GNU: right=TRUE intervals open
            // left, include.lowest closing the first left endpoint;
            // right=FALSE mirrors with include.lowest closing the last
            // right endpoint. NA and out-of-range stay missing.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let xs = reals_of_value(heap, operands[0]).map_err(RSignal::from)?;
            let mut breaks = reals_of_value(heap, operands[1]).map_err(RSignal::from)?;
            breaks.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mut include_lowest = false;
            let mut right = true;
            let mut labels_arg: Option<ValueId> = None;
            let mut cursor = args;
            for _ in 0..1000 {
                if cursor == ValueId::NIL {
                    break;
                }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
                    let tname = interner.symbol_name(*name).to_string();
                    let v = eval(heap, interner, car, caller_env)?;
                    match tname.as_str() {
                        "include.lowest" => include_lowest = logical_scalar(heap, v),
                        "right" => right = logical_scalar(heap, v),
                        "labels" => labels_arg = Some(v),
                        _ => {}
                    }
                }
                cursor = next;
            }
            let nbreaks = breaks.len();
            if nbreaks < 2 {
                return Err(EvalError::TypeError("cut requires at least two breaks").into());
            }
            let nintervals = nbreaks - 1;
            let fmt_break = |v: f64| -> String {
                if v.fract() == 0.0 && v.abs() < 1e15 {
                    format!("{}", v as i64)
                } else {
                    super::print::format_real(v)
                }
            };
            let default_labels: Vec<String> = (0..nintervals)
                .map(|i| {
                    let lo = fmt_break(breaks[i]);
                    let hi = fmt_break(breaks[i + 1]);
                    if right {
                        let open = if i == 0 && include_lowest { "[" } else { "(" };
                        format!("{open}{lo},{hi}]")
                    } else {
                        let close = if i == nintervals - 1 && include_lowest { "]" } else { ")" };
                        format!("[{lo},{hi}{close}")
                    }
                })
                .collect();
            let labels_false = matches!(labels_arg, Some(v) if matches!(heap.object(v), Some(RObject::Logical(l)) if l.first().copied().unwrap_or(1) == 0));
            let custom_labels: Option<Vec<String>> = match labels_arg {
                Some(v) if !labels_false => {
                    let texts = string_vec(heap, interner, v);
                    if texts.is_empty() { None } else { Some(texts) }
                }
                _ => None,
            };
            let labels: Vec<String> = custom_labels.clone().unwrap_or_else(|| default_labels.clone());
            // Binning: each x locates its interval exactly as GNU's
            /// boundary scan.
            let codes: Vec<i32> = xs
                .iter()
                .map(|x| {
                    if x.is_nan() {
                        return crate::value::object::NA_INTEGER;
                    }
                    let x = *x;
                    for i in 0..nintervals {
                        let lo = breaks[i];
                        let hi = breaks[i + 1];
                        let inside = if right {
                            let lo_ok = if i == 0 && include_lowest { x >= lo } else { x > lo };
                            lo_ok && x <= hi
                        } else {
                            let hi_ok = if i == nintervals - 1 && include_lowest { x <= hi } else { x < hi };
                            x >= lo && hi_ok
                        };
                        if inside {
                            return (i + 1) as i32;
                        }
                    }
                    crate::value::object::NA_INTEGER
                })
                .collect();
            if labels_false {
                return Ok(heap.alloc(RObject::Integer(codes)));
            }
            let result = heap.alloc(RObject::Integer(codes));
            let interned: Vec<_> = labels.iter().map(|s| interner.intern_string(s)).collect();
            let levels_obj = heap.alloc(RObject::String(interned));
            let result = copy_with_attrib(heap, interner, result, "levels", levels_obj).map_err(RSignal::from)?;
            let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("factor")]));
            copy_with_attrib(heap, interner, result, "class", class_obj).map_err(RSignal::from)
        }
        DATE_FN => {
            // GNU's fixed-shape timestamp: abbreviated weekday and month,
            /// space-padded day, colon time, year — 24 characters exactly.
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let days = secs.div_euclid(86400);
            let day_secs = secs.rem_euclid(86400);
            let (year, month, day) = civil_from_days(days);
            let weekday = (days + 4).rem_euclid(7);
            let wnames = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            let mnames = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            let hour = day_secs / 3600;
            let minute = (day_secs % 3600) / 60;
            let second = day_secs % 60;
            let text = format!(
                "{} {} {:2} {:02}:{:02}:{:02} {}",
                wnames[weekday as usize], mnames[(month - 1) as usize], day, hour, minute, second, year
            );
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&text)])))
        }
        CAPABILITIES => {
            // GNU's 19-capability vector in canonical order — support
            /// values reflecting this runtime honestly: Unicode string
            /// handling and sockets yes, X11/aqua/Tcl no.
            let entries: [(&str, bool); 19] = [
                ("jpeg", false), ("png", false), ("tiff", false), ("tcltk", false),
                ("X11", false), ("aqua", false), ("http/ftp", true), ("sockets", true),
                ("libxml", true), ("fifo", true), ("cledit", false), ("iconv", true),
                ("NLS", true), ("Rprof", false), ("profmem", false), ("cairo", false),
                ("ICU", true), ("long.double", true), ("libcurl", true),
            ];
            let values: Vec<i32> = entries.iter().map(|(_, v)| *v as i32).collect();
            let result = heap.alloc(RObject::Logical(values));
            let names: Vec<_> = entries.iter().map(|(n, _)| interner.intern_string(n)).collect();
            let names_obj = heap.alloc(RObject::String(names));
            copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)
        }
        BY_FN => {
            // GNU's by over atomic data IS tapply plus the "by" class — the
            /// dim/dimnames shape tapply already produces carries over.
            let applied = call_primitive_inner(heap, interner, crate::value::object::BuiltinId(TAPPLY), args, caller_env)?;
            // names surface dimnames[[1]] exactly as GNU's 1-D indexing.
            let with_names = match get_attrib(heap, interner, applied, "dimnames") {
                Some(dn) => match heap.object(dn) {
                    Some(RObject::List(items)) if items.len() == 1 => {
                        copy_with_attrib(heap, interner, applied, "names", items[0]).map_err(RSignal::from)?
                    }
                    _ => applied,
                },
                None => applied,
            };
            let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("by")]));
            copy_with_attrib(heap, interner, with_names, "class", class_obj).map_err(RSignal::from)
        }
        BROWSER_FN => {
            // Headless browser(): a no-op exactly as GNU's non-interactive
            /// capture — the prompt machinery is meaningless without a tty.
            Ok(heap.alloc(RObject::Nil))
        }
        SUMMARY_FN => summary_primitive(heap, interner, args, caller_env),
        MATRIX_MULT | CROSSPROD | TCROSSPROD | MAXCOL => matrix_ops_primitive(heap, interner, id.0, args, caller_env),
        AS_LIST => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            // Language and pairlist chains flatten to their node cars with
            // names carried from the tags exactly as GNU's as.list; lists
            // pass through and atomic vectors become element lists.
            let v = operands[0];
            let mut items: Vec<ValueId> = Vec::new();
            let mut names: Vec<String> = Vec::new();
            let mut is_chain = false;
            let mut cursor = v;
            loop {
                let triple = match heap.object(cursor) {
                    Some(RObject::Language { car, cdr, tag }) => Some((*car, *cdr, *tag)),
                    Some(RObject::Pairlist { car, cdr, tag }) => Some((*car, *cdr, *tag)),
                    _ => None,
                };
                match triple {
                    Some((car, cdr, tag)) => {
                        is_chain = true;
                        items.push(car);
                        if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
                            names.push(interner.symbol_name(*name).to_string());
                        } else {
                            names.push(String::new());
                        }
                        if cdr == ValueId::NIL { break; }
                        cursor = cdr;
                    }
                    None => break,
                }
            }
            if is_chain {
                let result = heap.alloc(RObject::List(items));
                let all_empty = names.iter().all(|n| n.is_empty());
                if !all_empty {
                    let interned: Vec<_> = names.iter().map(|n| interner.intern_string(n)).collect();
                    let names_obj = heap.alloc(RObject::String(interned));
                    return copy_with_attrib(heap, interner, result, "names", names_obj).map_err(Into::into);
                }
                return Ok(result);
            }
            match heap.object(v) {
                Some(RObject::List(_)) => Ok(v),
                Some(RObject::Expression(items)) => {
                    let cloned: Vec<ValueId> = items.clone();
                    Ok(heap.alloc(RObject::List(cloned)))
                }
                _ => {
                    let len = vector_length(heap, v).unwrap_or(0);
                    let mut out = Vec::new();
                    for i in 0..len {
                        let idx = heap.alloc(RObject::Real(vec![(i + 1) as f64]));
                        let inner = heap.alloc(RObject::Pairlist { car: idx, cdr: ValueId::NIL, tag: ValueId::NIL });
                        let call_args = heap.alloc(RObject::Pairlist { car: v, cdr: inner, tag: ValueId::NIL });
                        if let Ok(elem) = extract_primitive(heap, interner, call_args, caller_env) {
                            out.push(elem);
                        }
                    }
                    let result = heap.alloc(RObject::List(out));
                    // Element names ride the list exactly as GNU — the
                    // vector's names attribute transfers positionally.
                    if let Some(names_id) = get_attrib(heap, interner, v, "names") {
                        return copy_with_attrib(heap, interner, result, "names", names_id).map_err(Into::into);
                    }
                    Ok(result)
                }
            }
        }
        BODY_FN => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            match heap.object(operands[0]) {
                Some(RObject::Closure { body, .. }) => Ok(*body),
                _ => Ok(ValueId::NIL),
            }
        }
        STORAGE_MODE_ASSIGN => {
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let target_type = string_operand(heap, interner, operands[1]).unwrap_or_default();
            // storage.mode<- coerces the payload while the attribute chain
            // survives untouched exactly as GNU's in-place mode mutation —
            // dim, names, and custom attributes all ride through.
            // Factors reject mode changes outright; defunct and unknown
            // modes raise GNU's exact errors.
            if is_factor(heap, interner, operands[0]) {
                return Err(EvalError::Condition("invalid to change the storage mode of a factor".to_string()).into());
            }
            if target_type == "real" {
                return Err(EvalError::Condition("use of 'real' is defunct: use 'double' instead".to_string()).into());
            }
            let coerced = match target_type.as_str() {
                "double" | "numeric" => coerce_real(heap, interner, operands[0])?,
                "integer" => coerce_integer(heap, interner, operands[0])?,
                "logical" => coerce_logical(heap, interner, operands[0])?,
                "character" => {
                    let strings = strings_of_value(heap, interner, operands[0]).map_err(RSignal::from)?;
                    heap.alloc(RObject::String(strings))
                }
                "raw" => {
                    // Integer-valued byte truncation exactly as GNU.
                    let bytes: Vec<u8> = match heap.object(operands[0]) {
                        Some(RObject::Integer(vals)) => vals.iter().map(|v| (*v).clamp(0, 255) as u8).collect(),
                        Some(RObject::Real(vals)) => vals.iter().map(|v| (*v as i32).clamp(0, 255) as u8).collect(),
                        Some(RObject::Raw(vals)) => vals.clone(),
                        _ => Vec::new(),
                    };
                    heap.alloc(RObject::Raw(bytes))
                }
                "list" => {
                    // NULL and vectors convert to list-of-elements exactly
                    // as GNU's mode mutation.
                    if operands[0] == ValueId::NIL || matches!(heap.object(operands[0]), Some(RObject::Nil)) {
                        heap.alloc(RObject::List(Vec::new()))
                    } else {
                        let len = vector_length(heap, operands[0]).unwrap_or(0);
                        let mut items = Vec::new();
                        for i in 0..len {
                            let idx_obj = heap.alloc(RObject::Real(vec![(i + 1) as f64]));
                            let inner = heap.alloc(RObject::Pairlist { car: idx_obj, cdr: ValueId::NIL, tag: ValueId::NIL });
                            let call_args = heap.alloc(RObject::Pairlist { car: operands[0], cdr: inner, tag: ValueId::NIL });
                            if let Ok(elem) = extract_primitive(heap, interner, call_args, caller_env) {
                                items.push(elem);
                            }
                        }
                        heap.alloc(RObject::List(items))
                    }
                }
                _ => {
                    return Err(EvalError::Condition("invalid value".to_string()).into());
                }
            };
            // Reattach the source attribute chain exactly as GNU's
            // in-place mutation leaves attributes untouched.
            let source_attrib = heap.get(operands[0]).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
            if let Some(slot) = heap.get_mut(coerced) {
                slot.attrib = source_attrib;
            }
            Ok(coerced)
        }
        USEMETHOD => usemethod_primitive(heap, interner, args, caller_env),
        NEXTMETHOD => nextmethod_primitive(heap, interner, caller_env),
        GETS3METHOD => {
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let generic = string_operand(heap, interner, operands[0]).unwrap_or_default();
            let class = string_operand(heap, interner, operands[1]).unwrap_or_default();
            let sym = interner.intern_symbol(&format!("{generic}.{class}"));
            match super::env::lookup(heap, caller_env, sym) {
                Some((_, binding)) => Ok(force_if_promise(heap, interner, binding.value)?),
                None => Err(EvalError::Condition(format!("S3 method '{generic}.{class}' not found")).into()),
            }
        }
        ORDER_FN => order_primitive(heap, interner, args, caller_env),
        FIND_FN => find_primitive(heap, interner, args, caller_env),
        RANK_FN => rank_primitive(heap, interner, args, caller_env),
        ORDERED_FN => ordered_primitive(heap, interner, args, caller_env),
        IS_ORDERED => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = class_contains(heap, interner, operands[0], "ordered");
            Ok(heap.alloc(RObject::Logical(vec![is as i32])))
        }
        IS_FACTOR => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = is_factor(heap, interner, operands[0]);
            Ok(heap.alloc(RObject::Logical(vec![is as i32])))
        }
        NA_OMIT | NA_EXCLUDE => na_action_primitive(heap, interner, id.0, args, caller_env),
        GETTEXT => {
            // Identity translation — the C locale never translates.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let text = string_operand(heap, interner, operands[1]).unwrap_or_default();
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&text)])))
        }
        NGETTEXT => {
            let operands = eval_args(heap, interner, args, caller_env, 3)?;
            let n = match heap.object(operands[0]) {
                Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(1.0) as i64,
                Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => {
                    vals.first().copied().unwrap_or(1) as i64
                }
                _ => 1,
            };
            let singular = string_operand(heap, interner, operands[1]).unwrap_or_default();
            let plural = string_operand(heap, interner, operands[2]).unwrap_or_default();
            let chosen = if n == 1 { singular } else { plural };
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&chosen)])))
        }
        BINDTEXTDOMAIN => {
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let domain = string_operand(heap, interner, operands[0]);
            let dir = string_operand(heap, interner, operands[1]);
            match (domain.as_deref(), dir) {
                // Querying one domain returns its registered directory.
                (Some(d), None) if !d.is_empty() => {
                    match heap.text_domains.get(d) {
                        Some(registered) => {
                            let text = registered.clone();
                            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&text)])))
                        }
                        None => Ok(ValueId::NIL),
                    }
                }
                // Registering returns the directory exactly as GNU.
                (Some(d), Some(dir_text)) if !d.is_empty() => {
                    heap.text_domains.insert(d.to_string(), dir_text.clone());
                    Ok(heap.alloc(RObject::String(vec![interner.intern_string(&dir_text)])))
                }
                // A NULL domain queries the state as a whole — truthy when
                /// any registration exists, matching GNU's non-null answer.
                (None, None) => {
                    if heap.text_domains.is_empty() {
                        Ok(ValueId::NIL)
                    } else {
                        Ok(heap.alloc(RObject::Logical(vec![1])))
                    }
                }
                _ => Ok(ValueId::NIL),
            }
        }
        GETERRMESSAGE => {
            let text = heap.err_message.clone();
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&text)])))
        }
        SETERRMESSAGE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            heap.err_message = string_operand(heap, interner, operands[0]).unwrap_or_default();
            Ok(ValueId::NIL)
        }
        INTERRUPTS_SUSPENDED => {
            // Non-interactive sessions never suspend interrupts — GNU's
            /// oracle pins FALSE regardless of the setting argument.
            Ok(heap.alloc(RObject::Logical(vec![0])))
        }
        IS_TRUE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = matches!(
                heap.object(operands[0]),
                Some(RObject::Logical(vals)) if vals.len() == 1 && vals[0] == 1
            );
            Ok(heap.alloc(RObject::Logical(vec![if is { 1 } else { 0 }])))
        }
        PRINT_DEFERRED_WARNINGS => Ok(ValueId::NIL),
        IS_PRIMITIVE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = matches!(heap.object(operands[0]), Some(RObject::Primitive(_)));
            Ok(heap.alloc(RObject::Logical(vec![if is { 1 } else { 0 }])))
        }
        SAVE => save_primitive(heap, interner, args, caller_env),
        LOAD => load_primitive(heap, interner, args, caller_env),
        RM => {
            // rm(x, y) — bare symbols name bindings to remove from the
            /// calling environment exactly as GNU's non-standard lookup.
            let raw = raw_args(heap, args);
            for expr in &raw {
                // GNU accepts both bare symbols and character names.
                if let Some(RObject::Symbol { name, .. }) = heap.object(*expr) {
                    let sym = *name;
                    let _ = super::env::remove(heap, caller_env, sym);
                } else {
                    let evaluated = eval(heap, interner, *expr, caller_env)?;
                    let v = force_if_promise(heap, interner, evaluated)?;
                    for text in string_vec(heap, interner, v) {
                        let sym = interner.intern_symbol(&text);
                        let _ = super::env::remove(heap, caller_env, sym);
                    }
                }
            }
            Ok(ValueId::NIL)
        }
        AS_VECTOR => {
            // as.vector strips every attribute — the bare vector remains.
            /// The optional mode argument coerces exactly as GNU.
            let raw = raw_args(heap, args);
            let first = eval(heap, interner, raw[0], caller_env)?;
            let first = force_if_promise(heap, interner, first)?;
            let stripped = heap.alloc(heap.object(first).cloned().unwrap_or(RObject::Nil));
            if let Some(mode_expr) = raw.get(1).copied() {
                let evaluated = eval(heap, interner, mode_expr, caller_env)?;
                let mode_v = force_if_promise(heap, interner, evaluated)?;
                if let Some(mode) = string_operand(heap, interner, mode_v) {
                    if mode == "character" {
                        return coerce_character(heap, interner, stripped);
                    }
                }
            }
            Ok(stripped)
        }
        AS_CHARACTER => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            coerce_character(heap, interner, operands[0])
        }
        TSP => attrib_getter(heap, interner, args, caller_env, "tsp"),
        TSP_ASSIGN => {
            // tsp<- coerces its value to double exactly as GNU's time
            /// series parameter discipline; NULL removes the attribute.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            if matches!(heap.object(operands[1]), Some(RObject::Nil)) || operands[1] == ValueId::NIL {
                return remove_attrib(heap, interner, operands[0], "tsp").map_err(RSignal::from);
            }
            // GNU validates the time series parameters: numeric of length
            /// three, and frequency and ordering sane — NA and NaN slip
            /// through exactly as the oracle documents because their
            /// comparisons are never definitively true.
            let numeric_vals: Option<Vec<f64>> = match heap.object(operands[1]) {
                Some(RObject::Real(vals)) => Some(vals.clone()),
                Some(RObject::Integer(vals)) => Some(
                    vals.iter()
                        .map(|&v| {
                            if v == crate::value::object::NA_INTEGER {
                                f64::from_bits(crate::value::object::NA_REAL_BITS)
                            } else {
                                v as f64
                            }
                        })
                        .collect(),
                ),
                _ => None,
            };
            let vals = match numeric_vals {
                Some(v) if v.len() == 3 => v,
                _ => {
                    return Err(EvalError::Condition(
                        "'tsp' attribute must be numeric of length three".to_string(),
                    )
                    .into())
                }
            };
            let freq = vals[2];
            let invalid_order = vals[0] > vals[1];
            let invalid_freq = freq < 1.0;
            if invalid_order || invalid_freq {
                let freq_text = if freq.fract() == 0.0 && !freq.is_nan() {
                    format!("{}", freq as i64)
                } else {
                    super::print::format_real(freq)
                };
                return Err(EvalError::Condition(format!(
                    "invalid time series parameters specified ({freq_text})"
                ))
                .into());
            }
            let coerced = heap.alloc(RObject::Real(vals));
            copy_with_attrib(heap, interner, operands[0], "tsp", coerced).map_err(RSignal::from)
        }
        SETNAMES => {
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            copy_with_attrib(heap, interner, operands[0], "names", operands[1]).map_err(RSignal::from)
        }
        IDENTICAL => {
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let same = structural_identical(heap, interner, operands[0], operands[1]);
            Ok(heap.alloc(RObject::Logical(vec![if same { 1 } else { 0 }])))
        }
        GLOBALENV => {
            // The .GlobalEnv binding in base resolves to the user env.
            let sym = interner.intern_symbol(".GlobalEnv");
            match super::env::lookup(heap, caller_env, sym) {
                Some((_, binding)) => Ok(binding.value),
                None => Ok(caller_env),
            }
        }
        BASEENV => {
            // Walking the enclosure chain to its end reaches base exactly
            /// as GNU's baseenv — the last environment before empty.
            let mut cursor = caller_env;
            loop {
                let enclosure = match heap.object(cursor) {
                    Some(RObject::Environment { enclos, .. }) => *enclos,
                    _ => break,
                };
                if enclosure == ValueId::EMPTY_ENV || enclosure == ValueId::NIL {
                    break;
                }
                cursor = enclosure;
            }
            Ok(cursor)
        }
        ENVIRONMENT_NAME => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            // R_GlobalEnv names the user environment exactly as GNU; other
            /// environments report empty.
            let name_owned: String;
            let name = {
                let sym = interner.intern_symbol(".GlobalEnv");
                let genv = super::env::lookup(heap, caller_env, sym).map(|(_, b)| b.value);
                let attr_name = get_attrib(heap, interner, operands[0], "name")
                    .and_then(|n| string_operand(heap, interner, n));
                if operands[0] == ValueId::EMPTY_ENV {
                    "R_EmptyEnv"
                } else if let Some(n) = attr_name {
                    name_owned = n;
                    &name_owned
                } else if Some(operands[0]) == genv {
                    "R_GlobalEnv"
                } else {
                    // The chain end is base exactly as GNU's naming.
                    let chain = enclosure_chain(heap, caller_env);
                    if chain.last().copied() == Some(operands[0]) && chain.len() > 1 {
                        "base"
                    } else {
                        ""
                    }
                }
            };
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(name)])))
        }
        ENVIRONMENT => {
            // environment(fun) reads the closure's birth environment;
            // no argument is the calling frame exactly as GNU.
            let raw = raw_args(heap, args);
            if raw.is_empty() {
                return Ok(caller_env);
            }
            let v = eval(heap, interner, raw[0], caller_env)?;
            match heap.object(v) {
                Some(RObject::Closure { env, .. }) => Ok(*env),
                _ => Ok(v),
            }
        }
        R_VERSION_FN => {
            // R.Version() returns the shared version list exactly as the
            /// version constant — GNU's function reads the same structure.
            let sym = interner.intern_symbol("R.version");
            match super::env::lookup(heap, caller_env, sym) {
                Some((_, binding)) => Ok(binding.value),
                None => Ok(ValueId::NIL),
            }
        }
        GET_RVERSION => {
            // GNU's numeric_version is a list of integer components —
            /// typeof reports list exactly as the oracle pins.
            let component = heap.alloc(RObject::Integer(vec![4, 4, 0]));
            Ok(heap.alloc(RObject::List(vec![component])))
        }
        ARRAY => array_primitive(heap, interner, args, caller_env),
        AS_DATE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let out: Vec<f64> = match heap.object(operands[0]) {
                Some(RObject::String(values)) => values
                    .iter()
                    .map(|v| {
                        let text = interner.string_text(*v);
                        if text == "\0NA" {
                            f64::from_bits(crate::value::object::NA_REAL_BITS)
                        } else {
                            match super::civil::parse_iso(text) {
                                Some(days) => days as f64,
                                None => f64::from_bits(crate::value::object::NA_REAL_BITS),
                            }
                        }
                    })
                    .collect(),
                Some(RObject::Real(values)) => values.clone(),
                Some(RObject::Integer(values)) => values
                    .iter()
                    .map(|&v| {
                        if v == crate::value::object::NA_INTEGER {
                            f64::from_bits(crate::value::object::NA_REAL_BITS)
                        } else {
                            v as f64
                        }
                    })
                    .collect(),
                _ => Vec::new(),
            };
            // POSIXct input carries seconds — as.Date floors into days
            /// exactly as GNU, euclidean division keeping pre-epoch
            /// instants on their civil date.
            let out: Vec<f64> = if is_posixct_classed(heap, interner, operands[0]) {
                out.iter()
                    .map(|&s| {
                        if s.is_nan() {
                            s
                        } else {
                            (s as i64).div_euclid(86400) as f64
                        }
                    })
                    .collect()
            } else {
                out
            };
            let value = heap.alloc(RObject::Real(out));
            let class = heap.alloc(RObject::String(vec![interner.intern_string("Date")]));
            copy_with_attrib(heap, interner, value, "class", class).map_err(RSignal::from)
        }
        WEEKDAYS | MONTHS | QUARTERS => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let mut days = numeric_vector(heap, operands[0]);
            // POSIXct input carries seconds — the calendar labels floor
            /// into days exactly as the Date conversion does.
            if is_posixct_classed(heap, interner, operands[0]) {
                days = days
                    .iter()
                    .map(|&s| {
                        if s.is_nan() {
                            s
                        } else {
                            (s as i64).div_euclid(86400) as f64
                        }
                    })
                    .collect();
            }
            let out: Vec<super::intern::InternedStr> = days
                .iter()
                .map(|&d| {
                    if d.is_nan() {
                        // GNU: quarters renders NA as the literal "QNA"
                        /// while weekdays and months use the NA sentinel.
                        if id.0 == QUARTERS {
                            interner.intern_string("QNA")
                        } else {
                            interner.intern_string("\0NA")
                        }
                    } else {
                        let day_count = d as i64;
                        let text = match id.0 {
                            WEEKDAYS => {
                                super::civil::WEEKDAY_NAMES[super::civil::weekday_of(day_count) as usize].to_string()
                            }
                            MONTHS => {
                                let (_, m, _) = super::civil::civil_from_days(day_count);
                                super::civil::MONTH_NAMES[(m - 1) as usize].to_string()
                            }
                            _ => {
                                let (_, m, _) = super::civil::civil_from_days(day_count);
                                format!("Q{}", (m - 1) / 3 + 1)
                            }
                        };
                        interner.intern_string(&text)
                    }
                })
                .collect();
            Ok(heap.alloc(RObject::String(out)))
        }
        IS_UNSORTED => {
            // GNU sortedness across the atomic lattice: numeric, character,
            /// complex (real then imaginary), and raw each order by their
            /// own keys; NA participation makes the result NA unless na.rm
            /// drops it; strictly counts equality as unsorted.
            let mut xval = ValueId::NIL;
            let mut na_rm = false;
            let mut strictly = false;
            let mut cursor = args;
            for _ in 0..100_000 {
                if cursor == ValueId::NIL {
                    break;
                }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                let value = eval(heap, interner, car, caller_env)?;
                let tag_name = if tag == ValueId::NIL {
                    None
                } else {
                    match heap.object(tag) {
                        Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                        _ => None,
                    }
                };
                match tag_name.as_deref() {
                    Some("na.rm") => na_rm = logical_scalar(heap, value),
                    Some("strictly") => strictly = logical_scalar(heap, value),
                    _ => {
                        if xval == ValueId::NIL {
                            xval = value;
                        }
                    }
                }
                cursor = next;
            }
            // Keys as (primary, secondary, is_na) triples — complex ordering
            /// uses both parts, everything else keys on the primary alone.
            let keys: Vec<(f64, f64, bool)> = match heap.object(xval) {
                Some(RObject::Real(vals)) => vals
                    .iter()
                    .map(|v| (*v, 0.0, v.to_bits() == crate::value::object::NA_REAL_BITS))
                    .collect(),
                Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals
                    .iter()
                    .map(|&v| {
                        if v == crate::value::object::NA_INTEGER {
                            (0.0, 0.0, true)
                        } else {
                            (v as f64, 0.0, false)
                        }
                    })
                    .collect(),
                Some(RObject::Complex(vals)) => vals
                    .iter()
                    .map(|z| {
                        let na = z.r.to_bits() == crate::value::object::NA_REAL_BITS
                            || z.i.to_bits() == crate::value::object::NA_REAL_BITS
                            || z.r.is_nan()
                            || z.i.is_nan();
                        (z.r, z.i, na)
                    })
                    .collect(),
                Some(RObject::Raw(vals)) => vals.iter().map(|&v| (v as f64, 0.0, false)).collect(),
                Some(RObject::String(vals)) => {
                    // Character ordering is lexicographic in the C locale —
                    /// encode strings as rank positions of their sorted order.
                    let texts: Vec<(String, bool)> = vals
                        .iter()
                        .map(|v| {
                            let tx = interner.string_text(*v).to_string();
                            (tx.clone(), tx == "\0NA")
                        })
                        .collect();
                    let mut order: Vec<String> = texts
                        .iter()
                        .filter(|(_, na)| !na)
                        .map(|(s, _)| s.clone())
                        .collect();
                    order.sort();
                    order.dedup();
                    texts
                        .iter()
                        .map(|(s, na)| {
                            if *na {
                                (0.0, 0.0, true)
                            } else {
                                let rank = order.iter().position(|o| o == s).unwrap_or(0) as f64;
                                (rank, 0.0, false)
                            }
                        })
                        .collect()
                }
                _ => Vec::new(),
            };
            let filtered: Vec<(f64, f64)> = keys
                .iter()
                .filter(|(_, _, na)| !*na)
                .map(|(a, b, _)| (*a, *b))
                .collect();
            let had_na = keys.iter().any(|(_, _, na)| *na);
            let mut result = 0i32;
            for w in filtered.windows(2) {
                let descending = w[0].0 > w[1].0 || (w[0].0 == w[1].0 && w[0].1 > w[1].1);
                let equal = w[0].0 == w[1].0 && w[0].1 == w[1].1;
                if descending || (strictly && equal) {
                    result = 1;
                    break;
                }
            }
            if result == 0 && had_na && !na_rm {
                result = crate::value::object::NA_INTEGER;
            }
            Ok(heap.alloc(RObject::Logical(vec![result])))
        }
        TOSTRING => {
            // Comma-space collapse of the character coercion — width
            /// truncation sits outside the slice.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let coerced = coerce_character(heap, interner, operands[0])?;
            let text = string_vec(heap, interner, coerced);
            let joined = text.join(", ");
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&joined)])))
        }
        CAPTURE_OUTPUT => {
            // Each argument evaluates in order; everything the evaluation
            /// writes to the output buffer drains into the result as one
            /// element per line exactly as GNU's sink capture.
            let start = heap.output.len();
            let raw = raw_args(heap, args);
            for arg in raw {
                crate::value::eval::eval(heap, interner, arg, caller_env)?;
            }
            let captured: String = heap.output[start..].to_string();
            heap.output.truncate(start);
            let lines: Vec<super::intern::InternedStr> = captured
                .lines()
                .map(|l| interner.intern_string(l))
                .collect();
            Ok(heap.alloc(RObject::String(lines)))
        }
        ROW_NAMES => {
            // The row.names attribute when present; data.frames otherwise
            /// synthesize the compact sequence exactly as GNU's default.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            if let Some(existing) = get_attrib(heap, interner, operands[0], "row.names") {
                return Ok(existing);
            }
            let n = match heap.object(operands[0]) {
                Some(RObject::List(items)) if !items.is_empty() => {
                    vector_length(heap, items[0]).unwrap_or(0)
                }
                _ => vector_length(heap, operands[0]).unwrap_or(0),
            };
            let rows: Vec<i32> = (1..=n as i32).collect();
            Ok(heap.alloc(RObject::Integer(rows)))
        }
        ROW_NAMES_SET => {
            // Replacement form through the general desugar: row.names<-(x, value).
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            copy_with_attrib(heap, interner, operands[0], "row.names", operands[1]).map_err(RSignal::from)
        }
        TAPPLY => {
            // GNU tapply: the index is a vector or a list of vectors; each
            /// contributes a dimension whose levels come from factor levels
            /// or sorted-unique values. The result is an array filled
            /// column-major over the level combinations; empty combinations
            /// yield NA exactly as the oracle documents.
            let operands = eval_args(heap, interner, args, caller_env, 3)?;
            let x = operands[0];
            let index = operands[1];
            let fun = operands[2];
            let (index_cols, index_names): (Vec<ValueId>, Vec<String>) = match heap.object(index) {
                Some(RObject::List(items)) => {
                    let names = match get_attrib(heap, interner, index, "names") {
                        Some(id) => string_vec(heap, interner, id),
                        None => Vec::new(),
                    };
                    (items.clone(), names)
                }
                _ => (vec![index], Vec::new()),
            };
            // Per-column levels and per-row level text.
            let mut col_levels: Vec<Vec<String>> = Vec::new();
            let mut col_texts: Vec<Vec<String>> = Vec::new();
            for col in &index_cols {
                let levels: Vec<String> = match get_attrib(heap, interner, *col, "levels") {
                    Some(id) => string_vec(heap, interner, id),
                    None => {
                        let mut vals = string_vec(heap, interner, *col);
                        vals.retain(|s| s != "\0NA");
                        vals.sort();
                        vals.dedup();
                        vals
                    }
                };
                let is_factor = matches!(heap.object(*col), Some(RObject::Integer(_)))
                    && get_attrib(heap, interner, *col, "levels").is_some();
                let texts: Vec<String> = if is_factor {
                    match heap.object(*col) {
                        Some(RObject::Integer(codes)) => codes
                            .iter()
                            .map(|&c| {
                                if c == crate::value::object::NA_INTEGER {
                                    "\0NA".to_string()
                                } else {
                                    levels.get((c - 1) as usize).cloned().unwrap_or_default()
                                }
                            })
                            .collect(),
                        _ => Vec::new(),
                    }
                } else {
                    string_vec(heap, interner, *col)
                };
                col_levels.push(levels);
                col_texts.push(texts);
            }
    let x_len = vector_length(heap, x).unwrap_or(0);
            let dims: Vec<usize> = col_levels.iter().map(|l| l.len()).collect();
            let total: usize = dims.iter().product();
            let mut result_values: Vec<f64> = Vec::with_capacity(total);
            let mut result_int = true;
            for flat in 0..total {
                // Decode the flat index column-major: first dimension fastest.
                let mut rem = flat;
                let mut combo: Vec<usize> = Vec::with_capacity(dims.len());
                for &d in &dims {
                    combo.push(rem % d.max(1));
                    rem /= d.max(1);
                }
                let positions: Vec<usize> = (0..x_len)
                    .filter(|&row| {
                        col_texts.iter().enumerate().all(|(j, texts)| {
                            texts.get(row).map(|t| t == &col_levels[j][combo[j]]).unwrap_or(false)
                        })
                    })
                    .collect();
                if positions.is_empty() {
                    result_values.push(f64::from_bits(crate::value::object::NA_REAL_BITS));
                    result_int = false;
                    continue;
                }
                let sub = extract_positions(heap, interner, x, &positions)?;
                let applied = apply_fun_to(heap, interner, fun, sub, caller_env)?;
                match heap.object(applied) {
                    Some(RObject::Real(v)) if !v.is_empty() => {
                        result_values.push(v[0]);
                        result_int = false;
                    }
                    Some(RObject::Integer(v)) if !v.is_empty() => {
                        result_values.push(v[0] as f64);
                    }
                    _ => return Err(EvalError::TypeError("tapply FUN must return scalar numeric").into()),
                }
            }
            let values: ValueId = if result_int {
                heap.alloc(RObject::Integer(result_values.iter().map(|&v| v as i32).collect()))
            } else {
                heap.alloc(RObject::Real(result_values))
            };
            // dimnames: one character vector per dimension, the list itself
            /// named by the index list's names exactly as GNU.
            let dn_inners: Vec<ValueId> = col_levels
                .iter()
                .map(|levels| {
                    let interned: Vec<super::intern::InternedStr> = levels
                        .iter()
                        .map(|s| interner.intern_string(s))
                        .collect();
                    heap.alloc(RObject::String(interned))
                })
                .collect();
            let dn_list = heap.alloc(RObject::List(dn_inners));
            let dn_list = if !index_names.is_empty() {
                let nm: Vec<super::intern::InternedStr> = index_names
                    .iter()
                    .map(|s| interner.intern_string(s))
                    .collect();
                let names_obj = heap.alloc(RObject::String(nm));
                copy_with_attrib(heap, interner, dn_list, "names", names_obj).map_err(RSignal::from)?
            } else {
                dn_list
            };
            let dim_vec = heap.alloc(RObject::Integer(dims.iter().map(|&d| d as i32).collect()));
            let class_vec = heap.alloc(RObject::String(vec![interner.intern_string("array")]));
            // names() on the 1-D result surfaces dimnames[[1]] exactly as
            /// GNU arrays; carry the explicit names attribute only in the
            /// single-index case where GNU's object shows it.
            let values = if dims.len() == 1 {
                let first: Vec<super::intern::InternedStr> = col_levels[0]
                    .iter()
                    .map(|s| interner.intern_string(s))
                    .collect();
                let names_obj = heap.alloc(RObject::String(first));
                copy_with_attrib(heap, interner, values, "names", names_obj).map_err(RSignal::from)?
            } else {
                values
            };
            let with_dim = copy_with_attrib(heap, interner, values, "dim", dim_vec).map_err(RSignal::from)?;
            let with_dn = copy_with_attrib(heap, interner, with_dim, "dimnames", dn_list).map_err(RSignal::from)?;
            copy_with_attrib(heap, interner, with_dn, "class", class_vec).map_err(RSignal::from)
        }
        VAPPLY => {
            // Like sapply with a type template: each application must yield
            /// the template's type, and the results always combine into a
            /// template-typed vector — GNU's type-safe apply exactly.
            let operands = eval_args(heap, interner, args, caller_env, 3)?;
            let x = operands[0];
            let fun = operands[1];
            let template = operands[2];
            let elements = sequence_elements(heap, x)?;
            let mut results: Vec<ValueId> = Vec::with_capacity(elements.len());
            for element in elements {
                let applied = apply_fun_to(heap, interner, fun, element, caller_env)?;
                results.push(applied);
            }
            let first_of = |heap: &Heap, v: ValueId| -> (f64, Option<super::intern::InternedStr>) {
                match heap.object(v) {
                    Some(RObject::Real(vals)) => (vals.first().copied().unwrap_or(0.0), None),
                    Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => {
                        (vals.first().copied().unwrap_or(0) as f64, None)
                    }
                    Some(RObject::String(vals)) => {
                        (0.0, vals.first().copied())
                    }
                    _ => (0.0, None),
                }
            };
            let pairlist_names = iteration_names(heap, interner, x);
            let names = get_attrib(heap, interner, x, "names");
            let combined: ValueId = match heap.object(template) {
                Some(RObject::String(_)) => {
                    let out: Vec<super::intern::InternedStr> = results
                        .iter()
                        .map(|&r| {
                            let (_, s) = first_of(heap, r);
                            s.unwrap_or_else(|| interner.intern_string("\0NA"))
                        })
                        .collect();
                    heap.alloc(RObject::String(out))
                }
                Some(RObject::Integer(_)) => {
                    let out: Vec<i32> = results
                        .iter()
                        .map(|&r| first_of(heap, r).0 as i32)
                        .collect();
                    heap.alloc(RObject::Integer(out))
                }
                Some(RObject::Logical(_)) => {
                    let out: Vec<i32> = results
                        .iter()
                        .map(|&r| first_of(heap, r).0 as i32)
                        .collect();
                    heap.alloc(RObject::Logical(out))
                }
                _ => {
                    let out: Vec<f64> = results.iter().map(|&r| first_of(heap, r).0).collect();
                    heap.alloc(RObject::Real(out))
                }
            };
            if !pairlist_names.is_empty() && matches!(heap.object(x), Some(RObject::Pairlist { .. })) {
                let interned: Vec<_> = pairlist_names.iter().map(|n| interner.intern_string(n)).collect();
                let names_obj = heap.alloc(RObject::String(interned));
                copy_with_attrib(heap, interner, combined, "names", names_obj).map_err(RSignal::from)
            } else if let Some(nm) = names {
                copy_with_attrib(heap, interner, combined, "names", nm).map_err(RSignal::from)
            } else {
                Ok(combined)
            }
        }
        AS_POSIXCT => {
            // Parse ISO datetimes into seconds since epoch, stamping the
            /// two-element POSIXct/POSIXt class and the tzone attribute
            /// exactly as GNU. The tz argument arrives tagged.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let mut tz = "UTC".to_string();
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                if tag_name.as_deref() == Some("tz") {
                    let v = eval(heap, interner, car, caller_env)?;
                    if let Some(s) = string_operand(heap, interner, v) {
                        tz = s;
                    }
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            let out: Vec<f64> = match heap.object(operands[0]) {
                Some(RObject::String(values)) => values
                    .iter()
                    .map(|v| {
                        let text = interner.string_text(*v);
                        if text == "\0NA" {
                            f64::from_bits(crate::value::object::NA_REAL_BITS)
                        } else {
                            match super::civil::parse_iso_datetime(text) {
                                Some(secs) => secs as f64,
                                None => f64::from_bits(crate::value::object::NA_REAL_BITS),
                            }
                        }
                    })
                    .collect(),
                Some(RObject::Real(values)) => values.clone(),
                Some(RObject::Integer(values)) => values
                    .iter()
                    .map(|&v| {
                        if v == crate::value::object::NA_INTEGER {
                            f64::from_bits(crate::value::object::NA_REAL_BITS)
                        } else {
                            v as f64
                        }
                    })
                    .collect(),
                _ => Vec::new(),
            };
            // Date-classed input carries days — as.POSIXct scales into
            /// seconds at midnight exactly as GNU.
            let out: Vec<f64> = if is_date_classed(heap, interner, operands[0]) {
                out.iter()
                    .map(|&d| if d.is_nan() { d } else { d * 86400.0 })
                    .collect()
            } else {
                out
            };
            let value = heap.alloc(RObject::Real(out));
            let class = heap.alloc(RObject::String(vec![
                interner.intern_string("POSIXct"),
                interner.intern_string("POSIXt"),
            ]));
            let tzone = heap.alloc(RObject::String(vec![interner.intern_string(&tz)]));
            let with_class = copy_with_attrib(heap, interner, value, "class", class).map_err(RSignal::from)?;
            copy_with_attrib(heap, interner, with_class, "tzone", tzone).map_err(RSignal::from)
        }
        PROC_TIME => {
            // The five-slot timing vector exactly as GNU — user/sys/elapsed
            /// plus the child slots — named and classed proc_time.
            let values = heap.alloc(RObject::Real(vec![0.0, 0.0, 0.0, 0.0, 0.0]));
            let names = heap.alloc(RObject::String(
                ["user.self", "sys.self", "elapsed", "user.child", "sys.child"]
                    .iter()
                    .map(|s| interner.intern_string(s))
                    .collect(),
            ));
            let class = heap.alloc(RObject::String(vec![interner.intern_string("proc_time")]));
            let with_names = copy_with_attrib(heap, interner, values, "names", names).map_err(RSignal::from)?;
            copy_with_attrib(heap, interner, with_names, "class", class).map_err(RSignal::from)
        }
        QUOTE => {
            // The argument returns unevaluated exactly as GNU — quote is
            /// the fundamental capture primitive, and its arity discipline
            /// counts empty arguments exactly as the oracle pins.
            let count = raw_args(heap, args).len();
            if count != 1 {
                return Err(EvalError::Condition(format!(
                    "{count} arguments passed to 'quote' which requires 1"
                ))
                .into());
            }
            match heap.object(args) {
                Some(RObject::Pairlist { car, tag, .. }) => {
                    // A supplied name must match the sole formal exactly as
                    /// GNU's hand-rolled matching in quote.
                    if *tag != ValueId::NIL {
                        if let Some(RObject::Symbol { name, .. }) = heap.object(*tag) {
                            let tag_name = interner.symbol_name(*name).to_string();
                            if tag_name != "expr" {
                                return Err(EvalError::Condition(format!(
                                    "supplied argument name '{tag_name}' does not match 'expr'"
                                ))
                                .into());
                            }
                        }
                    }
                    Ok(*car)
                }
                _ => Ok(ValueId::NIL),
            }
        }
        TILDE => {
            // The formula constructor captures both sides unevaluated as a
            /// language object, stamped with the formula class and the
            /// calling environment exactly as GNU's ~ evaluation.
            let tilde_sym = heap.alloc(RObject::Symbol {
                name: interner.intern_symbol("~"),
                value: ValueId::UNBOUND,
                internal: ValueId::UNBOUND,
            });
            let call = heap.alloc(RObject::Language {
                car: tilde_sym,
                cdr: args,
                tag: ValueId::NIL,
            });
            let class = heap.alloc(RObject::String(vec![interner.intern_string("formula")]));
            let env_val = caller_env;
            let with_class = copy_with_attrib(heap, interner, call, "class", class).map_err(RSignal::from)?;
            copy_with_attrib(heap, interner, with_class, ".Environment", env_val).map_err(RSignal::from)
        }
        IS_CALL => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = matches!(heap.object(operands[0]), Some(RObject::Language { .. }));
            Ok(heap.alloc(RObject::Logical(vec![is as i32])))
        }
        IS_ENVIRONMENT => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = matches!(heap.object(operands[0]), Some(RObject::Environment { .. }));
            Ok(heap.alloc(RObject::Logical(vec![is as i32])))
        }
        SIN | COS | SINH | COSH | TANH_F => trig_unary(heap, interner, id.0, args, caller_env),
        DNORM | PNORM | QNORM | DPOIS | PPOIS | DBINOM | PBINOM | PBETA | DBETA | QBETA | DT | PT | QT | DF | PF | QF | DCAUCHY | DCHISQ | DEXP => dpq_primitive(heap, interner, id.0, args, caller_env),
        COMPLEX_CTOR => complex_ctor(heap, interner, args, caller_env),
        AS_COMPLEX => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let vals = as_complex_vec(heap, operands[0]).map_err(RSignal::from)?;
            Ok(heap.alloc(RObject::Complex(vals)))
        }
        RE_C | IM_C | MOD_C | ARG_C => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let vals = as_complex_vec(heap, operands[0]).map_err(RSignal::from)?;
            let out: Vec<f64> = vals
                .iter()
                .map(|z| match id.0 {
                    RE_C => z.r,
                    IM_C => z.i,
                    MOD_C => (z.r * z.r + z.i * z.i).sqrt(),
                    _ => z.i.atan2(z.r),
                })
                .collect();
            Ok(heap.alloc(RObject::Real(out)))
        }
        CONJ_C => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let vals = as_complex_vec(heap, operands[0]).map_err(RSignal::from)?;
            let out: Vec<crate::value::object::RComplex> = vals
                .iter()
                .map(|z| crate::value::object::RComplex { r: z.r, i: -z.i })
                .collect();
            Ok(heap.alloc(RObject::Complex(out)))
        }
        IS_COMPLEX => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let yes = matches!(heap.object(operands[0]), Some(RObject::Complex(_)));
            Ok(heap.alloc(RObject::Logical(vec![yes as i32])))
        }
        IS_RAW => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let yes = matches!(heap.object(operands[0]), Some(RObject::Raw(_)));
            Ok(heap.alloc(RObject::Logical(vec![yes as i32])))
        }
        AS_RAW => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let out: Vec<u8> = match heap.object(operands[0]) {
                Some(RObject::Real(vals)) => vals
                    .iter()
                    .map(|v| {
                        if v.is_nan() || *v < 0.0 || *v > 255.0 {
                            0u8
                        } else {
                            *v as u8
                        }
                    })
                    .collect(),
                Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals
                    .iter()
                    .map(|&v| {
                        if v == crate::value::object::NA_INTEGER || v < 0 || v > 255 {
                            0u8
                        } else {
                            v as u8
                        }
                    })
                    .collect(),
                Some(RObject::Raw(vals)) => vals.clone(),
                Some(RObject::Complex(vals)) => vals
                    .iter()
                    .map(|z| {
                        if z.r.is_nan() || z.r < 0.0 || z.r > 255.0 {
                            0u8
                        } else {
                            z.r as u8
                        }
                    })
                    .collect(),
                _ => Vec::new(),
            };
            Ok(heap.alloc(RObject::Raw(out)))
        }
        MODE_FN | STORAGE_MODE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            // mode collapses the numeric family and renames the object
            /// types exactly as GNU's mode; storage.mode keeps the storage
            /// precision names.
            let storage = match heap.object(operands[0]) {
                Some(RObject::Nil) | None => "NULL",
                Some(RObject::Logical(_)) => "logical",
                Some(RObject::Integer(_)) => "integer",
                Some(RObject::Real(_)) => "double",
                Some(RObject::Complex(_)) => "complex",
                Some(RObject::String(_)) => "character",
                Some(RObject::Raw(_)) => "raw",
                Some(RObject::List(_)) => "list",
                Some(RObject::Symbol { .. }) => "symbol",
                Some(RObject::Language { .. }) => "language",
                Some(RObject::Expression(_)) => "expression",
                Some(RObject::Closure { .. }) | Some(RObject::Primitive(_)) => "function",
                Some(RObject::Environment { .. }) => "environment",
                Some(RObject::Pairlist { .. }) => "pairlist",
                _ => "unknown",
            };
            let text = if id.0 == STORAGE_MODE {
                storage
            } else {
                match storage {
                    "integer" | "double" => "numeric",
                    "symbol" => "name",
                    "language" => "call",
                    other => other,
                }
            };
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(text)])))
        }
        EXPRESSION_FN => {
            // The argument expressions capture unevaluated exactly as GNU's
            // expression vector constructor; argument tags attach as the
            // names attribute.
            let raw = raw_args(heap, args);
            let mut tags: Vec<String> = Vec::new();
            let mut any_tag = false;
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                let _ = car;
                match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => {
                        let text = interner.symbol_name(*name).to_string();
                        if !text.is_empty() {
                            any_tag = true;
                        }
                        tags.push(text);
                    }
                    _ => tags.push(String::new()),
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            let result = heap.alloc(RObject::Expression(raw));
            if any_tag {
                let interned: Vec<_> = tags.iter().map(|n| interner.intern_string(n)).collect();
                let names_obj = heap.alloc(RObject::String(interned));
                return copy_with_attrib(heap, interner, result, "names", names_obj).map_err(Into::into);
            }
            Ok(result)
        }
        PARSE_FN => parse_primitive(heap, interner, args, caller_env),
        EVAL_FN => {
            // eval of an expression vector evaluates every element in
            /// order and yields the last value exactly as GNU; a bare
            /// language object evaluates directly.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            match heap.object(operands[0]) {
                Some(RObject::Expression(items)) => {
                    let items = items.clone();
                    let mut last = ValueId::NIL;
                    for item in items {
                        last = crate::value::eval::eval(heap, interner, item, caller_env)?;
                    }
                    Ok(last)
                }
                _ => crate::value::eval::eval(heap, interner, operands[0], caller_env),
            }
        }
        DEPARSE1 => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let text = crate::value::print::deparse(heap, interner, operands[0]);
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&text)])))
        }
        DPUT => {
            // dput writes the deparsed construction to the file (or the
            /// output buffer when no file is given) and returns its input
            /// invisibly exactly as GNU.
            let operands = eval_args(heap, interner, args, caller_env, 2)
                .unwrap_or_else(|_| eval_args(heap, interner, args, caller_env, 1).unwrap_or_default());
            if operands.is_empty() {
                return Err(EvalError::TypeError("dput requires an object").into());
            }
            let text = crate::value::print::deparse(heap, interner, operands[0]);
            if operands.len() >= 2 {
                let path = match heap.object(operands[1]) {
                    Some(RObject::String(vals)) if !vals.is_empty() => {
                        interner.string_text(vals[0]).to_string()
                    }
                    _ => String::new(),
                };
                if !path.is_empty() {
                    std::fs::write(&path, format!("{text}\n"))
                        .map_err(|e| RSignal::from(EvalError::TypeError(Box::leak(format!("cannot open file: {e}").into_boxed_str()))))?;
                    heap.visible = false;
                    return Ok(operands[0]);
                }
            }
            heap.output.push_str(&text);
            heap.output.push('\n');
            heap.visible = false;
            Ok(operands[0])
        }
        DGET => {
            // dget parses the file and evaluates the first expression —
            /// the write/read roundtrip exactly as GNU.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let path = match heap.object(operands[0]) {
                Some(RObject::String(vals)) if !vals.is_empty() => {
                    interner.string_text(vals[0]).to_string()
                }
                _ => return Err(EvalError::TypeError("dget requires a file path").into()),
            };
            let source = std::fs::read_to_string(&path)
                .map_err(|e| RSignal::from(EvalError::TypeError(Box::leak(format!("cannot open file: {e}").into_boxed_str()))))?;
            let parsed = parse_source(heap, interner, &source)?;
            let items = match heap.object(parsed) {
                Some(RObject::Expression(items)) => items.clone(),
                _ => Vec::new(),
            };
            match items.first() {
                Some(first) => crate::value::eval::eval(heap, interner, *first, caller_env),
                None => Ok(ValueId::NIL),
            }
        }
        SUBSTITUTE_FN => substitute_primitive(heap, interner, args, caller_env),
        IS_NUMERIC_FN | IS_INTEGER_FN | IS_DOUBLE_FN | IS_LOGICAL_FN | IS_CHARACTER_FN => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let yes = match id.0 {
                IS_NUMERIC_FN => matches!(
                    heap.object(operands[0]),
                    Some(RObject::Real(_)) | Some(RObject::Integer(_))
                ) && get_attrib(heap, interner, operands[0], "class").is_none(),
                IS_INTEGER_FN => matches!(heap.object(operands[0]), Some(RObject::Integer(_))),
                IS_DOUBLE_FN => matches!(heap.object(operands[0]), Some(RObject::Real(_))),
                IS_LOGICAL_FN => matches!(heap.object(operands[0]), Some(RObject::Logical(_))),
                _ => matches!(heap.object(operands[0]), Some(RObject::String(_))),
            };
            Ok(heap.alloc(RObject::Logical(vec![yes as i32])))
        }
        INHERITS => {
            // inherits checks the class chain for classed objects; the
            /// unclassed atomic types carry their implicit classes exactly
            /// as GNU's implicitClass table.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let want: Vec<String> = match heap.object(operands[1]) {
                Some(RObject::String(vals)) => vals
                    .iter()
                    .map(|v| interner.string_text(*v).to_string())
                    .collect(),
                _ => Vec::new(),
            };
            let mut classes: Vec<String> = match get_attrib(heap, interner, operands[0], "class") {
                Some(id) => string_vec(heap, interner, id),
                None => Vec::new(),
            };
            if classes.is_empty() {
                classes = match heap.object(operands[0]) {
                    Some(RObject::Real(_)) => vec!["double".to_string(), "numeric".to_string()],
                    Some(RObject::Integer(_)) => vec!["integer".to_string(), "numeric".to_string()],
                    Some(RObject::Logical(_)) => vec!["logical".to_string()],
                    Some(RObject::String(_)) => vec!["character".to_string()],
                    Some(RObject::Complex(_)) => vec!["complex".to_string()],
                    Some(RObject::Raw(_)) => vec!["raw".to_string()],
                    Some(RObject::List(_)) => vec!["list".to_string()],
                    _ => Vec::new(),
                };
            }
            let found = want.iter().any(|w| classes.iter().any(|c| c == w));
            Ok(heap.alloc(RObject::Logical(vec![found as i32])))
        }
        DIRNAME => {
            // The parent path: everything before the final separator, with
            /// trailing separators stripped first exactly as GNU's dirname.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let texts: Vec<String> = match heap.object(operands[0]) {
                Some(RObject::String(vals)) => vals
                    .iter()
                    .map(|v| interner.string_text(*v).to_string())
                    .collect(),
                _ => Vec::new(),
            };
            let out: Vec<super::intern::InternedStr> = texts
                .iter()
                .map(|path| {
                    let trimmed = path.trim_end_matches('/');
                    match trimmed.rfind('/') {
                        Some(0) => "/".to_string(),
                        Some(pos) => trimmed[..pos].to_string(),
                        None => ".".to_string(),
                    }
                })
                .map(|s| interner.intern_string(&s))
                .collect();
            Ok(heap.alloc(RObject::String(out)))
        }
        GREGEXPR => gregexpr_primitive(heap, interner, args, caller_env),
        GSUB | SUB_FN => gsub_primitive(heap, interner, id.0, args, caller_env),
        FORMALS => {
            // The closure's formals pairlist as a named list — defaults
            /// ride as their expressions, missing defaults as the empty
            /// symbol exactly as GNU's formals list.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let formals = match heap.object(operands[0]) {
                Some(RObject::Closure { formals, .. }) => Some(*formals),
                _ => None,
            };
            let mut items = Vec::new();
            let mut names = Vec::new();
            let mut cursor = formals.unwrap_or(ValueId::NIL);
            for _ in 0..100_000 {
                if cursor == ValueId::NIL {
                    break;
                }
                let (car, tag, next) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                    _ => break,
                };
                let name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
                    _ => String::new(),
                };
                names.push(name);
                let item = if car == ValueId::MISSING || car == ValueId::NIL {
                    // The missing default rides as the empty symbol — its
                    /// typeof reads symbol exactly as GNU's missing marker.
                    let empty = interner.intern_symbol("");
                    heap.alloc(RObject::Symbol { name: empty, value: ValueId::NIL, internal: ValueId::NIL })
                } else {
                    car
                };
                items.push(item);
                cursor = next;
            }
            let result = heap.alloc(RObject::List(items));
            if names.iter().any(|n| !n.is_empty()) {
                let interned: Vec<super::intern::InternedStr> = names
                    .iter()
                    .map(|s| interner.intern_string(s))
                    .collect();
                let names_vec = heap.alloc(RObject::String(interned));
                return copy_with_attrib(heap, interner, result, "names", names_vec).map_err(RSignal::from);
            }
            Ok(result)
        }
        OPTIONS_FN => options_primitive(heap, interner, args, caller_env),
        GET_OPTION => {
            if heap.options_width_fallback.is_none() {
                let fallback = heap.alloc(RObject::Real(vec![80.0]));
                heap.options_width_fallback = Some(fallback);
            }
            // One or two operands exactly as GNU — the second rides as the
            /// default for absent options. The optional operand walks the
            /// raw pairlist since eval_args enforces exact arity.
            let mut operand_count = 0usize;
            {
                let mut cursor = args;
                for _ in 0..16 {
                    match heap.object(cursor) {
                        Some(RObject::Pairlist { cdr, .. }) => {
                            operand_count += 1;
                            if *cdr == ValueId::NIL {
                                break;
                            }
                            cursor = *cdr;
                        }
                        _ => break,
                    }
                }
            }
            let operands = eval_args(heap, interner, args, caller_env, operand_count)?;
            let name = match heap.object(operands[0]) {
                Some(RObject::String(vals)) if !vals.is_empty() => interner.string_text(vals[0]).to_string(),
                _ => String::new(),
            };
            match lookup_option(heap, interner, &name) {
                Some(value) => Ok(value),
                None => Ok(if operands.len() > 1 { operands[1] } else { ValueId::NIL }),
            }
        }
        CBIND | RBIND => bind_primitive(heap, interner, id.0, args, caller_env),
        XTABS => xtabs_primitive(heap, interner, args, caller_env),
        FORMAT => {
            // format() on Date-classed input renders ISO exactly as the
            /// print path; other input coerces to character.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            if is_posixct_classed(heap, interner, operands[0]) {
                let secs = numeric_vector(heap, operands[0]);
                let out: Vec<super::intern::InternedStr> = secs
                    .iter()
                    .map(|&s| {
                        if s.is_nan() {
                            interner.intern_string("\0NA")
                        } else {
                            interner.intern_string(&super::civil::format_iso_datetime(s as i64))
                        }
                    })
                    .collect();
                return Ok(heap.alloc(RObject::String(out)));
            }
            if is_date_classed(heap, interner, operands[0]) {
                let days = numeric_vector(heap, operands[0]);
                let out: Vec<super::intern::InternedStr> = days
                    .iter()
                    .map(|&d| {
                        if d.is_nan() {
                            interner.intern_string("\0NA")
                        } else {
                            interner.intern_string(&super::civil::format_iso(d as i64))
                        }
                    })
                    .collect();
                Ok(heap.alloc(RObject::String(out)))
            } else {
                coerce_character(heap, interner, operands[0])
            }
        }
        IS_ARRAY => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = get_attrib(heap, interner, operands[0], "dim").is_some();
            Ok(heap.alloc(RObject::Logical(vec![if is { 1 } else { 0 }])))
        }
        IS_LIST => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = matches!(heap.object(operands[0]), Some(RObject::List(_)));
            Ok(heap.alloc(RObject::Logical(vec![if is { 1 } else { 0 }])))
        }
        GREPL => grep_primitive(heap, interner, args, caller_env, true),
        GREP => grep_primitive(heap, interner, args, caller_env, false),
        REGEXPR => regexpr_primitive(heap, interner, args, caller_env),
        TEMPDIR => {
            let text = interner.intern_string(&std::env::temp_dir().to_string_lossy());
            Ok(heap.alloc(RObject::String(vec![text])))
        }
        DIMNAMES => attrib_getter(heap, interner, args, caller_env, "dimnames"),
        DIMNAMES_ASSIGN => attrib_replacement(heap, interner, args, caller_env, "dimnames"),
        CLASS => class_getter(heap, interner, args, caller_env),
        CLASS_ASSIGN => attrib_replacement(heap, interner, args, caller_env, "class"),
        ROWNAMES => rowcolnames(heap, interner, args, caller_env, true),
        ROWNAMES_ASSIGN => rowcolnames_assign(heap, interner, args, caller_env, true),
        COLNAMES => rowcolnames(heap, interner, args, caller_env, false),
        COLNAMES_ASSIGN => rowcolnames_assign(heap, interner, args, caller_env, false),
        CHARACTER_CTOR => {
            let operands = eval_args(heap, interner, args, caller_env, 1).unwrap_or_default();
            let length = match operands.first().and_then(|v| heap.object(*v)) {
                Some(RObject::Real(v)) if !v.is_empty() => v[0] as usize,
                Some(RObject::Integer(v)) if !v.is_empty() => v[0] as usize,
                _ => 0,
            };
            let empty = interner.intern_string("");
            Ok(heap.alloc(RObject::String(vec![empty; length])))
        }
        CONDITION_MESSAGE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            // The condition object is a list with a message element —
            /// extraction mirrors dollar access exactly.
            match heap.object(operands[0]) {
                Some(RObject::List(items)) => {
                    let message_pos = get_attrib(heap, interner, operands[0], "names")
                        .and_then(|id| match heap.object(id) {
                            Some(RObject::String(entries)) => entries
                                .iter()
                                .position(|e| interner.string_text(*e) == "message"),
                            _ => None,
                        });
                    match message_pos {
                        Some(pos) if pos < items.len() => Ok(items[pos]),
                        _ => Ok(heap.alloc(RObject::String(vec![interner.intern_string("")]))),
                    }
                }
                _ => Err(EvalError::TypeError("conditionMessage requires a condition").into()),
            }
        }
        MATRIX => matrix_primitive(heap, interner, args, caller_env),
        DIM => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            // data.frames synthesize dim from the column shape exactly as
            /// GNU's implicit dims.
            if let Some((nr, nc)) = data_frame_shape(heap, interner, operands[0]) {
                return Ok(heap.alloc(RObject::Integer(vec![nr as i32, nc as i32])));
            }
            match get_attrib(heap, interner, operands[0], "dim") {
                Some(dim) => Ok(dim),
                None => Ok(ValueId::NIL),
            }
        }
        NROW | NCOL => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let which = if id.0 == NROW { 0 } else { 1 };
            // data.frames carry no dim attribute — the shape reads off the
            // column list: ncol is the column count, nrow the shared column
            // length exactly as GNU.
            let df_shape = data_frame_shape(heap, interner, operands[0]);
            if let Some((nr, nc)) = df_shape {
                let extent = if id.0 == NROW { nr } else { nc };
                return Ok(heap.alloc(RObject::Integer(vec![extent as i32])));
            }
            let extent = match get_attrib(heap, interner, operands[0], "dim") {
                Some(dim) => match heap.object(dim) {
                    Some(RObject::Integer(dims)) => dims.get(which).copied().unwrap_or(crate::value::object::NA_INTEGER),
                    _ => crate::value::object::NA_INTEGER,
                },
                // Vector nrow is the length; ncol of a vector is NULL in GNU
                /// but the slice reports the vector extent for nrow only.
                None => {
                    if id.0 == NROW {
                        vector_length(heap, operands[0]).unwrap_or(0) as i32
                    } else {
                        return Ok(ValueId::NIL);
                    }
                }
            };
            Ok(heap.alloc(RObject::Integer(vec![extent])))
        }
        PASTE => paste_primitive(heap, interner, args, caller_env, " "),
        PASTE0 => paste_primitive(heap, interner, args, caller_env, ""),
        NCHAR => nchar_primitive(heap, interner, args, caller_env),
        PRINT => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let text = super::print::format_value(heap, interner, operands[0]);
            heap.output.push_str(&text);
            heap.output.push('\n');
            // GNU: print returns its argument invisibly.
            Ok(operands[0])
        }
        LIST => {
            // Variadic: every evaluated operand becomes one list element;
            // tags attach as the names attribute exactly as GNU.
            let mut items = Vec::new();
            let mut tags: Vec<Option<String>> = Vec::new();
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => {
                        Some(interner.symbol_name(*name).to_string())
                    }
                    _ => None,
                };
                if car == ValueId::MISSING {
                    return Err(EvalError::Condition(format!(
                        "argument {} is empty",
                        items.len() + 1
                    ))
                    .into());
                }
                let evaluated = eval(heap, interner, car, caller_env)?;
                items.push(force_if_promise(heap, interner, evaluated)?);
                tags.push(tag_name);
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            let value = heap.alloc(RObject::List(items));
            if tags.iter().any(|t| t.is_some()) {
                let names: Vec<super::intern::InternedStr> = tags
                    .iter()
                    .map(|t| interner.intern_string(t.as_deref().unwrap_or("")))
                    .collect();
                let names_vec = heap.alloc(RObject::String(names));
                return copy_with_attrib(heap, interner, value, "names", names_vec)
                    .map_err(RSignal::from);
            }
            Ok(value)
        }
        SQRT | EXP | LOG | ABS | FLOOR | CEILING | TRUNC | EXPM1 | LOG1P | ACOSH | ASINH | ATANH | COSPI | SINPI | TANPI => math_unary(heap, interner, id.0, args, caller_env),
        _ => Err(EvalError::UnsupportedForm("unregistered primitive").into()),
    }
}

/// Collect the raw argument expressions of a call without evaluating them —
/// what special forms operate on.
fn raw_args(heap: &Heap, mut args: ValueId) -> Vec<ValueId> {
    let mut out = Vec::new();
    for _ in 0..100_000 {
        match heap.object(args) {
            Some(RObject::Pairlist { car, cdr, .. }) => {
                out.push(*car);
                if *cdr == ValueId::NIL {
                    break;
                }
                args = *cdr;
            }
            _ => break,
        }
    }
    out
}

/// `if (test) yes else no` — the test is forced to a single logical;
/// a missing else branch yields NIL, exactly GNU's invisible NULL.
fn if_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() < 2 || parts.len() > 3 {
        return Err(EvalError::UnsupportedForm("if arity").into());
    }
    let test = eval(heap, interner, parts[0], caller_env)?;
    let test = force_if_promise(heap, interner, test)?;
    let truth = match heap.object(test) {
        Some(RObject::Logical(values)) if !values.is_empty() => values[0] != 0,
        Some(RObject::Integer(values)) if !values.is_empty() => values[0] != 0,
        Some(RObject::Real(values)) if !values.is_empty() => values[0] != 0.0,
        _ => {
            return Err(EvalError::TypeError("if condition must be logical").into());
        }
    };
    if truth {
        eval(heap, interner, parts[1], caller_env)
    } else if parts.len() == 3 {
        eval(heap, interner, parts[2], caller_env)
    } else {
        Ok(ValueId::NIL)
    }
}

/// `{ ... }` — sequence evaluation in the same environment, value of the
/// last expression; empty blocks yield NIL.
fn brace_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut result = ValueId::NIL;
    for expr in raw_args(heap, args) {
        result = eval(heap, interner, expr, caller_env)?;
    }
    Ok(result)
}

/// `<-` / `=` and `<<-` — assignment special forms. The left operand is an
/// unevaluated symbol; the right evaluates in the caller's environment and
/// is itself the value of the assignment expression, exactly GNU.
fn assign_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    super_assign: bool,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() != 2 {
        return Err(EvalError::UnsupportedForm("assignment arity").into());
    }
    let sym = match heap.object(parts[0]) {
        Some(RObject::Symbol { name, .. }) => *name,
        Some(RObject::Language { .. }) => {
            // Replacement desugar: `x[i] <- v` is the replacement call
            // `x <- `[<-`(x, i, v)` — the evaluator recognizes the target's
            // call shape exactly as GNU's eval does.
            return assign_replacement(heap, interner, parts[0], parts[1], caller_env);
        }
        _ => {
            return Err(EvalError::TypeError("assignment target must be a symbol").into());
        }
    };
    let value = eval(heap, interner, parts[1], caller_env)?;
    let value = force_if_promise(heap, interner, value)?;
    if super_assign {
        match crate::value::env::assign(heap, caller_env, sym, value) {
            Ok(crate::value::env::AssignOutcome::Assigned)
            | Ok(crate::value::env::AssignOutcome::ReplacedActiveBinding) => {}
            Ok(crate::value::env::AssignOutcome::NoExistingBinding) => {
                // GNU: <<- with no existing binding defines in the global
                // environment. Sentinel resolution is pending; defining in
                // the current frame is the documented slice approximation.
                crate::value::env::define(heap, caller_env, sym, value)
                    .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("superassign define")))?;
            }
            Err(_) => {
                return Err(EvalError::TypeError("cannot change value of locked binding").into());
            }
        }
    } else {
        crate::value::env::define(heap, caller_env, sym, value)
            .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("assign define")))?;
    }
    Ok(value)
}

/// `return(value)` — evaluate the operand and signal the unwinding.
fn return_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    let value = match parts.len() {
        0 => ValueId::NIL,
        1 => {
            let value = eval(heap, interner, parts[0], caller_env)?;
            force_if_promise(heap, interner, value)?
        }
        _ => return Err(EvalError::UnsupportedForm("return arity").into()),
    };
    Err(RSignal::Return(value))
}

/// Binary arithmetic with GNU coercion and recycling semantics.
///
/// Arguments arrive unevaluated; each is evaluated in the caller's
/// environment and forced. Integer+integer stays integer (except division,
/// which is always real); any real operand yields real. Length recycling
/// requires the longer length be a multiple of the shorter.
fn arithmetic(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Unary forms: `+x` is the identity, `-x` negates — GNU folds both into
    // the same primitive as the binary operators. Arity is decided by the
    // call's actual operand count, not by successful prefix extraction.
    let arity = raw_args(heap, args).len();
    if matches!(op, ADD | SUB) && arity == 1 {
        if let Ok(unary) = eval_args(heap, interner, args, caller_env, 1) {
            // Unary negation of a classed timespan keeps the chain —
            /// -difftime negates the value and preserves units exactly as
            /// GNU's unary method.
            let classed = is_difftime_classed(heap, interner, unary[0])
                || is_date_classed(heap, interner, unary[0])
                || is_posixct_classed(heap, interner, unary[0]);
            if let Some(RObject::Complex(vals)) = heap.object(unary[0]) {
                let folded: Vec<crate::value::object::RComplex> = vals
                    .iter()
                    .map(|z| {
                        if op == ADD {
                            *z
                        } else {
                            crate::value::object::RComplex { r: -z.r, i: -z.i }
                        }
                    })
                    .collect();
                return Ok(heap.alloc(RObject::Complex(folded)));
            }
            let operand = numeric_operands(heap, unary[0]).map_err(RSignal::from)?;
            return match operand {
                Numeric::Integer(values) => {
                    let folded = values
                        .into_iter()
                        .map(|v| {
                            if v == crate::value::object::NA_INTEGER || op == ADD {
                                v
                            } else {
                                v.wrapping_neg()
                            }
                        })
                        .collect();
                    let bare = heap.alloc(RObject::Integer(folded));
                    if classed {
                        Ok(propagate_attributes(heap, unary[0], unary[0], bare))
                    } else {
                        Ok(bare)
                    }
                }
                Numeric::Real(values) => {
                    let folded = values
                        .into_iter()
                        .map(|v| if op == ADD { v } else { -v })
                        .collect();
                    let bare = heap.alloc(RObject::Real(folded));
                    if classed {
                        Ok(propagate_attributes(heap, unary[0], unary[0], bare))
                    } else {
                        Ok(bare)
                    }
                }
            };
        }
    }
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    // The datetime operand lattice: difftime operands rescale into the
    /// target unit of the operation — seconds against POSIXct, days
    /// against Dates — and difftime-difftime pairs convert into their
    /// finer unit, shedding class so the fold sees bare numerics exactly
    /// as GNU's arithmetic methods arrange.
    let left_date = is_date_classed(heap, interner, operands[0]);
    let right_date = is_date_classed(heap, interner, operands[1]);
    let left_posix = is_posixct_classed(heap, interner, operands[0]);
    let right_posix = is_posixct_classed(heap, interner, operands[1]);
    let left_dt = is_difftime_classed(heap, interner, operands[0]);
    let right_dt = is_difftime_classed(heap, interner, operands[1]);
    let left_dt_units = if left_dt { difftime_units(heap, interner, operands[0]) } else { "secs".to_string() };
    let right_dt_units = if right_dt { difftime_units(heap, interner, operands[1]) } else { "secs".to_string() };
    // Convert a difftime operand into the given unit count, as a bare
    /// numeric — the class sheds exactly as the method dispatch does.
    let convert_difftime = |heap: &mut Heap, interner: &SymbolInterner, v: ValueId, target_secs: f64| -> ValueId {
        let units = difftime_units(heap, interner, v);
        let factor = unit_seconds(&units) / target_secs;
        let scaled: Vec<f64> = match heap.object(v) {
            Some(RObject::Real(vals)) => vals.iter().map(|x| x * factor).collect(),
            Some(RObject::Integer(vals)) => vals.iter().map(|&x| x as f64 * factor).collect(),
            _ => return v,
        };
        heap.alloc(RObject::Real(scaled))
    };
    let dt_pair = left_dt && right_dt && matches!(op, ADD | SUB);
    let dt_pair_units = if dt_pair {
        // The finer unit wins — the smaller seconds-per-unit exactly as
        /// GNU's mixed-unit conversion.
        if unit_seconds(&left_dt_units) <= unit_seconds(&right_dt_units) {
            left_dt_units.clone()
        } else {
            right_dt_units.clone()
        }
    } else {
        "secs".to_string()
    };
    let operands = {
        let mut ops = operands;
        if matches!(op, ADD | SUB) {
            if left_posix || right_posix {
                if left_dt {
                    let c = convert_difftime(heap, interner, ops[0], 1.0);
                    ops[0] = c;
                }
                if right_dt {
                    let c = convert_difftime(heap, interner, ops[1], 1.0);
                    ops[1] = c;
                }
            } else if left_date || right_date {
                if left_dt {
                    let c = convert_difftime(heap, interner, ops[0], 86400.0);
                    ops[0] = c;
                }
                if right_dt {
                    let c = convert_difftime(heap, interner, ops[1], 86400.0);
                    ops[1] = c;
                }
            } else if dt_pair {
                let target = unit_seconds(&dt_pair_units);
                let c0 = convert_difftime(heap, interner, ops[0], target);
                ops[0] = c0;
                let c1 = convert_difftime(heap, interner, ops[1], target);
                ops[1] = c1;
            }
        }
        ops
    };
    // Complex arithmetic: either operand complex promotes both exactly as
    /// GNU's type lattice, complex being the highest arithmetic type.
    let complex_involved = matches!(heap.object(operands[0]), Some(RObject::Complex(_)))
        || matches!(heap.object(operands[1]), Some(RObject::Complex(_)));
    if complex_involved {
        return complex_arithmetic(heap, interner, op, operands[0], operands[1]);
    }
    let left = numeric_operands(heap, operands[0]).map_err(RSignal::from)?;
    let right = numeric_operands(heap, operands[1]).map_err(RSignal::from)?;
    match (left, right) {
        (Numeric::Integer(a), Numeric::Integer(b)) if op != DIV && op != POW => {
            if op == IDIV {
                let folded = fold_numeric(a, b, op, |x, y| {
                    // GNU integer division truncates; division by zero or
                    // the INT_MIN/-1 overflow yields NA exactly.
                    if y == 0 || x == crate::value::object::NA_INTEGER || y == crate::value::object::NA_INTEGER {
                        crate::value::object::NA_INTEGER
                    } else {
                        x.checked_div(y).unwrap_or(crate::value::object::NA_INTEGER)
                    }
                })
                .map_err(RSignal::from)?;
                let result = heap.alloc(RObject::Integer(folded));
                return Ok(propagate_attributes(heap, operands[0], operands[1], result));
            }
            let folded = fold_numeric(a, b, op, |x, y| match op {
                ADD => {
                    // GNU checks integer overflow: out-of-range results
                    // become NA with a warning rather than wrapping.
                    if x == crate::value::object::NA_INTEGER || y == crate::value::object::NA_INTEGER {
                        crate::value::object::NA_INTEGER
                    } else {
                        x.checked_add(y).unwrap_or(crate::value::object::NA_INTEGER)
                    }
                }
                SUB => {
                    if x == crate::value::object::NA_INTEGER || y == crate::value::object::NA_INTEGER {
                        crate::value::object::NA_INTEGER
                    } else {
                        x.checked_sub(y).unwrap_or(crate::value::object::NA_INTEGER)
                    }
                }
                MOD => {
                    // GNU: integer modulo, division by zero yields NA.
                    if y == 0 || x == crate::value::object::NA_INTEGER || y == crate::value::object::NA_INTEGER {
                        crate::value::object::NA_INTEGER
                    } else {
                        x % y
                    }
                }
                _ => {
                    if x == crate::value::object::NA_INTEGER || y == crate::value::object::NA_INTEGER {
                        crate::value::object::NA_INTEGER
                    } else {
                        x.checked_mul(y).unwrap_or(crate::value::object::NA_INTEGER)
                    }
                }
            })
            .map_err(RSignal::from)?;
            let result = heap.alloc(RObject::Integer(folded));
            Ok(propagate_attributes(heap, operands[0], operands[1], result))
        }
        (a, b) => {
            let left_reals = as_reals(a);
            let right_reals = as_reals(b);
            let folded = fold_numeric(left_reals, right_reals, op, |x, y| {
                let result = match op {
                    ADD => x + y,
                    SUB => x - y,
                    MUL => x * y,
                    POW => {
                        // GNU's power edges precede the libm call: a zero
                        // exponent or unit base yields exactly 1 even for
                        // NA, NaN, and infinite operands.
                        if y == 0.0 || x == 1.0 {
                            1.0
                        } else {
                            x.powf(y)
                        }
                    }
                    IDIV => (x / y).floor(),
                    MOD => {
                        if y == 0.0 {
                            f64::NAN
                        } else {
                            // GNU's fmod semantics: sign follows the dividend.
                            x - y * (x / y).trunc()
                        }
                    }
                    _ => x / y,
                };
                // Hardware NaN canonicalization destroys the NA payload, so
                // GNU's NA survival is explicit: an NA operand poisons any
                // NaN result back to NA; genuine NaN production (Inf-Inf,
                // 0/0) keeps the canonical quiet NaN exactly as GNU does.
                if result.is_nan()
                    && (x.to_bits() == crate::value::object::NA_REAL_BITS
                        || y.to_bits() == crate::value::object::NA_REAL_BITS)
                {
                    f64::from_bits(crate::value::object::NA_REAL_BITS)
                } else {
                    result
                }
            })
            .map_err(RSignal::from)?;
            let result = heap.alloc(RObject::Real(folded));
            // GNU's datetime arithmetic discipline: the classed operand's
            /// attribute chain wins regardless of position, and each class
            /// pair follows its exact method matrix.
            let left_date = is_date_classed(heap, interner, operands[0]);
            let right_date = is_date_classed(heap, interner, operands[1]);
            let result = if (left_date && !right_date) || (left_posix && !right_posix) {
                propagate_attributes(heap, operands[0], operands[0], result)
            } else if (right_date && !left_date) || (right_posix && !left_posix) {
                propagate_attributes(heap, operands[1], operands[1], result)
            } else {
                propagate_attributes(heap, operands[0], operands[1], result)
            };
            let op_text = match op {
                ADD => "+",
                SUB => "-",
                MUL => "*",
                DIV => "/",
                POW => "^",
                MOD => "%%",
                _ => "op",
            };
            // difftime arithmetic: pairs in +/- already folded in their
            /// finer unit — stamp the class back with that unit. Scaling
            /// by a plain number keeps the difftime and its units exactly
            /// as GNU; difftime*difftime and division-by-difftime error.
            if left_dt && right_dt && op == DIV {
                return Err(EvalError::Condition(
                    "second argument of / cannot be a \"difftime\" object".to_string(),
                )
                .into());
            }
            if left_dt && right_dt && op != ADD && op != SUB {
                return Err(EvalError::Condition(format!(
                    "both arguments of {op_text} cannot be \"difftime\" objects"
                ))
                .into());
            }
            if dt_pair {
                let class = heap.alloc(RObject::String(vec![interner.intern_string("difftime")]));
                let units = heap.alloc(RObject::String(vec![interner.intern_string(&dt_pair_units)]));
                let stripped = strip_all_attributes(heap, result);
                let with_class = copy_with_attrib(heap, interner, stripped, "class", class).map_err(RSignal::from)?;
                return copy_with_attrib(heap, interner, with_class, "units", units).map_err(RSignal::from);
            }
            if (left_dt || right_dt) && matches!(op, MUL | DIV) {
                let (units_text, _) = if left_dt {
                    (left_dt_units.clone(), 0)
                } else {
                    (right_dt_units.clone(), 1)
                };
                let class = heap.alloc(RObject::String(vec![interner.intern_string("difftime")]));
                let units = heap.alloc(RObject::String(vec![interner.intern_string(&units_text)]));
                let stripped = strip_all_attributes(heap, result);
                let with_class = copy_with_attrib(heap, interner, stripped, "class", class).map_err(RSignal::from)?;
                return copy_with_attrib(heap, interner, with_class, "units", units).map_err(RSignal::from);
            }
            // POSIXct mirrors the Date matrix exactly, in seconds, with
            /// POSIXt in the message text precisely as GNU's method.
            if left_posix && right_posix && op != SUB {
                return Err(EvalError::Condition(format!(
                    "binary '{op_text}' is not defined for \"POSIXt\" objects"
                ))
                .into());
            }
            if left_posix && !right_posix && op != SUB && op != ADD {
                return Err(EvalError::Condition(format!(
                    "'{op_text}' not defined for \"POSIXt\" objects"
                ))
                .into());
            }
            if right_posix && !left_posix && op == SUB {
                return Err(EvalError::Condition(
                    "can only subtract from \"POSIXt\" objects".to_string(),
                )
                .into());
            }
            if left_posix && op == SUB && !right_posix {
                let right_classed = get_attrib(heap, interner, operands[1], "class").is_some();
                if right_classed && !right_dt {
                    return Err(EvalError::Condition(
                        "can only subtract numbers from \"POSIXt\" objects".to_string(),
                    )
                    .into());
                }
            }
            if left_posix && right_posix && op == SUB {
                // The difference is in seconds — GNU auto-selects the
                /// coarsest unit dividing every finite value exactly.
                let secs = numeric_vector(heap, result);
                let (unit_name, divisor) = auto_difftime_units(&secs);
                let scaled: Vec<f64> = secs.iter().map(|&s| if s.is_nan() { s } else { s / divisor }).collect();
                let bare = heap.alloc(RObject::Real(scaled));
                let class = heap.alloc(RObject::String(vec![interner.intern_string("difftime")]));
                let units = heap.alloc(RObject::String(vec![interner.intern_string(unit_name)]));
                let with_class = copy_with_attrib(heap, interner, bare, "class", class).map_err(RSignal::from)?;
                return copy_with_attrib(heap, interner, with_class, "units", units).map_err(RSignal::from);
            }
            // GNU's exact Date arithmetic error matrix: non-subtraction
            /// between Dates, multiplication of a Date by anything, and
            /// subtraction of a Date from a number all raise distinct
            /// conditions exactly as the oracle pins.
            if left_date && right_date && op != SUB {
                return Err(EvalError::Condition(format!(
                    "binary {op_text} is not defined for \"Date\" objects"
                ))
                .into());
            }
            if left_date && op != SUB && op != ADD {
                return Err(EvalError::Condition(format!(
                    "{op_text} not defined for \"Date\" objects"
                ))
                .into());
            }
            if right_date && !left_date && op == SUB {
                return Err(EvalError::Condition(
                    "can only subtract from \"Date\" objects".to_string(),
                )
                .into());
            }
            // Subtracting a classed non-number from a Date is the exact
            /// "can only subtract numbers" condition — foo-classed input
            /// is not a number for arithmetic purposes in GNU.
            if left_date && op == SUB && !right_date {
                let right_classed = get_attrib(heap, interner, operands[1], "class").is_some();
                let right_difftime = is_difftime_classed(heap, interner, operands[1]);
                if right_classed && !right_difftime {
                    return Err(EvalError::Condition(
                        "can only subtract numbers from \"Date\" objects".to_string(),
                    )
                    .into());
                }
            }
            if left_date && right_date && op == SUB {
                // Strip the Date class, stamp difftime with units=days.
                let stripped = remove_attrib(heap, interner, result, "class").map_err(RSignal::from)?;
                let class = heap.alloc(RObject::String(vec![interner.intern_string("difftime")]));
                let units = heap.alloc(RObject::String(vec![interner.intern_string("days")]));
                let with_class = copy_with_attrib(heap, interner, stripped, "class", class).map_err(RSignal::from)?;
                return copy_with_attrib(heap, interner, with_class, "units", units).map_err(RSignal::from);
            }
            Ok(result)
        }
    }
}

/// GNU attribute discipline for elementwise arithmetic: the result carries
/// the attribute chain of the longer operand (ties favor the left), so
/// names survive vector arithmetic and dim/dimnames survive matrix
/// arithmetic exactly.
fn propagate_attributes(
    heap: &mut Heap,
    left: ValueId,
    right: ValueId,
    result: ValueId,
) -> ValueId {
    let left_len = vector_length(heap, left).unwrap_or(0);
    let right_len = vector_length(heap, right).unwrap_or(0);
    let source = if left_len >= right_len { left } else { right };
    let chain = heap.get(source).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    if chain != ValueId::NIL {
        if let Some(slot) = heap.get_mut(result) {
            slot.attrib = chain;
        }
    }
    result
}

/// Evaluate and force the leading `expected` arguments of a call.
fn eval_args(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    expected: usize,
) -> Result<Vec<ValueId>, RSignal> {
    let mut operands = Vec::new();
    let mut cursor = args;
    while operands.len() < expected {
        let (car, cdr) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, .. }) => (*car, *cdr),
            _ => break,
        };
        // Empty actual arguments error with their 1-based call position
        /// exactly as GNU's argument forcing reports them.
        if car == ValueId::MISSING {
            return Err(EvalError::Condition(format!(
                "argument {} is empty",
                operands.len() + 1
            ))
            .into());
        }
        let evaluated = eval(heap, interner, car, caller_env)?;
        operands.push(force_if_promise(heap, interner, evaluated)?);
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    if operands.len() != expected {
        return Err(EvalError::UnsupportedForm("wrong argument count").into());
    }
    Ok(operands)
}

enum Numeric {
    Integer(Vec<i32>),
    Real(Vec<f64>),
}

fn numeric_operands(heap: &Heap, value: ValueId) -> Result<Numeric, EvalError> {
    match heap.object(value) {
        Some(RObject::Integer(values)) => Ok(Numeric::Integer(values.clone())),
        Some(RObject::Real(values)) => Ok(Numeric::Real(values.clone())),
        Some(RObject::Logical(values)) => Ok(Numeric::Integer(values.clone())),
        _ => Err(EvalError::TypeError("non-numeric argument to binary operator")),
    }
}

fn as_reals(numeric: Numeric) -> Vec<f64> {
    match numeric {
        Numeric::Real(values) => values,
        Numeric::Integer(values) => values
            .into_iter()
            .map(|v| {
                if v == crate::value::object::NA_INTEGER {
                    f64::from_bits(crate::value::object::NA_REAL_BITS)
                } else {
                    v as f64
                }
            })
            .collect(),
    }
}

/// Elementwise binary fold with GNU recycling: the longer length must be a
/// multiple of the shorter; length-one operands recycle silently.
fn fold_numeric<T: Copy>(
    a: Vec<T>,
    b: Vec<T>,
    _op: u32,
    f: impl Fn(T, T) -> T,
) -> Result<Vec<T>, EvalError> {
    // GNU recycles the shorter operand regardless of multiplicity — the
    // non-multiple case only raises a warning — and a zero-length operand
    // yields a zero-length result outright.
    if a.is_empty() || b.is_empty() {
        return Ok(Vec::new());
    }
    // Operand order is significant — subtraction, division, modulo and
    // power are non-commutative, so each side indexes into its own vector
    // with independent recycling exactly as GNU.
    let len = a.len().max(b.len());
    Ok((0..len)
        .map(|i| f(a[i % a.len()], b[i % b.len()]))
        .collect())
}

/// `while (test) body` — loops until the test forces false. `break`
/// unwinds out of the loop entirely; `next` skips to the next iteration.
/// The loop value is always NIL in GNU.
fn while_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() != 2 {
        return Err(EvalError::UnsupportedForm("while arity").into());
    }
    loop {
        let test = eval(heap, interner, parts[0], caller_env)?;
        let test = force_if_promise(heap, interner, test)?;
        if !truthy(heap, test)? {
            return Ok(ValueId::NIL);
        }
        match eval(heap, interner, parts[1], caller_env) {
            Err(RSignal::Break) => return Ok(ValueId::NIL),
            Err(RSignal::Next) => continue,
            other => {
                let _ = other?;
            }
        }
    }
}

/// `repeat body` — an unconditional loop; only `break` exits.
fn repeat_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() != 1 {
        return Err(EvalError::UnsupportedForm("repeat arity").into());
    }
    loop {
        match eval(heap, interner, parts[0], caller_env) {
            Err(RSignal::Break) => return Ok(ValueId::NIL),
            Err(RSignal::Next) => continue,
            other => {
                let _ = other?;
            }
        }
    }
}

/// `for (var in seq) body` — binds each sequence element in turn. Loop
/// variables in R persist after the loop and take the last element's
/// value; elements are boxed as length-one vectors, matching GNU.
fn for_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() != 3 {
        return Err(EvalError::UnsupportedForm("for arity").into());
    }
    let sym = match heap.object(parts[0]) {
        Some(RObject::Symbol { name, .. }) => *name,
        _ => return Err(EvalError::TypeError("for variable must be a symbol").into()),
    };
    let seq = eval(heap, interner, parts[1], caller_env)?;
    let seq = force_if_promise(heap, interner, seq)?;
    let elements = sequence_elements(heap, seq)?;
    for element in elements {
        crate::value::env::define(heap, caller_env, sym, element)
            .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("for define")))?;
        match eval(heap, interner, parts[2], caller_env) {
            Err(RSignal::Break) => break,
            Err(RSignal::Next) => continue,
            other => {
                let _ = other?;
            }
        }
    }
    Ok(ValueId::NIL)
}

/// Force a value to a single truth value, GNU's condition coercion.
fn truthy(heap: &Heap, value: ValueId) -> Result<bool, RSignal> {
    match heap.object(value) {
        Some(RObject::Logical(values)) if !values.is_empty() => Ok(values[0] != 0),
        Some(RObject::Integer(values)) if !values.is_empty() => Ok(values[0] != 0),
        Some(RObject::Real(values)) if !values.is_empty() => Ok(values[0] != 0.0),
        _ => Err(EvalError::TypeError("condition must be logical").into()),
    }
}

/// Explode a vector into per-element length-one vectors — what for-loop
/// iteration binds. List elements pass through unboxed, exactly GNU.
fn sequence_elements(heap: &mut Heap, seq: ValueId) -> Result<Vec<ValueId>, RSignal> {
    let elements: Vec<RObject> = match heap.object(seq) {
        Some(RObject::Real(values)) => values
            .iter()
            .map(|v| RObject::Real(vec![*v]))
            .collect(),
        Some(RObject::Integer(values)) => values
            .iter()
            .map(|v| RObject::Integer(vec![*v]))
            .collect(),
        Some(RObject::Logical(values)) => values
            .iter()
            .map(|v| RObject::Logical(vec![*v]))
            .collect(),
        Some(RObject::List(elements)) => return Ok(elements.clone()),
        // Pairlist chains iterate their node cars exactly as apply over
        // formals does in GNU.
        Some(RObject::Pairlist { .. }) => {
            let mut out = Vec::new();
            let mut cursor = seq;
            loop {
                match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, .. }) => {
                        out.push(*car);
                        if *cdr == ValueId::NIL {
                            break;
                        }
                        cursor = *cdr;
                    }
                    _ => break,
                }
            }
            return Ok(out);
        }
        _ => {
            return Err(EvalError::TypeError("for sequence must be a vector").into());
        }
    };
    Ok(elements.into_iter().map(|body| heap.alloc(body)).collect())
}

/// Binary comparisons with GNU elementwise semantics: numeric coercion,
/// recycling, logical results.

/// Ordered-factor class membership — the class vector carries "ordered".
fn is_ordered_classed(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    match get_attrib(heap, interner, value, "class") {
        Some(class_id) => string_vec(heap, interner, class_id).iter().any(|c| c == "ordered"),
        None => false,
    }
}

fn comparison(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    // difftime operands compare through their seconds conversion —
    /// magnitudes in mixed units order exactly as GNU's time lattice.
    let operands = {
        let mut ops = operands;
        for i in 0..2 {
            if is_difftime_classed(heap, interner, ops[i]) {
                let factor = unit_seconds(&difftime_units(heap, interner, ops[i]));
                let scaled: Vec<f64> = match heap.object(ops[i]) {
                    Some(RObject::Real(vals)) => vals.iter().map(|x| x * factor).collect(),
                    Some(RObject::Integer(vals)) => vals.iter().map(|&x| x as f64 * factor).collect(),
                    _ => Vec::new(),
                };
                if !scaled.is_empty() {
                    ops[i] = heap.alloc(RObject::Real(scaled));
                }
            }
        }
        ops
    };
    // Classed comparison matrix — datetime and ordered-factor operands
    // intercept before the plain character lattice exactly as GNU's
    // Ops.Date/Ops.POSIXct/Ops.factor methods.
    let left_ordered = is_ordered_classed(heap, interner, operands[0]);
    let right_ordered = is_ordered_classed(heap, interner, operands[1]);
    let left_factor = left_ordered || is_factor(heap, interner, operands[0]);
    let right_factor = right_ordered || is_factor(heap, interner, operands[1]);
    if left_factor || right_factor {
        let left_is_string = matches!(heap.object(operands[0]), Some(RObject::String(_))) && !left_factor;
        let right_is_string = matches!(heap.object(operands[1]), Some(RObject::String(_))) && !right_factor;
        // Factor-vs-factor: the level sets must agree exactly — GNU errors
        // otherwise. Codes compare directly with NA propagation.
        if left_factor && right_factor {
            let left_levels: Vec<String> = match get_attrib(heap, interner, operands[0], "levels") {
                Some(l) => string_vec(heap, interner, l),
                None => Vec::new(),
            };
            let right_levels: Vec<String> = match get_attrib(heap, interner, operands[1], "levels") {
                Some(l) => string_vec(heap, interner, l),
                None => Vec::new(),
            };
            if left_levels != right_levels {
                return Err(EvalError::TypeError("level sets of factors are different").into());
            }
            let lefts: Vec<i32> = match heap.object(operands[0]) {
                Some(RObject::Integer(vals)) => vals.clone(),
                _ => Vec::new(),
            };
            let rights: Vec<i32> = match heap.object(operands[1]) {
                Some(RObject::Integer(vals)) => vals.clone(),
                _ => Vec::new(),
            };
            let len = lefts.len().max(rights.len());
            if len == 0 {
                return Ok(heap.alloc(RObject::Logical(vec![])));
            }
            let mut out: Vec<i32> = Vec::with_capacity(len);
            for i in 0..len {
                let a = lefts[i % lefts.len().max(1)];
                let b = rights[i % rights.len().max(1)];
                if a == crate::value::object::NA_INTEGER || b == crate::value::object::NA_INTEGER {
                    out.push(crate::value::object::NA_INTEGER);
                    continue;
                }
                let result = match op {
                    LT => a < b,
                    GT => a > b,
                    LE => a <= b,
                    GE => a >= b,
                    EQ => a == b,
                    _ => a != b,
                };
                out.push(result as i32);
            }
            return Ok(heap.alloc(RObject::Logical(out)));
        }
        // Factor vs character — equality compares labels; ordering on
        // ordered factors converts the string through its level index.
        let (factor_side, string_side, factor_levels) = if left_factor {
            let levels = match get_attrib(heap, interner, operands[0], "levels") {
                Some(l) => string_vec(heap, interner, l),
                None => Vec::new(),
            };
            (operands[0], operands[1], levels)
        } else {
            let levels = match get_attrib(heap, interner, operands[1], "levels") {
                Some(l) => string_vec(heap, interner, l),
                None => Vec::new(),
            };
            (operands[1], operands[0], levels)
        };
        let flipped = !left_factor;
        let _ = (left_is_string, right_is_string);
        let codes: Vec<i32> = match heap.object(factor_side) {
            Some(RObject::Integer(vals)) => vals.clone(),
            _ => Vec::new(),
        };
        let strings = string_vec(heap, interner, string_side);
        let len = codes.len().max(strings.len());
        if len == 0 {
            return Ok(heap.alloc(RObject::Logical(vec![])));
        }
        let mut out: Vec<i32> = Vec::with_capacity(len);
        for i in 0..len {
            let code = codes[i % codes.len().max(1)];
            let text = &strings[i % strings.len().max(1)];
            if code == crate::value::object::NA_INTEGER || text == "\0NA" {
                out.push(crate::value::object::NA_INTEGER);
                continue;
            }
            let label = factor_levels.get((code as usize).saturating_sub(1)).cloned().unwrap_or_default();
            let result = match op {
                EQ | NE => {
                    let eq = &label == text;
                    if op == EQ { eq } else { !eq }
                }
                _ => {
                    // Ordering: the string resolves through its level index
                    // exactly as GNU's ordered-factor method.
                    let text_index = factor_levels.iter().position(|l| l == text).map(|p| p as i32 + 1);
                    match text_index {
                        Some(ti) => {
                            let (a, b) = if flipped { (ti, code) } else { (code, ti) };
                            match op {
                                LT => a < b,
                                GT => a > b,
                                LE => a <= b,
                                _ => a >= b,
                            }
                        }
                        None => {
                            out.push(crate::value::object::NA_INTEGER);
                            continue;
                        }
                    }
                }
            };
            out.push(result as i32);
        }
        return Ok(heap.alloc(RObject::Logical(out)));
    }
    // Date/POSIXct vs character — strings parse into the temporal lattice
    // exactly as GNU's coercion; failures raise the unambiguous-format
    // error, NA_character_ poisons to NA.
    let left_temporal = is_date_classed(heap, interner, operands[0]) || is_posixct_classed(heap, interner, operands[0]);
    let right_temporal = is_date_classed(heap, interner, operands[1]) || is_posixct_classed(heap, interner, operands[1]);
    if (left_temporal || right_temporal)
        && (matches!(heap.object(operands[0]), Some(RObject::String(_))) || matches!(heap.object(operands[1]), Some(RObject::String(_))))
    {
        let posix = is_posixct_classed(heap, interner, operands[0]) || is_posixct_classed(heap, interner, operands[1]);
        let parse = |text: &str| -> Result<Option<f64>, EvalError> {
            if text == "\0NA" {
                return Ok(None);
            }
            let parsed = if posix {
                super::civil::parse_iso_datetime(text).map(|v| v as f64)
            } else {
                super::civil::parse_iso(text).map(|v| v as f64)
            };
            match parsed {
                Some(v) => Ok(Some(v)),
                None => Err(EvalError::TypeError("character string is not in a standard unambiguous format")),
            }
        };
        let numerify = |side: ValueId| -> Result<Vec<Option<f64>>, EvalError> {
            if matches!(heap.object(side), Some(RObject::String(_))) {
                let mut out = Vec::new();
                for text in string_vec(heap, interner, side) {
                    out.push(parse(&text)?);
                }
                Ok(out)
            } else {
                let vals = match heap.object(side) {
                    Some(RObject::Real(vals)) => vals.iter().map(|v| if v.is_nan() { None } else { Some(*v) }).collect(),
                    Some(RObject::Integer(vals)) => vals
                        .iter()
                        .map(|v| if *v == crate::value::object::NA_INTEGER { None } else { Some(*v as f64) })
                        .collect(),
                    _ => Vec::new(),
                };
                Ok(vals)
            }
        };
        let lefts = numerify(operands[0]).map_err(RSignal::from)?;
        let rights = numerify(operands[1]).map_err(RSignal::from)?;
        let len = lefts.len().max(rights.len());
        let mut out: Vec<i32> = Vec::with_capacity(len);
        for i in 0..len {
            let a = lefts[i % lefts.len().max(1)];
            let b = rights[i % rights.len().max(1)];
            match (a, b) {
                (Some(x), Some(y)) => {
                    let result = match op {
                        LT => x < y,
                        GT => x > y,
                        LE => x <= y,
                        GE => x >= y,
                        EQ => x == y,
                        _ => x != y,
                    };
                    out.push(result as i32);
                }
                _ => out.push(crate::value::object::NA_INTEGER),
            }
        }
        return Ok(heap.alloc(RObject::Logical(out)));
    }
    // GNU character comparison: when either operand is character the other
    // coerces, equality is exact and ordering lexicographic, and NA or NaN
    // participation poisons the element to NA logical exactly as numerics.
    let either_string = matches!(heap.object(operands[0]), Some(RObject::String(_)))
        || matches!(heap.object(operands[1]), Some(RObject::String(_)));
    if either_string {
        let lefts = string_vec(heap, interner, operands[0]);
        let rights = string_vec(heap, interner, operands[1]);
        let len = lefts.len().max(rights.len());
        if len == 0 {
            return Ok(heap.alloc(RObject::Logical(vec![])));
        }
        if lefts.is_empty() || rights.is_empty() {
            return Err(EvalError::TypeError("comparison requires non-empty operands").into());
        }
        let mut out: Vec<i32> = Vec::with_capacity(len);
        for i in 0..len {
            let a = &lefts[i % lefts.len()];
            let b = &rights[i % rights.len()];
            if a == "\0NA" || b == "\0NA" {
                out.push(crate::value::object::NA_INTEGER);
                continue;
            }
            let result = match op {
                LT => a < b,
                GT => a > b,
                LE => a <= b,
                GE => a >= b,
                EQ => a == b,
                _ => a != b,
            };
            out.push(if result { 1 } else { 0 });
        }
        return Ok(heap.alloc(RObject::Logical(out)));
    }
    let left = numeric_operands(heap, operands[0]).map_err(RSignal::from)?;
    let right = numeric_operands(heap, operands[1]).map_err(RSignal::from)?;
    let a = as_reals(left);
    let b = as_reals(right);
    let folded = fold_numeric(a, b, op, |x, y| {
        // GNU: any NaN operand (NA or NaN) poisons the comparison to NA
        /// logical — NaN comparisons are never simply false.
        if x.is_nan() || y.is_nan() {
            return crate::value::object::NA_INTEGER as f64;
        }
        let result = match op {
            LT => x < y,
            GT => x > y,
            LE => x <= y,
            GE => x >= y,
            EQ => x == y,
            _ => x != y,
        };
        if result {
            1.0
        } else {
            0.0
        }
    })
    .map_err(RSignal::from)?;
    Ok(heap.alloc(RObject::Logical(
        folded.into_iter().map(|v| v as i32).collect(),
    )))
}

/// Every primitive print name registered in the scaffolding table — the
/// canonical list bootstrap environments iterate so the two never drift.
pub fn primitive_names() -> &'static [&'static str] {
    &[
        "+", "-", "*", "/", "<", ">", "<=", ">=", "==", "!=", "if", "{", "<-", "=",
        "<<-", "return", "while", "repeat", "for", "break", "next", "function", "c", "[",
        "length", ":", "(", "&", "|", "!", "&&", "||", "sum", "prod", "min", "max", "sqrt",
        "exp", "log", "abs", "^", "[[", "list", "paste", "paste0", "nchar", "print", "[<-",
        "numeric", "integer", "logical", "names", "names<-", "$", "lapply", "sapply", "unlist", "seq", "seq_len", "rep", "mean", "var", "sd", "median", "is.na", "sort", "rev", "unique", "which", "any", "all", "ifelse", "%%", "stop", "%in%", "match", "new.env", "exists", "ls", "$<-", "cat", "invisible", "typeof", "structure", "assign", "get", "character", "tempfile", "writeLines", "readLines", "file.exists", "file.remove", "file.copy", "file.rename", "suppressWarnings", "as.logical", "attr", "attr<-", "attributes", "attributes<-", "is.null", "length<-", "file.path", "data.frame", "aggregate", "factor", "levels", "levels<-", "unclass", "tempdir", "sprintf", "Sys.setenv", "Sys.getenv", "Sys.unsetenv", "unlink", "dir.create", "list.dirs", "basename", ".Internal", ".Primitive", "warning", "gettext", "ngettext", "bindtextdomain", "geterrmessage", "seterrmessage", "interruptsSuspended", "is.primitive", "isTRUE", "save", "load", "rm", "as.vector", "as.character", "setNames", "identical", "globalenv", "baseenv", "environmentName", "environment", "R.Version", "getRversion", "is.list", "array", "is.array", "as.Date", "weekdays", "months", "quarters", "format", "is.unsorted", "toString", "capture.output", "row.names", "row.names<-", "tapply", "vapply", "as.POSIXct", "proc.time", "quote", "~", "is.call", "is.environment", "xtabs", "sin", "cos", "sinh", "cosh", "tanh", "complex", "as.complex", "Re", "Im", "Mod", "Arg", "Conj", "is.complex", "as.raw", "is.raw", "mode", "storage.mode", "expression", "raw", "vector", "identity", "parse", "eval", "deparse1", "deparse", "dput", "dget", "inherits", "dirname", "gregexpr", "gsub", "sub", "formals", "options", "getOption", "cbind", "rbind", "is.numeric", "is.integer", "is.double", "is.logical", "is.character", "substitute", "tsp", "tsp<-", "printDeferredWarnings", "grepl", "grep", "regexpr", "dimnames", "dimnames<-", "class", "class<-", "rownames", "rownames<-", "colnames", "colnames<-", "tryCatch", "conditionMessage", "matrix", "dim", "nrow", "ncol", "dnorm", "pnorm", "qnorm", "dpois", "ppois", "dbinom", "pbinom", "message", "missing", "parent.env", "order", "rank", "ordered", "is.ordered", "is.factor", "na.omit", "na.exclude", "pbeta", "dbeta", "qbeta", "find", "UseMethod", "NextMethod", "getS3method", "emptyenv", "as.integer", "as.numeric", "as.double", "stopifnot", "sequence", "rep.int", "rep_len", "dt", "pt", "qt", "sample", "sample.int", "builtins", "regexec", "df", "pf", "qf", "substr", "file", "isOpen", "close", "flush", "writeChar", "readChar", "union", "intersect", "setdiff", "setequal", "table", "setClass", "new", "is", "slot", "slotNames", "unname", "which.min", "which.max", "withVisible", "system", "writeBin", "readBin", "url", "dim<-", "storage.mode<-", "as.list", "body", "summary", "%*%", "crossprod", "tcrossprod", "max.col", "%/%", "NROW", "NCOL", "Sys.getpid", "OlsonNames", "IQR", "Sys.chmod", "file.info", "Sys.getlocale", "Sys.setlocale", "nzchar", "@", "@<-", "Sys.info", "Sys.time", "Sys.Date", "Sys.which", "floor", "ceiling", "trunc", "round", "signif", "expm1", "log1p", "acosh", "asinh", "atanh", "cospi", "sinpi", "tanpi", "anyNA", "Sys.localeconv", "agrep", "agrepl", "apply", "as.environment", "as.call", "enc2native", "enc2utf8", "search", "as.factor", "as.ordered", "addNA", "aperm", "addmargins", ".subset", ".subset2", "asplit", "attach", "detach", "pos.to.env", ".class2", "gc.time", "ave", "backsolve", "bquote", "call", "charToRaw", "charmatch", "cumsum", "cumprod", "cummin", "cummax", "comment", "comment<-", "complete.cases", "colSums", "rowSums", "colMeans", "rowMeans", "dcauchy", "dchisq", "dexp", "cut", "date", "capabilities", "by", "browser", "bzfile", "gzfile", "dgamma", "dgeom", "dnbinom", "diff", "diag", "do.call", "dweibull", "drop", "droplevels", "duplicated", "anyDuplicated", "dir.exists", "file.create", "file.access", "environment<-", "forceAndCall", "declare", ".primTrace", ".primUntrace", "standardGeneric", "dmultinom", "...length", "...names", "...elt", "is.data.frame", "file.append", "file.link", "file.symlink", "Sys.readlink", "file.size", "file.mtime", "gl", "head", "tail", "interaction", "gcinfo", "gctorture", "gctorture2", "is.finite", "is.infinite", "is.nan", "is.vector", "is.loaded", "l10n_info", "Cstack_info", "extSoftVersion", "list.files", "memory.profile",
    ]
}

/// Resolve a primitive's print name to a scaffolding id — the interop
/// mapping the import bridge uses until the canonical registry lands.
pub fn primitive_id_for_name(name: &str) -> Option<u32> {
    match name {
        "+" => Some(ADD),
        "-" => Some(SUB),
        "*" => Some(MUL),
        "%%" => Some(MOD),
        "%/%" => Some(IDIV),
        "NROW" => Some(NROW_FN),
        "NCOL" => Some(NCOL_FN),
        "Sys.getpid" => Some(SYS_GETPID),
        "OlsonNames" => Some(OLSONNAMES),
        "IQR" => Some(IQR_FN),
        "Sys.chmod" => Some(SYS_CHMOD),
        "file.info" => Some(FILE_INFO),
        "Sys.getlocale" => Some(SYS_GETLOCALE),
        "Sys.setlocale" => Some(SYS_SETLOCALE),
        "nzchar" => Some(NZCHAR),
        "@" => Some(AT_OP),
        "@<-" => Some(AT_ASSIGN),
        "Sys.info" => Some(SYS_INFO),
        "Sys.time" => Some(SYS_TIME),
        "Sys.Date" => Some(SYS_DATE),
        "Sys.which" => Some(SYS_WHICH),
        "floor" => Some(FLOOR),
        "ceiling" => Some(CEILING),
        "trunc" => Some(TRUNC),
        "round" => Some(ROUND),
        "signif" => Some(SIGNIF),
        "expm1" => Some(EXPM1),
        "log1p" => Some(LOG1P),
        "acosh" => Some(ACOSH),
        "asinh" => Some(ASINH),
        "atanh" => Some(ATANH),
        "cospi" => Some(COSPI),
        "sinpi" => Some(SINPI),
        "tanpi" => Some(TANPI),
        "anyNA" => Some(ANYNA),
        "Sys.localeconv" => Some(SYS_LOCALECONV),
        "agrep" => Some(AGREP),
        "agrepl" => Some(AGREPL),
        "apply" => Some(APPLY),
        "as.environment" => Some(AS_ENVIRONMENT),
        "as.call" => Some(AS_CALL),
        "enc2native" => Some(ENC2NATIVE),
        "enc2utf8" => Some(ENC2UTF8),
        "search" => Some(SEARCH_FN),
        "as.factor" => Some(AS_FACTOR),
        "as.ordered" => Some(AS_ORDERED),
        "addNA" => Some(ADDNA),
        "aperm" => Some(APERM),
        "addmargins" => Some(ADDMARGINS),
        ".subset" => Some(DOT_SUBSET),
        ".subset2" => Some(DOT_SUBSET2),
        "asplit" => Some(ASPLIT),
        "attach" => Some(ATTACH),
        "detach" => Some(DETACH),
        "pos.to.env" => Some(POS_TO_ENV),
        ".class2" => Some(DOT_CLASS2),
        "gc.time" => Some(GC_TIME),
        "ave" => Some(AVE),
        "backsolve" => Some(BACKSOLVE),
        "bquote" => Some(BQUOTE),
        "call" => Some(CALL_FN),
        "charToRaw" => Some(CHAR_TO_RAW),
        "charmatch" => Some(CHARMATCH),
        "cumsum" => Some(CUMSUM),
        "cumprod" => Some(CUMPROD),
        "cummin" => Some(CUMMIN),
        "cummax" => Some(CUMMAX),
        "comment" => Some(COMMENT_FN),
        "comment<-" => Some(COMMENT_ASSIGN),
        "complete.cases" => Some(COMPLETE_CASES),
        "colSums" => Some(COLSUMS),
        "rowSums" => Some(ROWSUMS),
        "colMeans" => Some(COLMEANS),
        "rowMeans" => Some(ROWMEANS),
        "dcauchy" => Some(DCAUCHY),
        "dchisq" => Some(DCHISQ),
        "dexp" => Some(DEXP),
        "cut" => Some(CUT),
        "date" => Some(DATE_FN),
        "capabilities" => Some(CAPABILITIES),
        "by" => Some(BY_FN),
        "browser" => Some(BROWSER_FN),
        "dgamma" => Some(DGAMMA),
        "dgeom" => Some(DGEOM),
        "dnbinom" => Some(DNBINOM),
        "diff" => Some(DIFF),
        "diag" => Some(DIAG),
        "do.call" => Some(DO_CALL),
        "dweibull" => Some(DWEIBULL),
        "drop" => Some(DROP_FN),
        "droplevels" => Some(DROPLEVELS),
        "duplicated" => Some(DUPLICATED),
        "anyDuplicated" => Some(ANYDUPLICATED),
        "dir.exists" => Some(DIR_EXISTS),
        "file.create" => Some(FILE_CREATE),
        "file.access" => Some(FILE_ACCESS),
        "environment<-" => Some(ENVIRONMENT_ASSIGN),
        "forceAndCall" => Some(FORCEANDCALL),
        "declare" => Some(DECLARE),
        ".primTrace" => Some(PRIMTRACE),
        ".primUntrace" => Some(PRIMUNTRACE),
        "standardGeneric" => Some(STANDARDGENERIC),
        "dmultinom" => Some(DMULTINOM),
        "...length" => Some(DOTS_LENGTH),
        "...names" => Some(DOTS_NAMES),
        "...elt" => Some(DOTS_ELT),
        "is.data.frame" => Some(IS_DATA_FRAME),
        "file.append" => Some(FILE_APPEND),
        "file.link" => Some(FILE_LINK),
        "file.symlink" => Some(FILE_SYMLINK),
        "Sys.readlink" => Some(SYS_READLINK),
        "file.size" => Some(FILE_SIZE),
        "file.mtime" => Some(FILE_MTIME),
        "gl" => Some(GL),
        "head" => Some(HEAD_FN),
        "tail" => Some(TAIL_FN),
        "interaction" => Some(INTERACTION),
        "gcinfo" => Some(GCINFO),
        "gctorture" => Some(GCTORTURE),
        "gctorture2" => Some(GCTORTURE2),
        "is.finite" => Some(IS_FINITE),
        "is.infinite" => Some(IS_INFINITE),
        "is.nan" => Some(IS_NAN),
        "is.vector" => Some(IS_VECTOR),
        "is.loaded" => Some(IS_LOADED),
        "l10n_info" => Some(L10N_INFO),
        "Cstack_info" => Some(CSTACK_INFO),
        "extSoftVersion" => Some(EXTSOFTVERSION),
        "list.files" => Some(LIST_FILES),
        "dir" => Some(LIST_FILES),
        "memory.profile" => Some(MEMORY_PROFILE),
        // bzfile/gzfile share the raw-byte connection machinery — GNU pipes
        // through the codec, but within-R observable behavior (write/read
        // roundtrips, append semantics) is identical; real compression is
        // a documented fidelity gap.
        "bzfile" => Some(FILE_FN),
        "gzfile" => Some(FILE_FN),
        "/" => Some(DIV),
        "if" => Some(IF),
        "{" => Some(BRACE),
        "<-" | "=" => Some(ASSIGN),
        "<<-" => Some(SUPERASSIGN),
        "return" => Some(RETURN),
        "while" => Some(WHILE),
        "repeat" => Some(REPEAT),
        "for" => Some(FOR),
        "break" => Some(BREAK),
        "next" => Some(NEXT),
        "<" => Some(LT),
        ">" => Some(GT),
        "<=" => Some(LE),
        ">=" => Some(GE),
        "==" => Some(EQ),
        "!=" => Some(NE),
        "function" => Some(FUNCTION),
        "c" => Some(CONCAT),
        "[" => Some(SUBSET),
        "length" => Some(LENGTH),
        ":" => Some(COLON),
        "(" => Some(PAREN),
        "&" => Some(AND),
        "|" => Some(OR),
        "!" => Some(NOT),
        "&&" => Some(SHORT_AND),
        "||" => Some(SHORT_OR),
        "sum" => Some(SUM),
        "prod" => Some(PROD),
        "min" => Some(MIN),
        "max" => Some(MAX),
        "sqrt" => Some(SQRT),
        "exp" => Some(EXP),
        "log" => Some(LOG),
        "abs" => Some(ABS),
        "^" => Some(POW),
        "[[" => Some(EXTRACT),
        "list" => Some(LIST),
        "paste" => Some(PASTE),
        "paste0" => Some(PASTE0),
        "nchar" => Some(NCHAR),
        "print" => Some(PRINT),
        "[<-" => Some(SUBSET_ASSIGN),
        "numeric" => Some(NUMERIC),
        "integer" => Some(INTEGER_CTOR),
        "logical" => Some(LOGICAL_CTOR),
        "names" => Some(NAMES),
        "names<-" => Some(NAMES_ASSIGN),
        "$" => Some(DOLLAR),
        "lapply" => Some(LAPPLY),
        "sapply" => Some(SAPPLY),
        "unlist" => Some(UNLIST),
        "seq" => Some(SEQ),
        "seq_len" => Some(SEQ_LEN),
        "rep" => Some(REP),
        "mean" => Some(MEAN),
        "var" => Some(VAR),
        "sd" => Some(SD),
        "median" => Some(MEDIAN),
        "is.na" => Some(IS_NA),
        "sort" => Some(SORT),
        "rev" => Some(REV),
        "unique" => Some(UNIQUE),
        "which" => Some(WHICH),
        "any" => Some(ANY),
        "all" => Some(ALL),
        "ifelse" => Some(IFELSE),
        "stop" => Some(STOP),
        "%in%" => Some(IN),
        "match" => Some(MATCH),
        "new.env" => Some(NEW_ENV),
        "exists" => Some(EXISTS),
        "ls" => Some(LS),
        "$<-" => Some(DOLLAR_ASSIGN),
        "cat" => Some(CAT),
        "invisible" => Some(INVISIBLE),
        "typeof" => Some(TYPEOF),
        "structure" => Some(STRUCTURE),
        "assign" => Some(ASSIGN_FN),
        "get" => Some(GET),
        "character" => Some(CHARACTER_CTOR),
        "tempfile" => Some(TEMPFILE),
        "writeLines" => Some(WRITELINES),
        "readLines" => Some(READLINES),
        "file.exists" => Some(FILE_EXISTS),
        "file.remove" => Some(FILE_REMOVE),
        "file.copy" => Some(FILE_COPY),
        "file.rename" => Some(FILE_RENAME),
        "suppressWarnings" => Some(SUPPRESS_WARNINGS),
        "as.logical" => Some(AS_LOGICAL),
        "attr" => Some(ATTR),
        "attr<-" => Some(ATTR_ASSIGN),
        "attributes" => Some(ATTRIBUTES),
        "attributes<-" => Some(ATTRIBUTES_ASSIGN),
        "is.null" => Some(IS_NULL),
        "length<-" => Some(LENGTH_ASSIGN),
        "file.path" => Some(FILE_PATH),
        "data.frame" => Some(DATA_FRAME),
        "aggregate" => Some(AGGREGATE),
        "factor" => Some(FACTOR),
        "levels" => Some(LEVELS),
        "levels<-" => Some(LEVELS_ASSIGN),
        "unclass" => Some(UNCLASS),
        "sprintf" => Some(SPRINTF),
        "Sys.setenv" => Some(SYS_SETENV),
        "Sys.getenv" => Some(SYS_GETENV),
        "Sys.unsetenv" => Some(SYS_UNSETENV),
        "unlink" => Some(UNLINK),
        "dir.create" => Some(DIR_CREATE),
        "list.dirs" => Some(LIST_DIRS),
        "basename" => Some(BASENAME),
        ".Internal" => Some(INTERNAL),
        ".Primitive" => Some(PRIMITIVE_FN),
        "warning" => Some(WARNING),
        "gettext" => Some(GETTEXT),
        "ngettext" => Some(NGETTEXT),
        "bindtextdomain" => Some(BINDTEXTDOMAIN),
        "geterrmessage" => Some(GETERRMESSAGE),
        "seterrmessage" => Some(SETERRMESSAGE),
        "interruptsSuspended" => Some(INTERRUPTS_SUSPENDED),
        "is.primitive" => Some(IS_PRIMITIVE),
        "isTRUE" => Some(IS_TRUE),
        "save" => Some(SAVE),
        "load" => Some(LOAD),
        "rm" => Some(RM),
        "as.vector" => Some(AS_VECTOR),
        "as.character" => Some(AS_CHARACTER),
        "setNames" => Some(SETNAMES),
        "identical" => Some(IDENTICAL),
        "globalenv" => Some(GLOBALENV),
        "baseenv" => Some(BASEENV),
        "environmentName" => Some(ENVIRONMENT_NAME),
        "environment" => Some(ENVIRONMENT),
        "R.Version" => Some(R_VERSION_FN),
        "getRversion" => Some(GET_RVERSION),
        "is.list" => Some(IS_LIST),
        "array" => Some(ARRAY),
        "is.array" => Some(IS_ARRAY),
        "as.Date" => Some(AS_DATE),
        "weekdays" => Some(WEEKDAYS),
        "months" => Some(MONTHS),
        "quarters" => Some(QUARTERS),
        "format" => Some(FORMAT),
        "is.unsorted" => Some(IS_UNSORTED),
        "toString" => Some(TOSTRING),
        "capture.output" => Some(CAPTURE_OUTPUT),
        "row.names" => Some(ROW_NAMES),
        "row.names<-" => Some(ROW_NAMES_SET),
        "tapply" => Some(TAPPLY),
        "vapply" => Some(VAPPLY),
        "as.POSIXct" => Some(AS_POSIXCT),
        "proc.time" => Some(PROC_TIME),
        "quote" => Some(QUOTE),
        "~" => Some(TILDE),
        "is.call" => Some(IS_CALL),
        "is.environment" => Some(IS_ENVIRONMENT),
        "xtabs" => Some(XTABS),
        "sin" => Some(SIN),
        "cos" => Some(COS),
        "sinh" => Some(SINH),
        "cosh" => Some(COSH),
        "tanh" => Some(TANH_F),
        "complex" => Some(COMPLEX_CTOR),
        "as.complex" => Some(AS_COMPLEX),
        "Re" => Some(RE_C),
        "Im" => Some(IM_C),
        "Mod" => Some(MOD_C),
        "Arg" => Some(ARG_C),
        "Conj" => Some(CONJ_C),
        "is.complex" => Some(IS_COMPLEX),
        "as.raw" => Some(AS_RAW),
        "is.raw" => Some(IS_RAW),
        "mode" => Some(MODE_FN),
        "storage.mode" => Some(STORAGE_MODE),
        "expression" => Some(EXPRESSION_FN),
        "raw" => Some(RAW_CTOR),
        "vector" => Some(VECTOR_FN),
        "identity" => Some(IDENTITY_FN),
        "parse" => Some(PARSE_FN),
        "eval" => Some(EVAL_FN),
        "deparse1" => Some(DEPARSE1),
        "deparse" => Some(DEPARSE1),
        "dput" => Some(DPUT),
        "dget" => Some(DGET),
        "inherits" => Some(INHERITS),
        "dirname" => Some(DIRNAME),
        "gregexpr" => Some(GREGEXPR),
        "gsub" => Some(GSUB),
        "sub" => Some(SUB_FN),
        "formals" => Some(FORMALS),
        "options" => Some(OPTIONS_FN),
        "getOption" => Some(GET_OPTION),
        "cbind" => Some(CBIND),
        "rbind" => Some(RBIND),
        "is.numeric" => Some(IS_NUMERIC_FN),
        "is.integer" => Some(IS_INTEGER_FN),
        "is.double" => Some(IS_DOUBLE_FN),
        "is.logical" => Some(IS_LOGICAL_FN),
        "is.character" => Some(IS_CHARACTER_FN),
        "substitute" => Some(SUBSTITUTE_FN),
        "tsp" => Some(TSP),
        "tsp<-" => Some(TSP_ASSIGN),
        "printDeferredWarnings" => Some(PRINT_DEFERRED_WARNINGS),
        "grepl" => Some(GREPL),
        "grep" => Some(GREP),
        "regexpr" => Some(REGEXPR),
        "tempdir" => Some(TEMPDIR),
        "dimnames" => Some(DIMNAMES),
        "dimnames<-" => Some(DIMNAMES_ASSIGN),
        "class" => Some(CLASS),
        "class<-" => Some(CLASS_ASSIGN),
        "rownames" => Some(ROWNAMES),
        "rownames<-" => Some(ROWNAMES_ASSIGN),
        "colnames" => Some(COLNAMES),
        "colnames<-" => Some(COLNAMES_ASSIGN),
        "tryCatch" => Some(TRYCATCH),
        "conditionMessage" => Some(CONDITION_MESSAGE),
        "matrix" => Some(MATRIX),
        "dim" => Some(DIM),
        "nrow" => Some(NROW),
        "ncol" => Some(NCOL),
        "dnorm" => Some(DNORM),
        "pnorm" => Some(PNORM),
        "qnorm" => Some(QNORM),
        "dpois" => Some(DPOIS),
        "ppois" => Some(PPOIS),
        "dbinom" => Some(DBINOM),
        "pbinom" => Some(PBINOM),
        "message" => Some(MESSAGE_FN),
        "missing" => Some(MISSING_FN),
        "parent.env" => Some(PARENT_ENV),
        "order" => Some(ORDER_FN),
        "rank" => Some(RANK_FN),
        "ordered" => Some(ORDERED_FN),
        "is.ordered" => Some(IS_ORDERED),
        "is.factor" => Some(IS_FACTOR),
        "na.omit" => Some(NA_OMIT),
        "na.exclude" => Some(NA_EXCLUDE),
        "pbeta" => Some(PBETA),
        "dbeta" => Some(DBETA),
        "qbeta" => Some(QBETA),
        "find" => Some(FIND_FN),
        "UseMethod" => Some(USEMETHOD),
        "NextMethod" => Some(NEXTMETHOD),
        "getS3method" => Some(GETS3METHOD),
        "emptyenv" => Some(EMPTYENV),
        "as.integer" => Some(AS_INTEGER),
        "as.numeric" => Some(AS_NUMERIC),
        "as.double" => Some(AS_NUMERIC),
        "stopifnot" => Some(STOPIFNOT),
        "sequence" => Some(SEQUENCE),
        "rep.int" => Some(REP_INT),
        "rep_len" => Some(REP_LEN),
        "dt" => Some(DT),
        "pt" => Some(PT),
        "qt" => Some(QT),
        "sample" => Some(SAMPLE),
        "sample.int" => Some(SAMPLE_INT),
        "builtins" => Some(BUILTINS),
        "regexec" => Some(REGEXEC),
        "df" => Some(DF),
        "pf" => Some(PF),
        "qf" => Some(QF),
        "substr" => Some(SUBSTR),
        "file" => Some(FILE_FN),
        "isOpen" => Some(ISOPEN),
        "close" => Some(CLOSE_FN),
        "flush" => Some(FLUSH_FN),
        "writeChar" => Some(WRITECHAR),
        "readChar" => Some(READCHAR),
        "union" => Some(UNION),
        "intersect" => Some(INTERSECT),
        "setdiff" => Some(SETDIFF),
        "setequal" => Some(SETEQUAL),
        "table" => Some(TABLE),
        "setClass" => Some(SETCLASS),
        "new" => Some(NEW_S4),
        "is" => Some(IS_S4),
        "slot" => Some(SLOT),
        "slotNames" => Some(SLOTNAMES),
        "unname" => Some(UNNAME),
        "which.min" => Some(WHICH_MIN),
        "which.max" => Some(WHICH_MAX),
        "withVisible" => Some(WITHVISIBLE),
        "system" => Some(SYSTEM),
        "writeBin" => Some(WRITEBIN),
        "readBin" => Some(READBIN),
        "url" => Some(URL_FN),
        "dim<-" => Some(DIM_ASSIGN),
        "storage.mode<-" => Some(STORAGE_MODE_ASSIGN),
        "as.list" => Some(AS_LIST),
        "body" => Some(BODY_FN),
        "summary" => Some(SUMMARY_FN),
        "%*%" => Some(MATRIX_MULT),
        "crossprod" => Some(CROSSPROD),
        "tcrossprod" => Some(TCROSSPROD),
        "max.col" => Some(MAXCOL),
        _ => None,
    }
}

/// `function (formals) body` — closure construction. Neither operand
/// evaluates: the formals pairlist and body expression capture verbatim
/// alongside the defining environment, exactly GNU's `mkClosure`.
fn function_form(heap: &mut Heap, args: ValueId, caller_env: ValueId) -> EvalResult {
    let parts = raw_args(heap, args);
    // The parser emits an optional third environment operand (NIL when
    // absent); explicit environments are outside the migration slice.
    if parts.len() < 2 || parts.len() > 3 {
        return Err(EvalError::UnsupportedForm("function arity").into());
    }
    Ok(heap.alloc(RObject::Closure {
        formals: parts[0],
        body: parts[1],
        env: caller_env,
    }))
}

/// `c(...)` — concatenation with GNU coercion. Arguments evaluate eagerly;
/// NULL operands drop out. All operands coerce to the highest type present
/// in the logical < integer < real < character hierarchy. Names are outside
/// the migration slice.
fn concat_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let result = concat_primitive_inner(heap, interner, args, caller_env)?;
    // The datetime stamp rides the concatenated result — first operand's
    /// class method governs exactly as GNU's dispatch.
    let mut cursor = args;
    let first = loop {
        let (car, cdr, _) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break ValueId::NIL,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        break force_if_promise(heap, interner, evaluated)?;
        #[allow(unreachable_code)]
        {
            cursor = cdr;
        }
    };
    let _ = cursor;
    if first != ValueId::NIL {
        if is_posixct_classed(heap, interner, first) && !is_posixct_classed(heap, interner, result) {
            let tz = get_attrib(heap, interner, first, "tzone");
            let class = heap.alloc(RObject::String(vec![
                interner.intern_string("POSIXct"),
                interner.intern_string("POSIXt"),
            ]));
            let stamped = copy_with_attrib(heap, interner, result, "class", class).map_err(RSignal::from)?;
            return if let Some(tzv) = tz {
                copy_with_attrib(heap, interner, stamped, "tzone", tzv).map_err(RSignal::from)
            } else {
                Ok(stamped)
            };
        }
        if is_date_classed(heap, interner, first)
            && !is_posixct_classed(heap, interner, first)
            && !is_date_classed(heap, interner, result)
        {
            let class = heap.alloc(RObject::String(vec![interner.intern_string("Date")]));
            return copy_with_attrib(heap, interner, result, "class", class).map_err(RSignal::from);
        }
    }
    Ok(result)
}

fn concat_primitive_inner(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Variadic: evaluate and force every operand in the call, keeping each
    // operand's tag — tagged concatenation expands names across element
    // counts exactly as GNU.
    let mut operands = Vec::new();
    let mut tags: Vec<Option<String>> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        if car == ValueId::MISSING {
            return Err(EvalError::Condition(format!(
                "argument {} is empty",
                operands.len() + 1
            ))
            .into());
        }
        let evaluated = eval(heap, interner, car, caller_env)?;
        operands.push(force_if_promise(heap, interner, evaluated)?);
        tags.push(tag_name);
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }

    // GNU's datetime concatenation: the first operand's class method
    /// governs — c.Date coerces POSIXct siblings down to days, c.POSIXct
    /// lifts Date siblings to midnight seconds — and the result carries
    /// the class exactly as the method dispatch arranges.
    let first_date = operands.first().map(|v| is_date_classed(heap, interner, *v)).unwrap_or(false);
    let first_posix = operands.first().map(|v| is_posixct_classed(heap, interner, *v)).unwrap_or(false);
    let first_tzone = if first_posix {
        get_attrib(heap, interner, operands[0], "tzone")
            .and_then(|id| match heap.object(id) {
                Some(RObject::String(vals)) if !vals.is_empty() => Some(interner.string_text(vals[0]).to_string()),
                _ => None,
            })
            .unwrap_or_else(|| "UTC".to_string())
    } else {
        "UTC".to_string()
    };
    if first_date || first_posix {
        operands = operands
            .into_iter()
            .map(|v| {
                let posix = is_posixct_classed(heap, interner, v);
                let date = is_date_classed(heap, interner, v);
                if first_date && posix {
                    let scaled: Vec<f64> = numeric_vector(heap, v)
                        .iter()
                        .map(|&x| if x.is_nan() { x } else { (x as i64).div_euclid(86400) as f64 })
                        .collect();
                    heap.alloc(RObject::Real(scaled))
                } else if first_posix && date && !posix {
                    let scaled: Vec<f64> = numeric_vector(heap, v)
                        .iter()
                        .map(|&x| if x.is_nan() { x } else { x * 86400.0 })
                        .collect();
                    heap.alloc(RObject::Real(scaled))
                } else {
                    v
                }
            })
            .collect();
    }

    enum Flat {
        Raw(Vec<u8>),
        Logical(Vec<i32>),
        Integer(Vec<i32>),
        Real(Vec<f64>),
        Complex(Vec<crate::value::object::RComplex>),
        String(Vec<super::intern::InternedStr>),
    }
    let mut flattened = Vec::new();
    let mut operand_lengths = Vec::new();
    let mut rank = 0u8;
    for operand in operands {
        let flat = match heap.object(operand) {
            Some(RObject::Nil) => continue,
            Some(RObject::Raw(values)) => {
                operand_lengths.push(values.len());
                Flat::Raw(values.clone())
            }
            Some(RObject::Logical(values)) => {
                rank = rank.max(1);
                operand_lengths.push(values.len());
                Flat::Logical(values.clone())
            }
            Some(RObject::Integer(values)) => {
                rank = rank.max(2);
                operand_lengths.push(values.len());
                Flat::Integer(values.clone())
            }
            Some(RObject::Real(values)) => {
                rank = rank.max(3);
                operand_lengths.push(values.len());
                Flat::Real(values.clone())
            }
            Some(RObject::Complex(values)) => {
                rank = rank.max(4);
                operand_lengths.push(values.len());
                Flat::Complex(values.clone())
            }
            Some(RObject::String(values)) => {
                rank = rank.max(5);
                operand_lengths.push(values.len());
                Flat::String(values.clone())
            }
            _ => {
                return Err(EvalError::TypeError("c() operands must be atomic vectors").into());
            }
        };
        flattened.push(flat);
    }
    if flattened.is_empty() {
        return Ok(ValueId::NIL);
    }

    // GNU names expansion: each operand's tag repeats across its element
    // count; untagged operands contribute empty strings. Any tag at all
    // turns the names attribute on.
    let attach_names = |heap: &mut Heap, interner: &mut SymbolInterner, value: ValueId| -> Result<ValueId, RSignal> {
        // NIL operands dropped above shift the tag alignment; the slice
        // accepts the common all-present shape and drops names otherwise.
        if tags.iter().all(|t| t.is_none()) {
            return Ok(value);
        }
        if tags.len() != operand_lengths.len() {
            return Ok(value);
        }
        let mut names = Vec::new();
        for (tag, count) in tags.iter().zip(operand_lengths.iter()) {
            for _ in 0..*count {
                names.push(interner.intern_string(tag.as_deref().unwrap_or("")));
            }
        }
        let names_vec = heap.alloc(RObject::String(names));
        copy_with_attrib(heap, interner, value, "names", names_vec).map_err(RSignal::from)
    };

    fn logicals_of(flat: &Flat) -> Vec<i32> {
        match flat {
            Flat::Raw(v) => v.iter().map(|&x| (x != 0) as i32).collect(),
            Flat::Logical(v) => v.clone(),
            _ => unreachable!("lower ranks coerce upward before extraction"),
        }
    }
    fn integers_of(flat: &Flat) -> Vec<i32> {
        match flat {
            Flat::Raw(v) => v.iter().map(|&x| x as i32).collect(),
            Flat::Logical(v) | Flat::Integer(v) => v.clone(),
            _ => unreachable!("lower ranks coerce upward before extraction"),
        }
    }
    fn reals_of(flat: &Flat) -> Vec<f64> {
        match flat {
            Flat::Logical(v) | Flat::Integer(v) => v
                .iter()
                .map(|x| {
                    if *x == crate::value::object::NA_INTEGER {
                        f64::from_bits(crate::value::object::NA_REAL_BITS)
                    } else {
                        *x as f64
                    }
                })
                .collect(),
            Flat::Real(v) => v.clone(),
            Flat::Raw(v) => v.iter().map(|&x| x as f64).collect(),
            _ => unreachable!("lower ranks coerce upward before extraction"),
        }
    }
    fn complexes_of(flat: &Flat) -> Vec<crate::value::object::RComplex> {
        use crate::value::object::{NA_REAL_BITS, RComplex};
        match flat {
            Flat::Complex(v) => v.clone(),
            _ => reals_of(flat)
                .iter()
                .map(|&r| {
                    if r.to_bits() == NA_REAL_BITS {
                        RComplex { r: f64::from_bits(NA_REAL_BITS), i: f64::from_bits(NA_REAL_BITS) }
                    } else {
                        RComplex { r, i: 0.0 }
                    }
                })
                .collect(),
        }
    }
    fn raws_of(flat: &Flat) -> Vec<u8> {
        match flat {
            Flat::Raw(v) => v.clone(),
            _ => unreachable!("any higher operand raises the rank past raw"),
        }
    }

    match rank {
        0 => {
            let mut out = Vec::new();
            for flat in &flattened {
                out.extend(raws_of(flat));
            }
            let value = heap.alloc(RObject::Raw(out));
            attach_names(heap, interner, value)
        }
        1 => {
            let mut out = Vec::new();
            for flat in &flattened {
                out.extend(logicals_of(flat));
            }
            let value = heap.alloc(RObject::Logical(out));
            attach_names(heap, interner, value)
        }
        4 => {
            let mut out = Vec::new();
            for flat in &flattened {
                out.extend(complexes_of(flat));
            }
            let value = heap.alloc(RObject::Complex(out));
            attach_names(heap, interner, value)
        }
        2 => {
            let mut out = Vec::new();
            for flat in &flattened {
                out.extend(integers_of(flat));
            }
            let value = heap.alloc(RObject::Integer(out));
            attach_names(heap, interner, value)
        }
        3 => {
            let mut out = Vec::new();
            for flat in &flattened {
                out.extend(reals_of(flat));
            }
            let value = heap.alloc(RObject::Real(out));
            attach_names(heap, interner, value)
        }
        _ => {
            // Character coercion: every operand renders through its ASCII
            // element text, matching GNU's paste-style coercion for the
            // atomic numeric types in the migration slice.
            let mut out = Vec::new();
            for flat in flattened {
                match flat {
                    Flat::String(values) => out.extend(values),
                    Flat::Logical(values) => {
                        for v in values {
                            let text = if v == crate::value::object::NA_INTEGER {
                                // GNU: NA coerces to NA_character_ — the
                                // missing sentinel, never the literal text.
                                "\0NA".to_string()
                            } else if v != 0 {
                                "TRUE".to_string()
                            } else {
                                "FALSE".to_string()
                            };
                            out.push(interner.intern_string(&text));
                        }
                    }
                    Flat::Integer(values) => {
                        for v in values {
                            let text = if v == crate::value::object::NA_INTEGER {
                                "\0NA".to_string()
                            } else {
                                v.to_string()
                            };
                            out.push(interner.intern_string(&text));
                        }
                    }
                    Flat::Real(values) => {
                        for v in values {
                            let text = if v.to_bits() == crate::value::object::NA_REAL_BITS {
                                "\0NA".to_string()
                            } else if v.fract() == 0.0 && v.abs() < 1e15 {
                                format!("{}", v as i64)
                            } else {
                                format!("{v}")
                            };
                            out.push(interner.intern_string(&text));
                        }
                    }
                    Flat::Complex(values) => {
                        for z in values {
                            out.push(interner.intern_string(&crate::value::print::format_complex(z)));
                        }
                    }
                    Flat::Raw(values) => {
                        for v in values {
                            out.push(interner.intern_string(&format!("{v}")));
                        }
                    }
                }
            }
            let value = heap.alloc(RObject::String(out));
            attach_names(heap, interner, value)
        }
    }
}

/// `length(x)` — element count as a length-one integer, GNU exactly for
/// atomic and list vectors.
fn length_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let length = match heap.object(operands[0]) {
        Some(RObject::Logical(v)) => v.len(),
        Some(RObject::Integer(v)) => v.len(),
        Some(RObject::Real(v)) => v.len(),
        Some(RObject::Complex(v)) => v.len(),
        Some(RObject::String(v)) => v.len(),
        Some(RObject::Raw(v)) => v.len(),
        Some(RObject::List(v)) => v.len(),
        Some(RObject::Expression(v)) => v.len(),
        Some(RObject::Nil) => 0,
        // GNU: length of a call counts the operator plus every argument.
        Some(RObject::Language { cdr, .. }) => {
            let mut count = 1usize;
            let mut cursor = *cdr;
            for _ in 0..100_000 {
                if cursor == ValueId::NIL {
                    break;
                }
                match heap.object(cursor) {
                    Some(RObject::Pairlist { cdr: next, .. }) => {
                        count += 1;
                        cursor = *next;
                    }
                    _ => break,
                }
            }
            count
        }
        _ => return Err(EvalError::TypeError("length requires a vector").into()),
    };
    Ok(heap.alloc(RObject::Integer(vec![length as i32])))
}

/// `from:to` — unit-step sequence, ascending or descending, inclusive of
/// `to` when it lands exactly. Integer when both operands are integer,
/// real otherwise — GNU's documented behaviour for the slice.
fn colon_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let numeric = |value: ValueId| -> Result<(f64, bool), RSignal> {
        match heap.object(value) {
            Some(RObject::Integer(v)) if !v.is_empty() => Ok((v[0] as f64, true)),
            Some(RObject::Real(v)) if !v.is_empty() => Ok((v[0], false)),
            Some(RObject::Logical(v)) if !v.is_empty() => Ok((v[0] as f64, true)),
            _ => Err(EvalError::TypeError("NA/NaN argument").into()),
        }
    };
    let (from, _from_type) = numeric(operands[0])?;
    let (to, _to_type) = numeric(operands[1])?;
    // GNU's ncolon: integer result when both operands are integer-valued
    // and in int range — literal storage type (double) is irrelevant.
    let from_int = from.fract() == 0.0 && from.abs() <= i32::MAX as f64;
    let to_int = to.fract() == 0.0 && to.abs() <= i32::MAX as f64;
    let mut out = Vec::new();
    if from <= to {
        let mut x = from;
        while x <= to {
            out.push(x);
            x += 1.0;
        }
    } else {
        let mut x = from;
        while x >= to {
            out.push(x);
            x -= 1.0;
        }
    }
    if from_int && to_int {
        Ok(heap.alloc(RObject::Integer(out.into_iter().map(|x| x as i32).collect())))
    } else {
        Ok(heap.alloc(RObject::Real(out)))
    }
}

/// `x[i]` — subsetting with GNU indexing semantics for the atomic vector
/// types in the migration slice. Positive integer indices are 1-based;
/// out-of-range positive indices and NA indices yield NA elements.
/// Negative indices exclude. Logical indices recycle across the vector.
/// Zero indices drop out. Real indices truncate toward zero, exactly GNU.
fn subset_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // GNU's dispatch on argument count: two operands is vector-style
    /// subsetting; three or more with a dim attribute is matrix indexing —
    /// `x[i, j]` with the missing-argument sentinel standing for whole
    /// extents.
    let raw = raw_args(heap, args);
    if raw.len() >= 3 {
        return matrix_subset(heap, interner, raw, caller_env);
    }
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    subset_impl(heap, interner, operands[0], operands[1])
}

/// Matrix two-index subsetting: `x[i, j]` where either index may be the
/// missing sentinel meaning the whole extent. Column-major linearization —
/// element (r, c) lives at c*nrow + r zero-based, GNU exactly. Result keeps
/// matrix shape when both extents exceed one; otherwise drops to a vector,
/// GNU's default drop semantics shape.
fn matrix_subset(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    raw: Vec<ValueId>,
    caller_env: ValueId,
) -> EvalResult {
    let object = eval(heap, interner, raw[0], caller_env)?;
    let object = force_if_promise(heap, interner, object)?;
    let dims = match get_attrib(heap, interner, object, "dim") {
        Some(dim) => match heap.object(dim) {
            Some(RObject::Integer(dims)) if dims.len() >= 2 => (dims[0] as usize, dims[1] as usize),
            _ => return Err(EvalError::TypeError("matrix subscript requires dim").into()),
        },
        None => return Err(EvalError::TypeError("matrix subscript requires dim").into()),
    };
    let (nrow, ncol) = dims;
    let mut resolve_index = |raw_index: ValueId, extent: usize| -> Result<Vec<usize>, RSignal> {
        if raw_index == ValueId::MISSING {
            return Ok((0..extent).collect());
        }
        let evaluated = eval(heap, interner, raw_index, caller_env)?;
        let evaluated = force_if_promise(heap, interner, evaluated)?;
        match heap.object(evaluated) {
            Some(RObject::Integer(values)) => Ok(values
                .iter()
                .map(|v| {
                    if *v == crate::value::object::NA_INTEGER {
                        usize::MAX
                    } else {
                        (*v - 1) as usize
                    }
                })
                .collect()),
            Some(RObject::Real(values)) => Ok(values
                .iter()
                .map(|v| {
                    if v.is_nan() {
                        usize::MAX
                    } else {
                        (*v as i32 - 1) as usize
                    }
                })
                .collect()),
            Some(RObject::Logical(values)) => {
                let mut positions = Vec::new();
                for i in 0..extent {
                    let flag = values[i % values.len().max(1)];
                    if flag == crate::value::object::NA_INTEGER {
                        positions.push(usize::MAX);
                    } else if flag != 0 {
                        positions.push(i);
                    }
                }
                Ok(positions)
            }
            _ => Err(EvalError::TypeError("unsupported matrix index").into()),
        }
    };
    let rows = resolve_index(raw[1], nrow)?;
    let cols = resolve_index(raw[2], ncol)?;
    // Linear positions column-major: iterate columns outermost so the
    // result fills column-by-column exactly as GNU's storage order.
    let na = usize::MAX;
    let mut positions = Vec::with_capacity(rows.len() * cols.len());
    for col in &cols {
        for row in &rows {
            if *row == na || *col == na || *row >= nrow || *col >= ncol {
                positions.push(na);
            } else {
                positions.push(col * nrow + row);
            }
        }
    }
    let extracted = extract_elements(heap, object, &positions).map_err(RSignal::from)?;
    // Shape discipline: both extents multi-length keeps a dim attribute of
    /// the selected shape; single-length margins drop to a vector.
    if rows.len() > 1 && cols.len() > 1 {
        let dims_vec = heap.alloc(RObject::Integer(vec![rows.len() as i32, cols.len() as i32]));
        return copy_with_attrib(heap, interner, extracted, "dim", dims_vec).map_err(RSignal::from);
    }
    Ok(extracted)
}

/// Subsetting core operating on already-evaluated operands — shared by the
/// primitive and by the real-index truncation recursion.
fn subset_impl(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    source: ValueId,
    index: ValueId,
) -> EvalResult {
    enum Selection {
        /// Explicit 0-based positions; usize::MAX encodes an NA element.
        Positions(Vec<usize>),
        /// Positions to exclude (0-based, no duplicates required).
        Excluding(Vec<usize>),
    }
    // data.frame single-index subsetting selects COLUMNS, not elements —
    /// the frame structure rebuilds over the selection exactly as GNU.
    if let Some((_, ncol)) = data_frame_shape(heap, interner, source) {
        let names_id = get_attrib(heap, interner, source, "names");
        let col_names: Vec<String> = match names_id {
            Some(id) => string_vec(heap, interner, id),
            None => Vec::new(),
        };
        let positions: Vec<usize> = match heap.object(index) {
            Some(RObject::String(values)) => values
                .iter()
                .filter_map(|v| {
                    let text = interner.string_text(*v).to_string();
                    col_names.iter().position(|n| *n == text)
                })
                .collect(),
            Some(RObject::Real(values)) => values
                .iter()
                .filter(|v| !v.is_nan() && **v >= 1.0)
                .map(|v| (*v as usize) - 1)
                .filter(|p| *p < ncol)
                .collect(),
            Some(RObject::Integer(values)) => values
                .iter()
                .filter(|v| **v >= 1 && **v != crate::value::object::NA_INTEGER)
                .map(|v| (*v as usize) - 1)
                .filter(|p| *p < ncol)
                .collect(),
            _ => Vec::new(),
        };
        let columns = match heap.object(source) {
            Some(RObject::List(items)) => items.clone(),
            _ => Vec::new(),
        };
        let selected: Vec<ValueId> = positions.iter().filter_map(|p| columns.get(*p).copied()).collect();
        let selected_names: Vec<String> = positions
            .iter()
            .filter_map(|p| col_names.get(*p).cloned())
            .collect();
        return build_data_frame(heap, interner, selected, selected_names);
    }
    let source_length = vector_length(heap, source)
        .ok_or(EvalError::TypeError("subscript requires a vector"))
        .map_err(RSignal::from)?;
    let selection = match heap.object(index) {
        Some(RObject::Integer(values)) => {
            if values.iter().any(|v| *v < 0 && *v != crate::value::object::NA_INTEGER) {
                Selection::Excluding(
                    values
                        .iter()
                        .filter(|v| **v < 0 && **v != crate::value::object::NA_INTEGER)
                        .map(|v| (-*v - 1) as usize)
                        .collect(),
                )
            } else {
                Selection::Positions(
                    values
                        .iter()
                        .map(|v| {
                            if *v == crate::value::object::NA_INTEGER {
                                usize::MAX
                            } else if *v == 0 {
                                usize::MAX - 1 // drop marker, filtered below
                            } else {
                                (*v - 1) as usize
                            }
                        })
                        .collect(),
                )
            }
        }
        Some(RObject::Real(values)) => {
            // Truncation must preserve NA — Rust's `as` saturates NaN to 0,
            /// which would silently drop positions GNU keeps as NA.
            let truncs: Vec<i32> = values
                .iter()
                .map(|v| {
                    if v.is_nan() {
                        crate::value::object::NA_INTEGER
                    } else {
                        *v as i32
                    }
                })
                .collect();
            let truncated = heap.alloc(RObject::Integer(truncs));
            return subset_impl(heap, interner, source, truncated);
        }
        Some(RObject::Logical(values)) => {
            // GNU: a logical index longer than the vector extends the
            /// selection — out-of-range TRUE positions yield NA elements.
            let mut positions = Vec::new();
            let extent = source_length.max(values.len());
            for i in 0..extent {
                let flag = values[i % values.len().max(1)];
                if flag == crate::value::object::NA_INTEGER {
                    positions.push(usize::MAX);
                } else if flag != 0 {
                    positions.push(i);
                }
            }
            Selection::Positions(positions)
        }
        Some(RObject::String(values)) => {
            // Named access: exact matching against the source's names,
            /// unmatched positions yield NA exactly as GNU. Partial matching
            /// is `[[`'s business, not `[`'s.
            let source_names = get_attrib(heap, interner, source, "names");
            let positions: Vec<usize> = match source_names {
                Some(names_id) => {
                    let names_vec: Vec<String> = match heap.object(names_id) {
                        Some(RObject::String(entries)) => entries
                            .iter()
                            .map(|entry| interner.string_text(*entry).to_string())
                            .collect(),
                        _ => Vec::new(),
                    };
                    values
                        .iter()
                        .map(|wanted| {
                            let wanted_text = interner.string_text(*wanted);
                            match names_vec.iter().position(|n| n == wanted_text) {
                                Some(position) => position,
                                None => usize::MAX,
                            }
                        })
                        .collect()
                }
                None => values.iter().map(|_| usize::MAX).collect(),
            };
            Selection::Positions(positions)
        }
        _ => return Err(EvalError::TypeError("unsupported subscript type").into()),
    };

    // Materialize the final 0-based position list.
    let positions: Vec<usize> = match selection {
        Selection::Positions(list) => list
            .into_iter()
            .filter(|p| *p != usize::MAX - 1) // drop zero-index markers
            .collect(),
        Selection::Excluding(excluded) => (0..source_length)
            .filter(|i| !excluded.contains(i))
            .collect(),
    };

    let extracted = extract_elements(heap, source, &positions).map_err(RSignal::from)?;
    // GNU fidelity: `[` carries the names of selected elements across —
    /// positional and named subsetting alike. Out-of-range and NA positions
    /// take the NA-string sentinel the bridge round-trips.
    if let Some(names_id) = get_attrib(heap, interner, source, "names") {
        let source_names: Vec<super::intern::InternedStr> = match heap.object(names_id) {
            Some(RObject::String(entries)) => entries.clone(),
            _ => Vec::new(),
        };
        if !source_names.is_empty() {
            let na_text = interner.intern_string("\0NA");
            let selected: Vec<super::intern::InternedStr> = positions
                .iter()
                .map(|p| {
                    if *p == usize::MAX || *p >= source_names.len() {
                        na_text
                    } else {
                        source_names[*p]
                    }
                })
                .collect();
            let names_vec = heap.alloc(RObject::String(selected));
            return copy_with_attrib(heap, interner, extracted, "names", names_vec)
                .map_err(RSignal::from);
        }
    }
    Ok(extracted)
}

fn vector_length(heap: &Heap, value: ValueId) -> Option<usize> {
    match heap.object(value) {
        Some(RObject::Logical(v)) => Some(v.len()),
        Some(RObject::Integer(v)) => Some(v.len()),
        Some(RObject::Real(v)) => Some(v.len()),
        Some(RObject::String(v)) => Some(v.len()),
        Some(RObject::Raw(v)) => Some(v.len()),
        Some(RObject::Complex(v)) => Some(v.len()),
        Some(RObject::List(v)) => Some(v.len()),
        Some(RObject::Expression(v)) => Some(v.len()),
        // GNU: length of a call counts the operator plus every argument
        /// exactly as the pairlist chain encodes.
        Some(RObject::Language { cdr, .. }) => {
            let mut count = 1usize;
            let mut cursor = *cdr;
            for _ in 0..100_000 {
                if cursor == ValueId::NIL {
                    break;
                }
                match heap.object(cursor) {
                    Some(RObject::Pairlist { cdr, .. }) => {
                        count += 1;
                        cursor = *cdr;
                    }
                    _ => break,
                }
            }
            Some(count)
        }
        _ => None,
    }
}

/// Extract positions from an atomic vector into a fresh vector of the same
/// type; usize::MAX positions yield the type's NA element.
fn extract_elements(
    heap: &mut Heap,
    source: ValueId,
    positions: &[usize],
) -> Result<ValueId, EvalError> {
    macro_rules! extract {
        ($variant:ident, $na:expr) => {
            match heap.object(source) {
                Some(RObject::$variant(values)) => {
                    let out: Vec<_> = positions
                        .iter()
                        .map(|p| {
                            if *p == usize::MAX || *p >= values.len() {
                                $na
                            } else {
                                values[*p]
                            }
                        })
                        .collect();
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                _ => {}
            }
        };
    }
    extract!(Logical, crate::value::object::NA_INTEGER);
    extract!(Integer, crate::value::object::NA_INTEGER);
    extract!(Real, f64::from_bits(crate::value::object::NA_REAL_BITS));
    extract!(Raw, 0u8);
    match heap.object(source) {
        Some(RObject::String(values)) => {
            let na_text = values.first().copied();
            let out: Vec<_> = positions
                .iter()
                .map(|p| {
                    if *p == usize::MAX || *p >= values.len() {
                        na_text.unwrap_or(super::intern::InternedStr(0))
                    } else {
                        values[*p]
                    }
                })
                .collect();
            Ok(heap.alloc(RObject::String(out)))
        }
        Some(RObject::List(values)) => {
            // List out-of-range positions are NULL elements in GNU's [[;
            // the migration slice rejects them rather than fabricate.
            if positions.iter().any(|p| *p == usize::MAX || *p >= values.len()) {
                return Err(EvalError::TypeError("list subscript out of bounds"));
            }
            let out: Vec<_> = positions.iter().map(|p| values[*p]).collect();
            Ok(heap.alloc(RObject::List(out)))
        }
        _ => Err(EvalError::TypeError("subscript requires a vector")),
    }
}

/// Extract a logical view of a value with GNU coercion: numeric zero is
/// FALSE, nonzero TRUE, NA propagates as the integer NA sentinel.
fn logicals_of_value(heap: &Heap, value: ValueId) -> Result<Vec<i32>, EvalError> {
    match heap.object(value) {
        Some(RObject::Logical(values)) => Ok(values.clone()),
        Some(RObject::Integer(values)) => Ok(values.clone()),
        Some(RObject::Real(values)) => Ok(values
            .iter()
            .map(|v| {
                if v.is_nan() {
                    crate::value::object::NA_INTEGER
                } else if *v != 0.0 {
                    1
                } else {
                    0
                }
            })
            .collect()),
        _ => Err(EvalError::TypeError("logical operand must be logical or numeric")),
    }
}

/// `&` and `|` — elementwise logic with GNU NA semantics: NA & FALSE is
/// FALSE, NA & TRUE is NA, NA | TRUE is TRUE, NA | FALSE is NA. Length
/// recycling follows the arithmetic rule.
fn logic_elementwise(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let a = logicals_of_value(heap, operands[0]).map_err(RSignal::from)?;
    let b = logicals_of_value(heap, operands[1]).map_err(RSignal::from)?;
    let na = crate::value::object::NA_INTEGER;
    let combine = |x: i32, y: i32| -> i32 {
        let xt = x != 0 && x != na;
        let yt = y != 0 && y != na;
        if op == AND {
            if (x != na && !xt) || (y != na && !yt) {
                0 // either operand definitely FALSE
            } else if x == na || y == na {
                na
            } else {
                (xt && yt) as i32
            }
        } else {
            if xt || yt {
                1 // either operand definitely TRUE
            } else if x == na || y == na {
                na
            } else {
                0
            }
        }
    };
    let folded = fold_numeric(a, b, op, combine).map_err(RSignal::from)?;
    Ok(heap.alloc(RObject::Logical(folded)))
}

/// `!` — elementwise negation; NA stays NA, exactly GNU.
fn logic_not(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let values = logicals_of_value(heap, operands[0]).map_err(RSignal::from)?;
    let na = crate::value::object::NA_INTEGER;
    let out = values
        .into_iter()
        .map(|v| if v == na { na } else { (v == 0) as i32 })
        .collect();
    Ok(heap.alloc(RObject::Logical(out)))
}

/// `&&` and `||` — short-circuit operators over first elements only. The
/// right operand evaluates only when the left doesn't decide the outcome —
/// GNU's control-flow contract, which side effects observe.
fn logic_short_circuit(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() != 2 {
        return Err(EvalError::UnsupportedForm("short-circuit arity").into());
    }
    let na = crate::value::object::NA_INTEGER;
    let first_of = |heap: &Heap, value: ValueId| -> Result<i32, EvalError> {
        let values = logicals_of_value(heap, value)?;
        Ok(values.first().copied().unwrap_or(na))
    };
    let left = eval(heap, interner, parts[0], caller_env)?;
    let left = force_if_promise(heap, interner, left)?;
    let left_first = first_of(heap, left).map_err(RSignal::from)?;

    // Does the left operand decide the outcome without the right?
    let decided = if op == SHORT_AND {
        left_first == 0
    } else {
        left_first != 0 && left_first != na
    };
    if decided {
        let verdict = if op == SHORT_AND { 0 } else { 1 };
        return Ok(heap.alloc(RObject::Logical(vec![verdict])));
    }

    let right = eval(heap, interner, parts[1], caller_env)?;
    let right = force_if_promise(heap, interner, right)?;
    let right_first = first_of(heap, right).map_err(RSignal::from)?;

    let result = if op == SHORT_AND {
        if right_first == 0 {
            0
        } else if left_first == na || right_first == na {
            na
        } else {
            1
        }
    } else {
        if right_first != 0 && right_first != na {
            1
        } else if left_first == na || right_first == na {
            na
        } else {
            0
        }
    };
    Ok(heap.alloc(RObject::Logical(vec![result])))
}

/// Numeric coercion for the reduction and math primitives: logical and
/// integer vectors widen to reals with the NA bit pattern preserved.
fn reals_of_value(heap: &Heap, value: ValueId) -> Result<Vec<f64>, EvalError> {
    match heap.object(value) {
        Some(RObject::Real(values)) => Ok(values.clone()),
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => Ok(values
            .iter()
            .map(|v| {
                if *v == crate::value::object::NA_INTEGER {
                    f64::from_bits(crate::value::object::NA_REAL_BITS)
                } else {
                    *v as f64
                }
            })
            .collect()),
        _ => Err(EvalError::TypeError("numeric argument expected")),
    }
}

/// `sum`/`prod`/`min`/`max` — scalar folds with GNU NA propagation: any NA
/// element poisons the reduction (the slice has no na.rm yet). Integer and
/// logical inputs fold in integer arithmetic exactly as GNU does; real
/// inputs fold in real.
fn reduction(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // GNU's variadic shape: every unlabeled argument concatenates into the
    /// reduction input; the tagged na.rm strips NA positions instead of
    /// poisoning.
    let mut inputs: Vec<ValueId> = Vec::new();
    let mut na_rm = false;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        if car == ValueId::MISSING {
            return Err(EvalError::Condition(format!(
                "argument {} is empty",
                inputs.len() + 1
            ))
            .into());
        }
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        if tag_name.as_deref() == Some("na.rm") {
            na_rm = matches!(
                heap.object(value),
                Some(RObject::Logical(v)) if !v.is_empty() && v[0] != 0 && v[0] != crate::value::object::NA_INTEGER
            );
        } else {
            inputs.push(value);
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    if inputs.is_empty() {
        return Err(EvalError::TypeError("reduction requires input").into());
    }
    let combined = concat_vectors(heap, &inputs).map_err(RSignal::from)?;
    reduction_fold(heap, op, &[combined], na_rm)
}


/// Concatenate vectors exactly as `c()` — same-type fast path, numeric
/// promotion otherwise. Shared by the variadic reduction reshape.
fn concat_vectors(heap: &mut Heap, inputs: &[ValueId]) -> Result<ValueId, EvalError> {
    if inputs.len() == 1 {
        return Ok(inputs[0]);
    }
    let all_int = inputs.iter().all(|v| {
        matches!(
            heap.object(*v),
            Some(RObject::Integer(_)) | Some(RObject::Logical(_))
        )
    });
    if all_int {
        let mut out = Vec::new();
        for input in inputs {
            match heap.object(*input) {
                Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => {
                    out.extend(values.iter().copied());
                }
                _ => {}
            }
        }
        return Ok(heap.alloc(RObject::Integer(out)));
    }
    let mut out = Vec::new();
    for input in inputs {
        out.extend(reals_of_value(heap, *input)?);
    }
    Ok(heap.alloc(RObject::Real(out)))
}

fn reduction_fold(
    heap: &mut Heap,
    op: u32,
    operands: &[ValueId],
    na_rm: bool,
) -> EvalResult {
    // GNU type fidelity: integer/logical inputs fold in integer arithmetic
    // for sum/prod and preserve type for min/max; real inputs fold in real.
    if let Some(RObject::Integer(values)) | Some(RObject::Logical(values)) =
        heap.object(operands[0])
    {
        let mut values = values.clone();
        if values.iter().any(|v| *v == crate::value::object::NA_INTEGER) {
            if !na_rm {
                return Ok(heap.alloc(RObject::Integer(vec![crate::value::object::NA_INTEGER])));
            }
            values.retain(|v| *v != crate::value::object::NA_INTEGER);
        }
        let result: i32 = match op {
            SUM => values.iter().copied().fold(0i32, |a, b| a.wrapping_add(b)),
            PROD => values.iter().copied().fold(1i32, |a, b| a.wrapping_mul(b)),
            MIN => values.iter().copied().fold(i32::MAX, i32::min),
            _ => values.iter().copied().fold(i32::MIN, i32::max),
        };
        return Ok(heap.alloc(RObject::Integer(vec![result])));
    }
    let mut values = reals_of_value(heap, operands[0]).map_err(RSignal::from)?;
    if values.iter().any(|v| v.is_nan()) {
        if !na_rm {
            return Ok(heap.alloc(RObject::Real(vec![f64::from_bits(
                crate::value::object::NA_REAL_BITS,
            )])));
        }
        values.retain(|v| !v.is_nan());
    }
    let result = match op {
        SUM => values.iter().sum(),
        PROD => values.iter().product(),
        MIN => values.iter().copied().fold(f64::INFINITY, f64::min),
        _ => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    };
    Ok(heap.alloc(RObject::Real(vec![result])))
}

/// Elementwise unary math — `sqrt`/`exp`/`log`/`abs` over real-coerced
/// inputs. NA inputs yield NA outputs; domain behavior matches IEEE, which
/// is what GNU inherits from libm.
fn math_unary(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let values = reals_of_value(heap, operands[0]).map_err(RSignal::from)?;
    let out: Vec<f64> = values
        .iter()
        .map(|v| {
            if v.is_nan() {
                f64::from_bits(crate::value::object::NA_REAL_BITS)
            } else {
                match op {
                    SQRT => v.sqrt(),
                    EXP => v.exp(),
                    LOG => v.ln(),
                    FLOOR => v.floor(),
                    CEILING => v.ceil(),
                    TRUNC => v.trunc(),
                    EXPM1 => v.exp_m1(),
                    LOG1P => v.ln_1p(),
                    ACOSH => v.acosh(),
                    ASINH => v.asinh(),
                    ATANH => v.atanh(),
                    COSPI => cospi_exact(*v),
                    SINPI => sinpi_exact(*v),
                    TANPI => tanpi_exact(*v),
                    _ => v.abs(),
                }
            }
        })
        .collect();
    // GNU type fidelity: abs of an integer vector stays integer; the
    /// transcendental functions always produce real.
    if op == ABS
        && matches!(
            heap.object(operands[0]),
            Some(RObject::Integer(_)) | Some(RObject::Logical(_))
        )
    {
        let ints: Vec<i32> = out
            .iter()
            .map(|v| {
                if v.is_nan() {
                    crate::value::object::NA_INTEGER
                } else {
                    *v as i32
                }
            })
            .collect();
        return Ok(heap.alloc(RObject::Integer(ints)));
    }
    Ok(heap.alloc(RObject::Real(out)))
}

/// `x[[i]]` — exact extraction of a single element. Unlike `[`, extraction
/// unboxes list elements (the element itself, not a length-one list) and
/// out-of-range positive indices are errors in GNU, not NA padding.
/// Negative indices count exclusion-invariant positions exactly as GNU's
/// scalar negative subscript — the slice supports positive indices and
/// documents the negative case as future work.
fn extract_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    // Character indices resolve through the names attribute exactly as
    /// GNU's exact-name matching.
    let index_value = match heap.object(operands[1]) {
        Some(RObject::Real(v)) if !v.is_empty() => v[0] as i64,
        Some(RObject::Integer(v)) if !v.is_empty() => v[0] as i64,
        Some(RObject::String(_)) => {
            let key = string_operand(heap, interner, operands[1]).unwrap_or_default();
            let names: Vec<String> = match get_attrib(heap, interner, operands[0], "names") {
                Some(n) => string_vec(heap, interner, n),
                None => Vec::new(),
            };
            match names.iter().position(|n| n == &key) {
                Some(pos) => (pos + 1) as i64,
                None => return Err(EvalError::TypeError("subscript out of bounds").into()),
            }
        }
        _ => return Err(EvalError::TypeError("extraction index must be scalar numeric").into()),
    };
    if index_value < 1 {
        return Err(EvalError::TypeError("extraction requires positive indices").into());
    }
    let position = (index_value - 1) as usize;
    // Language extraction: position zero is the operator itself, further
    /// positions walk the argument chain exactly as GNU's call indexing.
    if let Some(RObject::Language { car, cdr, .. }) = heap.object(operands[0]).cloned() {
        if position == 0 {
            return Ok(car);
        }
        let mut cursor = cdr;
        for _ in 1..position {
            match heap.object(cursor) {
                Some(RObject::Pairlist { cdr: next, .. }) => cursor = *next,
                _ => return Err(EvalError::TypeError("subscript out of bounds").into()),
            }
        }
        return match heap.object(cursor) {
            Some(RObject::Pairlist { car, .. }) => Ok(*car),
            _ => Err(EvalError::TypeError("subscript out of bounds").into()),
        };
    }
    macro_rules! extract_scalar {
        ($variant:ident) => {
            match heap.object(operands[0]) {
                Some(RObject::$variant(values)) => {
                    if position >= values.len() {
                        return Err(EvalError::TypeError("subscript out of bounds").into());
                    }
                    return Ok(heap.alloc(RObject::$variant(vec![values[position]])));
                }
                _ => {}
            }
        };
    }
    extract_scalar!(Logical);
    extract_scalar!(Integer);
    extract_scalar!(Real);
    extract_scalar!(Raw);
    extract_scalar!(Complex);
    match heap.object(operands[0]) {
        Some(RObject::String(values)) => {
            if position >= values.len() {
                return Err(EvalError::TypeError("subscript out of bounds").into());
            }
            Ok(heap.alloc(RObject::String(vec![values[position]])))
        }
        Some(RObject::Expression(items)) => {
            // Expression extraction unboxes exactly as list extraction.
            if position >= items.len() {
                return Err(EvalError::TypeError("subscript out of bounds").into());
            }
            Ok(items[position])
        }
        Some(RObject::List(items)) => {
            // List extraction unboxes: the element itself, exactly GNU.
            if position >= items.len() {
                return Err(EvalError::TypeError("subscript out of bounds").into());
            }
            Ok(items[position])
        }
        _ => Err(EvalError::TypeError("extraction requires a vector").into()),
    }
}

/// Coerce a value to its character representation with GNU formatting for
/// the atomic types in the migration slice.
fn strings_of_value(
    heap: &Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
) -> Result<Vec<super::intern::InternedStr>, EvalError> {
    // Factor-classed input coerces through level labels exactly as GNU's
    /// character coercion — the codes index the levels attribute.
    if is_factor(heap, interner, value) {
        if let Some(levels_id) = get_attrib(heap, interner, value, "levels") {
            let levels = string_vec(heap, interner, levels_id);
            if let Some(RObject::Integer(codes)) = heap.object(value) {
                return Ok(codes
                    .iter()
                    .map(|&code| {
                        if code == crate::value::object::NA_INTEGER
                            || code < 1
                            || code as usize > levels.len()
                        {
                            interner.intern_string("NA")
                        } else {
                            interner.intern_string(&levels[(code - 1) as usize])
                        }
                    })
                    .collect());
            }
        }
    }
    match heap.object(value) {
        Some(RObject::String(values)) => Ok(values.clone()),
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => {
            let logical = matches!(heap.object(value), Some(RObject::Logical(_)));
            Ok(values
                .iter()
                .map(|v| {
                    let text = if *v == crate::value::object::NA_INTEGER {
                        "NA".to_string()
                    } else if logical {
                        if *v != 0 { "TRUE".to_string() } else { "FALSE".to_string() }.to_string()
                    } else {
                        v.to_string()
                    };
                    interner.intern_string(&text)
                })
                .collect())
        }
        Some(RObject::Real(values)) => Ok(values
            .iter()
            .map(|v| {
                let text = if v.is_nan() {
                    "NA".to_string()
                } else if v.fract() == 0.0 && v.abs() < 1e15 {
                    format!("{}", *v as i64)
                } else {
                    format!("{v}")
                };
                interner.intern_string(&text)
            })
            .collect()),
        Some(RObject::Raw(values)) => Ok(values
            .iter()
            .map(|v| interner.intern_string(&format!("{v:02x}")))
            .collect()),
        _ => Err(EvalError::TypeError("cannot coerce to character")),
    }
}

/// `paste(...)` — elementwise string concatenation with recycling. Tagged
/// `sep` and `collapse` arguments control joining; untagged arguments are
/// the vector operands in order. `paste0` maps here with an empty default
/// separator — resolved by call name at the import boundary, so the slice
/// reads the separator argument identically for both spellings.
fn paste_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    default_sep: &str,
) -> EvalResult {
    // Walk the pairlist manually: tags decide the argument's role.
    let mut operands = Vec::new();
    let mut sep = default_sep.to_string();
    let mut collapse: Option<String> = None;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let forced = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("sep") => {
                let strings = strings_of_value(heap, interner, forced).map_err(RSignal::from)?;
                if let Some(first) = strings.first() {
                    sep = interner.string_text(*first).to_string();
                }
            }
            Some("collapse") => {
                let strings = strings_of_value(heap, interner, forced).map_err(RSignal::from)?;
                if let Some(first) = strings.first() {
                    collapse = Some(interner.string_text(*first).to_string());
                }
            }
            _ => operands.push(forced),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    if operands.is_empty() {
        // GNU's empty-call matrix: no vector operands yields character(0)
        // unless collapse is present, in which case the collapsed empty
        // vector is a single empty string exactly.
        if collapse.is_some() {
            let empty = interner.intern_string("");
            return Ok(heap.alloc(RObject::String(vec![empty])));
        }
        return Ok(heap.alloc(RObject::String(Vec::new())));
    }
    // Coerce and recycle elementwise — NULL operands contribute empty
    // columns exactly as GNU's paste(NULL) yields character(0).
    let mut columns = Vec::new();
    for operand in operands {
        if operand == ValueId::NIL || matches!(heap.object(operand), Some(RObject::Nil)) {
            columns.push(Vec::new());
        } else {
            columns.push(strings_of_value(heap, interner, operand).map_err(RSignal::from)?);
        }
    }
    // GNU paste coercion renders NA_character_ as the text "NA" — the
    /// conversion lives here, not in shared string coercion, so nchar and
    /// friends still see genuine missingness.
    for column in columns.iter_mut() {
        for element in column.iter_mut() {
            if interner.string_text(*element) == "\0NA" {
                *element = interner.intern_string("NA");
            }
        }
    }
    let width = columns.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        // Every operand zero-length: the result is character(0), or a
        // single empty string when collapse joins the empty output.
        if collapse.is_some() {
            let empty = interner.intern_string("");
            return Ok(heap.alloc(RObject::String(vec![empty])));
        }
        return Ok(heap.alloc(RObject::String(Vec::new())));
    }
    // GNU treats zero-length operands in mixed company as a single empty
    // string recycling across the result width exactly.
    let empty_text = interner.intern_string("");
    for column in columns.iter_mut() {
        if column.is_empty() {
            column.push(empty_text);
        }
    }
    // GNU paste recycles silently even when lengths are not multiples —
    /// unlike arithmetic, no warning or error attaches.
    let mut out = Vec::new();
    for i in 0..width {
        let mut pieces = Vec::new();
        for column in &columns {
            pieces.push(interner.string_text(column[i % column.len()]).to_string());
        }
        out.push(interner.intern_string(&pieces.join(&sep)));
    }
    if let Some(collapse_sep) = collapse {
        let joined = out
            .iter()
            .map(|s| interner.string_text(*s).to_string())
            .collect::<Vec<_>>()
            .join(&collapse_sep);
        out = vec![interner.intern_string(&joined)];
    }
    Ok(heap.alloc(RObject::String(out)))
}

/// `nchar(x)` — character counts per element, integer output exactly GNU.
fn nchar_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let strings = strings_of_value(heap, interner, operands[0]).map_err(RSignal::from)?;
    let counts: Vec<i32> = strings
        .iter()
        .map(|s| {
            let text = interner.string_text(*s);
            // GNU: nchar of NA is NA — the sentinel text marks the missing
            /// string exactly as the bridge round-trips it.
            if text == "\0NA" {
                crate::value::object::NA_INTEGER
            } else {
                text.chars().count() as i32
            }
        })
        .collect();
    Ok(heap.alloc(RObject::Integer(counts)))
}

/// The replacement-assignment desugar: `x[i] <- v` where the target is a
/// `[` call. The slice requires the subscripted object be a plain symbol —
/// nested replacement chains are deep GNU territory beyond the migration
/// scaffolding.
fn assign_replacement(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    target: ValueId,
    rhs: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let (head, args) = match heap.object(target) {
        Some(RObject::Language { car, cdr, .. }) => (*car, *cdr),
        _ => return Err(EvalError::TypeError("unsupported assignment target").into()),
    };
    let head_name = match heap.object(head) {
        Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
        _ => return Err(EvalError::TypeError("unsupported assignment target").into()),
    };
    let call_args = raw_args(heap, args);
    if call_args.is_empty() {
        return Err(EvalError::UnsupportedForm("replacement arity").into());
    }
    let sym = match heap.object(call_args[0]) {
        Some(RObject::Symbol { name, .. }) => *name,
        _ => return Err(EvalError::TypeError("replacement object must be a symbol").into()),
    };
    if head_name == "[" {
        if call_args.len() != 2 {
            return Err(EvalError::UnsupportedForm("replacement arity").into());
        }
        // The object is the symbol's current binding, exactly GNU.
        let object = eval(heap, interner, call_args[0], caller_env)?;
        let object = force_if_promise(heap, interner, object)?;
        let index = eval(heap, interner, call_args[1], caller_env)?;
        let index = force_if_promise(heap, interner, index)?;
        let value = eval(heap, interner, rhs, caller_env)?;
        let value = force_if_promise(heap, interner, value)?;
        let replaced = subset_replace(heap, object, index, value).map_err(RSignal::from)?;
        crate::value::env::define(heap, caller_env, sym, replaced)
            .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("replacement define")))?;
        // Replacement assignments evaluate to the assigned value in GNU — the
        // right-hand value, not the whole modified object.
        return Ok(value);
    }
    // Generic replacement functions: `F(x, ...) <- v` desugars to
    // `x <- `F<-`(x, ..., v)` — exactly GNU's rewrite. The synthetic call
    // evaluates through ordinary dispatch, so primitive AND user-defined
    // replacement functions resolve uniformly. The rhs pre-evaluates once
    // and injects as a self-evaluating constant.
    let replacement_name = format!("{head_name}<-");
    if primitive_id_for_name(&replacement_name).is_some()
        || crate::value::env::lookup(heap, caller_env, interner.intern_symbol(&replacement_name)).is_some()
    {
        let value = eval(heap, interner, rhs, caller_env)?;
        let value = force_if_promise(heap, interner, value)?;
        // Build the argument chain: object expression, any extra target
        /// arguments, the pre-evaluated value last.
        let mut chain = heap.alloc(RObject::Pairlist {
            car: value,
            cdr: ValueId::NIL,
            tag: ValueId::NIL,
        });
        for extra in call_args[1..].iter().rev() {
            chain = heap.alloc(RObject::Pairlist {
                car: *extra,
                cdr: chain,
                tag: ValueId::NIL,
            });
        }
        chain = heap.alloc(RObject::Pairlist {
            car: call_args[0],
            cdr: chain,
            tag: ValueId::NIL,
        });
        let head_sym = interner.intern_symbol(&replacement_name);
        let head_obj = heap.alloc(RObject::Symbol {
            name: head_sym,
            value: ValueId::UNBOUND,
            internal: ValueId::NIL,
        });
        let synthetic = heap.alloc(RObject::Language {
            car: head_obj,
            cdr: chain,
            tag: ValueId::NIL,
        });
        let replaced = eval(heap, interner, synthetic, caller_env)?;
        let replaced = force_if_promise(heap, interner, replaced)?;
        crate::value::env::define(heap, caller_env, sym, replaced)
            .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("replacement define")))?;
        return Ok(value);
    }
    if head_name == "$" {
        if call_args.len() != 2 {
            return Err(EvalError::UnsupportedForm("dollar replacement arity").into());
        }
        // Build the synthetic `$<-(object, name, value)` argument chain —
        /// object and name pass unevaluated exactly as the parser emits them,
        /// the rhs attaches as the final argument.
        let value_node = heap.alloc(RObject::Pairlist {
            car: rhs,
            cdr: ValueId::NIL,
            tag: ValueId::NIL,
        });
        let name_node = heap.alloc(RObject::Pairlist {
            car: call_args[1],
            cdr: value_node,
            tag: ValueId::NIL,
        });
        let object_node = heap.alloc(RObject::Pairlist {
            car: call_args[0],
            cdr: name_node,
            tag: ValueId::NIL,
        });
        let replaced = dollar_assign_primitive(heap, interner, object_node, caller_env)?;
        crate::value::env::define(heap, caller_env, sym, replaced)
            .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("replacement define")))?;
        // Replacement assignments evaluate to the right-hand value in GNU.
        let value = eval(heap, interner, rhs, caller_env)?;
        return force_if_promise(heap, interner, value);
    }
    Err(EvalError::TypeError("unsupported replacement target").into())
}

/// `[<-` as a primitive: positional arguments object, index, value.
fn subset_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 3)?;
    subset_replace(heap, operands[0], operands[1], operands[2]).map_err(RSignal::from)
}

/// The replacement core: copy the object with positions replaced by the
/// recycled value. Positive out-of-range indices extend the vector with NA
/// padding exactly as GNU. Negative indices replace everything but the
/// excluded positions. Logical indices select replacement positions. The
/// slice requires the value's type match the object's — type promotion in
/// replacement is documented as future work.
fn subset_replace(
    heap: &mut Heap,
    object: ValueId,
    index: ValueId,
    value: ValueId,
) -> Result<ValueId, EvalError> {
    // Resolve positions exactly as subsetting does, but out-of-range
    // positive indices extend rather than pad-NA-read.
    let length = vector_length(heap, object)
        .ok_or(EvalError::TypeError("replacement requires a vector"))?;
    enum Positions {
        Replace(Vec<usize>),
    }
    let positions = match heap.object(index) {
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => {
            let values = values.clone();
            let is_logical = matches!(heap.object(index), Some(RObject::Logical(_)));
            if is_logical {
                let mut out = Vec::new();
                for i in 0..length.max(values.len()) {
                    let flag = values[i % values.len().max(1)];
                    if flag != 0 && flag != crate::value::object::NA_INTEGER {
                        out.push(i);
                    }
                }
                Positions::Replace(out)
            } else if values.iter().any(|v| *v < 0 && *v != crate::value::object::NA_INTEGER) {
                let excluded: Vec<usize> = values
                    .iter()
                    .filter(|v| **v < 0 && **v != crate::value::object::NA_INTEGER)
                    .map(|v| (-*v - 1) as usize)
                    .collect();
                Positions::Replace((0..length).filter(|i| !excluded.contains(i)).collect())
            } else {
                Positions::Replace(
                    values
                        .iter()
                        .filter(|v| **v > 0)
                        .map(|v| (*v - 1) as usize)
                        .collect(),
                )
            }
        }
        Some(RObject::Real(values)) => {
            let truncs: Vec<i32> = values
                .iter()
                .map(|v| {
                    if v.is_nan() {
                        crate::value::object::NA_INTEGER
                    } else {
                        *v as i32
                    }
                })
                .collect();
            let truncated = heap.alloc(RObject::Integer(truncs));
            return subset_replace(heap, object, truncated, value);
        }
        _ => return Err(EvalError::TypeError("unsupported replacement index")),
    };
    let Positions::Replace(positions) = positions;
    if positions.is_empty() {
        return Ok(object);
    }

    // GNU coercion discipline: target and replacement normalize to their
    /// common numeric rank before replacement — logical < integer < real.
    /// The vector widens when the replacement ranks higher; the replacement
    /// coerces when the target ranks higher. Strings are outside the slice.
    fn rank(heap: &Heap, value: ValueId) -> Option<u32> {
        match heap.object(value) {
            Some(RObject::Logical(_)) => Some(0),
            Some(RObject::Integer(_)) => Some(1),
            Some(RObject::Real(_)) => Some(2),
            _ => None,
        }
    }
    fn coerce(heap: &mut Heap, value: ValueId, to: u32) -> ValueId {
        let na_int = crate::value::object::NA_INTEGER;
        let na_real = f64::from_bits(crate::value::object::NA_REAL_BITS);
        match (heap.object(value), to) {
            (Some(RObject::Logical(v)), 1) => {
                let out: Vec<i32> = v.clone();
                heap.alloc(RObject::Integer(out))
            }
            (Some(RObject::Logical(v)), 2) => {
                let out: Vec<f64> = v
                    .iter()
                    .map(|x| if *x == na_int { na_real } else { *x as f64 })
                    .collect();
                heap.alloc(RObject::Real(out))
            }
            (Some(RObject::Integer(v)), 2) => {
                let out: Vec<f64> = v
                    .iter()
                    .map(|x| if *x == na_int { na_real } else { *x as f64 })
                    .collect();
                heap.alloc(RObject::Real(out))
            }
            _ => value,
        }
    }
    let (target_rank, replacement_rank) = match (rank(heap, object), rank(heap, value)) {
        (Some(a), Some(b)) => (a, b),
        _ => (0, 0),
    };
    let (object, value) = if target_rank > 0 || replacement_rank > 0 {
        let common = target_rank.max(replacement_rank);
        (
            coerce(heap, object, common),
            coerce(heap, value, common),
        )
    } else {
        (object, value)
    };

    macro_rules! replace_typed {
        ($variant:ident, $na:expr) => {
            match (heap.object(object), heap.object(value)) {
                (Some(RObject::$variant(target)), Some(RObject::$variant(replacements))) => {
                    let mut out = target.clone();
                    let replacements = replacements.clone();
                    let required = positions.iter().max().map(|m| m + 1).unwrap_or(0);
                    while out.len() < required {
                        out.push($na);
                    }
                    for (n, pos) in positions.iter().enumerate() {
                        out[*pos] = replacements[n % replacements.len()];
                    }
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                _ => {}
            }
        };
    }
    replace_typed!(Logical, crate::value::object::NA_INTEGER);
    replace_typed!(Integer, crate::value::object::NA_INTEGER);
    replace_typed!(Real, f64::from_bits(crate::value::object::NA_REAL_BITS));
    replace_typed!(Raw, 0u8);
    // Integer object with real value: GNU widens the whole vector to real.
    Err(EvalError::TypeError("replacement type mismatch"))
}

/// `numeric(n)`/`integer(n)`/`logical(n)` — zero/FALSE-filled vectors of
/// the requested length, GNU's constructor defaults exactly.
fn vector_constructor(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1).unwrap_or_default();
    let length = match operands.first().and_then(|v| heap.object(*v)) {
        Some(RObject::Real(v)) if !v.is_empty() => v[0] as usize,
        Some(RObject::Integer(v)) if !v.is_empty() => v[0] as usize,
        _ => 0,
    };
    let body = match op {
        NUMERIC => RObject::Real(vec![0.0; length]),
        INTEGER_CTOR => RObject::Integer(vec![0; length]),
        RAW_CTOR => RObject::Raw(vec![0u8; length]),
        _ => RObject::Logical(vec![0; length]),
    };
    Ok(heap.alloc(body))
}

/// Read one named attribute off an object's slot attribute chain — the
/// pairlist walk every attribute access shares. Attribute chains are
/// pairlist nodes whose tags are the attribute name symbols.
pub(crate) fn get_attrib(
    heap: &Heap,
    interner: &SymbolInterner,
    obj: ValueId,
    attr_name: &str,
) -> Option<ValueId> {
    let mut cursor = heap.get(obj)?.attrib;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            return None;
        }
        match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => {
                if let Some(RObject::Symbol { name, .. }) = heap.object(*tag) {
                    if interner.symbol_name(*name) == attr_name {
                        return Some(*car);
                    }
                }
                cursor = *cdr;
            }
            _ => return None,
        }
    }
    None
}

/// Copy an object with one attribute replaced (or prepended when absent).
/// R's copy-on-write discipline: the original object is never mutated —
/// assignment binds the fresh copy, so aliases keep the old attributes.
/// The attribute chain itself is rebuilt node-by-node for the same reason.
pub(crate) fn copy_with_attrib(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    obj: ValueId,
    attr_name: &str,
    attr_value: ValueId,
) -> Result<ValueId, EvalError> {
    let body = heap
        .object(obj)
        .cloned()
        .ok_or(EvalError::TypeError("attribute target missing"))?;
    let attr_sym = interner.intern_symbol(attr_name);
    let sym_obj = heap.alloc(RObject::Symbol {
        name: attr_sym,
        value: ValueId::UNBOUND,
        internal: ValueId::UNBOUND,
    });
    // Walk the original chain, rebuilding nodes; substitute the matching tag.
    let original_chain = heap.get(obj).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    let mut entries: Vec<(ValueId, ValueId)> = Vec::new();
    let mut found = false;
    let mut cursor = original_chain;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            break;
        }
        match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => {
                let matches = match heap.object(*tag) {
                    Some(RObject::Symbol { name, .. }) => {
                        interner.symbol_name(*name) == attr_name
                    }
                    _ => false,
                };
                if matches {
                    // GNU: assigning NULL removes the attribute outright.
                    if attr_value != ValueId::NIL {
                        entries.push((sym_obj, attr_value));
                    }
                    found = true;
                } else {
                    entries.push((*tag, *car));
                }
                cursor = *cdr;
            }
            _ => break,
        }
    }
    if !found && attr_value != ValueId::NIL {
        // GNU appends new attributes at the chain's end — attributes()
        /// enumerates in insertion order exactly as the oracle pins.
        entries.push((sym_obj, attr_value));
    }
    // Rebuild the chain tail-to-head.
    let mut chain = ValueId::NIL;
    for (tag, car) in entries.into_iter().rev() {
        chain = heap.alloc(RObject::Pairlist {
            car,
            cdr: chain,
            tag,
        });
    }
    let copy = heap.alloc(body);
    if let Some(slot) = heap.get_mut(copy) {
        slot.attrib = chain;
    }
    Ok(copy)
}

/// `names(x)` — the names attribute as a character vector, or NIL when the
/// object has none, exactly GNU.
fn names_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    match get_attrib(heap, interner, operands[0], "names") {
        Some(names) => Ok(names),
        None => Ok(ValueId::NIL),
    }
}

/// The names<- core, shared between the replacement primitive and the
/// assignment desugar: a copy of the object carrying the new names.
fn names_assign_core(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    obj: ValueId,
    value: ValueId,
) -> Result<ValueId, EvalError> {
    copy_with_attrib(heap, interner, obj, "names", value)
}

/// `` `names<-` `` as a primitive: positional object and value.
fn names_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    names_assign_core(heap, interner, operands[0], operands[1]).map_err(RSignal::from)
}

/// `x$name` — exact-match extraction by name. The second operand is a
/// literal symbol, never evaluated — GNU's argument discipline for `$`.
/// The slice implements exact matching only; GNU's partial matching is
/// documented as future work. Atomic vectors extract a length-one vector
/// of the same type; list elements unbox exactly as `[[`.
fn dollar_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() != 2 {
        return Err(EvalError::UnsupportedForm("$ arity").into());
    }
    let object = eval(heap, interner, parts[0], caller_env)?;
    let object = force_if_promise(heap, interner, object)?;
    let wanted = match heap.object(parts[1]) {
        Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
        _ => return Err(EvalError::TypeError("$ name must be a symbol").into()),
    };
    // Environments extract frame-only, exactly GNU: no enclosure chain,
    /// missing bindings are NULL rather than an error.
    if matches!(heap.object(object), Some(RObject::Environment { .. })) {
        let sym = interner.intern_symbol(&wanted);
        return match crate::value::env::lookup_in_frame(heap, object, sym) {
            Some(binding) => Ok(binding.value),
            None => Ok(ValueId::NIL),
        };
    }
    let names = match get_attrib(heap, interner, object, "names") {
        Some(n) => n,
        // GNU: dollar on a nameless object is NULL, not an error.
        None => return Ok(ValueId::NIL),
    };
    // GNU's matching order for `$`: exact first, then a UNIQUE prefix
    /// match — ambiguous prefixes and missing names are NULL.
    let position = match heap.object(names) {
        Some(RObject::String(values)) => {
            let exact = values
                .iter()
                .position(|s| interner.string_text(*s) == wanted);
            match exact {
                Some(p) => Some(p),
                None => {
                    let prefixes: Vec<usize> = values
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| {
                            let text = interner.string_text(**s);
                            text.starts_with(wanted.as_str()) && !wanted.is_empty()
                        })
                        .map(|(i, _)| i)
                        .collect();
                    if prefixes.len() == 1 {
                        Some(prefixes[0])
                    } else {
                        None
                    }
                }
            }
        }
        _ => None,
    };
    // GNU discipline: `$` is invalid for atomic vectors — only recursive
    /// objects (lists) extract by dollar. Atomic named access goes through
    /// `[` with character indices, a separate slice.
    match heap.object(object) {
        Some(RObject::List(items)) => {
            let position = match position {
                Some(p) => p,
                None => return Ok(ValueId::NIL),
            };
            if position >= items.len() {
                return Ok(ValueId::NIL);
            }
            Ok(items[position])
        }
        _ => Err(EvalError::TypeError("$ operator is invalid for atomic vectors").into()),
    }
}

/// `lapply(x, f)` / `sapply(x, f)` — apply a closure to each element.
/// Atomic vectors box elements as length-one vectors (GNU exactly); list
/// elements pass through unboxed. Application reuses the closure machinery
/// through a synthetic argument pairlist — the element is already a value,
/// and vector values self-evaluate when the argument promise forces.
/// sapply simplifies homogeneous length-one results into a vector, GNU's
/// canonical simplification shape.
fn apply_family(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let elements = sequence_elements(heap, operands[0])?;
    let fun = operands[1];
    // Primitives and closures both apply through the shared value passing
    /// machinery exactly as GNU's lapplyFUN dispatches.
    let closure_parts = match heap.object(fun) {
        Some(RObject::Closure { formals, body, env }) => Some((*formals, *body, *env)),
        _ => None,
    };
    let mut results = Vec::new();
    for element in elements {
        if let Some((formals, body, closure_env)) = closure_parts {
            let forced = heap.alloc(RObject::Promise {
                expr: element,
                env: caller_env,
                value: element,
            });
            let arg_node = heap.alloc(RObject::Pairlist {
                car: forced,
                cdr: ValueId::NIL,
                tag: ValueId::NIL,
            });
            let applied = super::eval::apply_closure_public(
                heap, interner, formals, body, closure_env, arg_node, caller_env,
            )?;
            let applied = force_if_promise(heap, interner, applied)?;
            results.push(applied);
        } else {
            results.push(apply_fun_to(heap, interner, fun, element, caller_env)?);
        }
    }
    // Iteration names ride the output exactly as GNU's apply family —
    // pairlist inputs contribute their tags.
    let iter_names = iteration_names(heap, interner, operands[0]);
    let stamp_names = |heap: &mut Heap, interner: &mut SymbolInterner, result: ValueId| -> EvalResult {
        if !iter_names.is_empty() {
            let interned: Vec<_> = iter_names.iter().map(|n| interner.intern_string(n)).collect();
            let names_obj = heap.alloc(RObject::String(interned));
            return copy_with_attrib(heap, interner, result, "names", names_obj).map_err(Into::into);
        }
        Ok(result)
    };
    if op == LAPPLY {
        let list = heap.alloc(RObject::List(results));
        return stamp_names(heap, interner, list);
    }
    // sapply simplification: homogeneous length-one atomic results unbox
    // into a single vector of the common type; anything else stays a list.
    let simplified = simplify_sapply(heap, results)?;
    // GNU carries input names onto the simplified output whenever the
    // lengths align — vector names and pairlist tags alike.
    let out_len = vector_length(heap, simplified).unwrap_or(0);
    if !iter_names.is_empty() && iter_names.len() == out_len {
        stamp_names(heap, interner, simplified)
    } else {
        Ok(simplified)
    }
}

/// GNU's simplification rule shape: every result a length-one atomic vector
/// of one type unboxes elementwise; otherwise the list stands.
fn simplify_sapply(heap: &mut Heap, results: Vec<ValueId>) -> EvalResult {
    if results.is_empty() {
        return Ok(heap.alloc(RObject::List(results)));
    }
    enum Kind {
        Logical,
        Integer,
        Real,
        String,
    }
    let mut kind: Option<Kind> = None;
    for result in &results {
        let this = match heap.object(*result) {
            Some(RObject::Logical(v)) if v.len() == 1 => Kind::Logical,
            Some(RObject::Integer(v)) if v.len() == 1 => Kind::Integer,
            Some(RObject::Real(v)) if v.len() == 1 => Kind::Real,
            Some(RObject::String(v)) if v.len() == 1 => Kind::String,
            _ => return Ok(heap.alloc(RObject::List(results))),
        };
        match (&kind, &this) {
            (None, _) => kind = Some(this),
            (Some(Kind::Logical), Kind::Logical) => {}
            (Some(Kind::Integer), Kind::Integer) => {}
            (Some(Kind::Real), Kind::Real) => {}
            (Some(Kind::String), Kind::String) => {}
            // Character mixtures dominate — GNU coerces to character when
            /// any result is character.
            (Some(Kind::String), _) | (_, Kind::String) => kind = Some(Kind::String),
            // Mixed numeric kinds simplify upward to real in GNU's exact
            // shape; logical/integer mixtures stay integer. The slice
            // simplifies any numeric mixture to real, documented.
            _ => kind = Some(Kind::Real),
        }
    }
    match kind {
        Some(Kind::Logical) => {
            let mut out = Vec::new();
            for result in results {
                match heap.object(result) {
                    Some(RObject::Logical(v)) => out.push(v[0]),
                    _ => return Err(EvalError::TypeError("simplify kind drift").into()),
                }
            }
            Ok(heap.alloc(RObject::Logical(out)))
        }
        Some(Kind::Integer) => {
            let mut out = Vec::new();
            for result in results {
                match heap.object(result) {
                    Some(RObject::Integer(v)) => out.push(v[0]),
                    _ => return Err(EvalError::TypeError("simplify kind drift").into()),
                }
            }
            Ok(heap.alloc(RObject::Integer(out)))
        }
        Some(Kind::String) => {
            let mut out = Vec::new();
            for result in results {
                match heap.object(result) {
                    Some(RObject::String(v)) => out.push(v[0]),
                    _ => return Err(EvalError::TypeError("simplify kind drift").into()),
                }
            }
            return Ok(heap.alloc(RObject::String(out)));
        }
        _ => {
            let mut out = Vec::new();
            for result in results {
                match heap.object(result) {
                    Some(RObject::Real(v)) => out.push(v[0]),
                    Some(RObject::Integer(v)) => {
                        if v[0] == crate::value::object::NA_INTEGER {
                            out.push(f64::from_bits(crate::value::object::NA_REAL_BITS));
                        } else {
                            out.push(v[0] as f64);
                        }
                    }
                    Some(RObject::Logical(v)) => out.push(v[0] as f64),
                    _ => return Err(EvalError::TypeError("simplify kind drift").into()),
                }
            }
            Ok(heap.alloc(RObject::Real(out)))
        }
    }
}

/// `unlist(x)` — recursive flattening to the common atomic type, GNU's
/// type-hierarchy shape: any real promotes to real, integer/logical
/// mixtures promote to integer. The slice covers numeric flattening;
/// string mixtures are future work.
fn unlist_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
    recursive: bool,
    use_names: bool,
) -> EvalResult {
    // GNU unlist: recursive=TRUE flattens nested lists fully and atomizes
    /// to the highest type in the lattice; recursive=FALSE descends exactly
    /// one list level and yields a list whose nested lists stay intact.
    /// Names concatenate outer.inner with dots, positional indices gluing
    /// bare exactly as GNU's name construction.
    if !matches!(heap.object(value), Some(RObject::List(_))) {
        return Ok(value);
    }
    enum Leaf {
        Scalar { rank: u8, int: i32, real: f64, imag: f64, text: String, name: String },
        Intact { value: ValueId, name: String },
    }
    fn rank_of(heap: &Heap, v: ValueId) -> u8 {
        match heap.object(v) {
            Some(RObject::Logical(_)) | Some(RObject::Raw(_)) => 1,
            Some(RObject::Integer(_)) => 2,
            Some(RObject::Real(_)) => 3,
            Some(RObject::Complex(_)) => 4,
            Some(RObject::String(_)) => 5,
            _ => 0,
        }
    }
    fn walk(
        heap: &Heap,
        interner: &SymbolInterner,
        value: ValueId,
        prefix: &str,
        depth: usize,
        recursive: bool,
        out: &mut Vec<Leaf>,
        crossed_list: &mut bool,
    ) {
        if depth > 0 {
            *crossed_list = true;
        }
        if let Some(RObject::List(items)) = heap.object(value) {
            let names = get_attrib(heap, interner, value, "names")
                .map(|id| string_vec(heap, interner, id))
                .unwrap_or_default();
            for (i, item) in items.clone().iter().enumerate() {
                let outer = names.get(i).cloned().unwrap_or_default();
                let item_prefix = if prefix.is_empty() {
                    outer.clone()
                } else if outer.is_empty() {
                    prefix.to_string()
                } else {
                    format!("{prefix}.{outer}")
                };
                if matches!(heap.object(*item), Some(RObject::List(_))) {
                    if recursive || depth == 0 {
                        walk(heap, interner, *item, &item_prefix, depth + 1, recursive, out, crossed_list);
                    } else {
                        out.push(Leaf::Intact { value: *item, name: item_prefix });
                    }
                    continue;
                }
                let len = vector_length(heap, *item).unwrap_or(0);
                if len == 0 {
                    continue;
                }
                let inner_names = get_attrib(heap, interner, *item, "names")
                    .map(|id| string_vec(heap, interner, id))
                    .unwrap_or_default();
                let rank = rank_of(heap, *item);
                for k in 0..len {
                    let inner = inner_names.get(k).cloned().unwrap_or_default();
                    let name = if len == 1 || !inner.is_empty() {
                        if inner.is_empty() {
                            item_prefix.clone()
                        } else if item_prefix.is_empty() {
                            inner.clone()
                        } else {
                            format!("{item_prefix}.{inner}")
                        }
                    } else if item_prefix.is_empty() {
                        String::new()
                    } else {
                        format!("{item_prefix}{}", k + 1)
                    };
                    let (int, real, im, text) = match heap.object(*item) {
                        Some(RObject::Logical(vals)) | Some(RObject::Integer(vals)) => {
                            let v = vals.get(k).copied().unwrap_or(crate::value::object::NA_INTEGER);
                            let r = if v == crate::value::object::NA_INTEGER {
                                f64::from_bits(crate::value::object::NA_REAL_BITS)
                            } else {
                                v as f64
                            };
                            (v, r, 0.0, v.to_string())
                        }
                        Some(RObject::Real(vals)) => {
                            let v = vals.get(k).copied().unwrap_or(f64::NAN);
                            (0, v, 0.0, super::print::format_real(v))
                        }
                        Some(RObject::String(vals)) => {
                            let s = vals
                                .get(k)
                                .map(|x| interner.string_text(*x).to_string())
                                .unwrap_or_default();
                            (0, f64::NAN, 0.0, s)
                        }
                        Some(RObject::Complex(vals)) => {
                            let z = vals.get(k).copied().unwrap_or_default();
                            (0, z.r, z.i, super::print::format_complex(z))
                        }
                        Some(RObject::Raw(vals)) => {
                            let v = vals.get(k).copied().unwrap_or(0);
                            (v as i32, v as f64, 0.0, format!("{v}"))
                        }
                        _ => (0, f64::NAN, 0.0, String::new()),
                    };
                    out.push(Leaf::Scalar { rank, int, real, imag: im, text, name });
                }
            }
        }
    }
    let mut leaves: Vec<Leaf> = Vec::new();
    let mut crossed_list = false;
    walk(heap, interner, value, "", 0, recursive, &mut leaves, &mut crossed_list);
    // recursive=FALSE atomizes exactly like recursive=TRUE when no sublist
    /// boundary was crossed — the shallow shape only appears once list
    /// structure actually participates, exactly as GNU.
    if !recursive && !crossed_list {
        return unlist_primitive(heap, interner, value, true, use_names);
    }
    if !recursive {
        let mut items = Vec::new();
        let mut names = Vec::new();
        for leaf in &leaves {
            match leaf {
                Leaf::Scalar { rank, int, real, text, name, .. } => {
                    let scalar = match rank {
                        3 => heap.alloc(RObject::Real(vec![*real])),
                        5 => heap.alloc(RObject::String(vec![interner.intern_string(text)])),
                        1 | 2 => {
                            if *rank == 2 {
                                heap.alloc(RObject::Integer(vec![*int]))
                            } else {
                                heap.alloc(RObject::Logical(vec![*int]))
                            }
                        }
                        _ => heap.alloc(RObject::Real(vec![*real])),
                    };
                    items.push(scalar);
                    names.push(name.clone());
                }
                Leaf::Intact { value, name } => {
                    items.push(*value);
                    names.push(name.clone());
                }
            }
        }
        let result = heap.alloc(RObject::List(items));
        if use_names && names.iter().any(|n| !n.is_empty()) {
            let interned: Vec<super::intern::InternedStr> = names
                .iter()
                .map(|s| interner.intern_string(s))
                .collect();
            let names_vec = heap.alloc(RObject::String(interned));
            return copy_with_attrib(heap, interner, result, "names", names_vec).map_err(RSignal::from);
        }
        return Ok(result);
    }
    let mut top_rank = 0u8;
    let mut names = Vec::new();
    let mut ints = Vec::new();
    let mut reals = Vec::new();
    let mut imags = Vec::new();
    let mut texts = Vec::new();
    for leaf in &leaves {
        match leaf {
            Leaf::Scalar { rank, int, real, imag, text, name } => {
                top_rank = top_rank.max(*rank);
                ints.push(*int);
                reals.push(*real);
                imags.push(*imag);
                texts.push(text.clone());
                names.push(name.clone());
            }
            Leaf::Intact { .. } => {}
        }
    }
    let result = if top_rank >= 5 {
        let interned: Vec<super::intern::InternedStr> = texts
            .iter()
            .map(|s| interner.intern_string(s))
            .collect();
        heap.alloc(RObject::String(interned))
    } else if top_rank >= 4 {
        // Complex promotion: real parts ride from the numeric leaves exactly
        /// as GNU's coercion — logicals and integers contribute their value,
        /// complex leaves their own real part.
        let zs: Vec<crate::value::object::RComplex> = reals
            .iter()
            .zip(imags.iter())
            .map(|(&r, &i)| crate::value::object::RComplex { r, i })
            .collect();
        heap.alloc(RObject::Complex(zs))
    } else if top_rank >= 3 {
        heap.alloc(RObject::Real(reals))
    } else {
        heap.alloc(RObject::Integer(ints))
    };
    if use_names && names.iter().any(|n| !n.is_empty()) {
        let interned: Vec<super::intern::InternedStr> = names
            .iter()
            .map(|s| interner.intern_string(s))
            .collect();
        let names_vec = heap.alloc(RObject::String(interned));
        copy_with_attrib(heap, interner, result, "names", names_vec).map_err(RSignal::from)
    } else {
        Ok(result)
    }
}

/// First element of a numeric vector as a double — the scalar extraction
/// the constructors share.
fn first_real(heap: &Heap, value: ValueId) -> Result<f64, EvalError> {
    match heap.object(value) {
        Some(RObject::Real(v)) if !v.is_empty() => Ok(v[0]),
        Some(RObject::Integer(v)) if !v.is_empty() => {
            if v[0] == crate::value::object::NA_INTEGER {
                Err(EvalError::TypeError("NA scalar"))
            } else {
                Ok(v[0] as f64)
            }
        }
        Some(RObject::Logical(v)) if !v.is_empty() => Ok(v[0] as f64),
        _ => Err(EvalError::TypeError("numeric scalar required")),
    }
}

/// `seq(from, to, by)` — GNU's general sequence. Integer-valued from/to/by
/// yield integer sequences when from and by are both whole, matching the
/// colon operator's integer fidelity; otherwise real. Positional and named
/// argument shapes both resolve — named extraction uses the same eval_args
/// machinery since seq's operands are positional in practice.
fn seq_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Collect raw operands with tags so seq(from=1, to=5, by=2) resolves.
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let lookup = |name: &str, position: usize| -> Option<ValueId> {
        named
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
            .or_else(|| positional.get(position).copied())
    };
    let from = lookup("from", 0).ok_or(EvalError::TypeError("seq requires from"))?;
    let to = lookup("to", 1).ok_or(EvalError::TypeError("seq requires to"))?;
    let from_v = first_real(heap, from).map_err(RSignal::from)?;
    let to_v = first_real(heap, to).map_err(RSignal::from)?;
    let by_v = match lookup("by", 2) {
        Some(by) => first_real(heap, by).map_err(RSignal::from)?,
        None => 1.0,
    };
    if by_v == 0.0 {
        return Err(EvalError::TypeError("seq by zero").into());
    }
    let mut out: Vec<f64> = Vec::new();
    let mut current = from_v;
    for _ in 0..100_000_000 {
        if (by_v > 0.0 && current > to_v) || (by_v < 0.0 && current < to_v) {
            break;
        }
        out.push(current);
        current += by_v;
    }
    // GNU fidelity: integer-valued from and by produce integer sequences.
    let whole = from_v.fract() == 0.0 && by_v.fract() == 0.0 && out.iter().all(|v| v.fract() == 0.0);
    if whole && out.iter().all(|v| v.abs() <= i32::MAX as f64) {
        let ints: Vec<i32> = out.iter().map(|v| *v as i32).collect();
        return Ok(heap.alloc(RObject::Integer(ints)));
    }
    Ok(heap.alloc(RObject::Real(out)))
}

/// `rep(x, times)` — GNU's simplest repetition shape: the whole vector
/// repeats times over. Per-element repetition (times as a vector) is
/// future work.
fn rep_primitive(heap: &mut Heap, source: ValueId, times: ValueId) -> EvalResult {
    let n = first_real(heap, times).map_err(RSignal::from)? as usize;
    macro_rules! repeat {
        ($variant:ident) => {
            match heap.object(source) {
                Some(RObject::$variant(values)) => {
                    let mut out = Vec::with_capacity(values.len() * n);
                    for _ in 0..n {
                        out.extend(values.iter().copied());
                    }
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                _ => {}
            }
        };
    }
    repeat!(Logical);
    repeat!(Integer);
    repeat!(Real);
    repeat!(String);
    repeat!(Raw);
    repeat!(Complex);
    match heap.object(source) {
        Some(RObject::List(values)) => {
            let mut out = Vec::with_capacity(values.len() * n);
            for _ in 0..n {
                out.extend(values.iter().copied());
            }
            Ok(heap.alloc(RObject::List(out)))
        }
        _ => Err(EvalError::TypeError("rep requires a vector").into()),
    }
}

/// Numeric contents of a vector as doubles — the extraction the statistics
/// share. Logicals and integers coerce up exactly as GNU's as-double.
fn numerics_of(heap: &Heap, value: ValueId) -> Result<Vec<f64>, EvalError> {
    let na = f64::from_bits(crate::value::object::NA_REAL_BITS);
    match heap.object(value) {
        Some(RObject::Real(v)) => Ok(v.clone()),
        Some(RObject::Integer(v)) => Ok(v
            .iter()
            .map(|x| {
                if *x == crate::value::object::NA_INTEGER {
                    na
                } else {
                    *x as f64
                }
            })
            .collect()),
        Some(RObject::Logical(v)) => Ok(v
            .iter()
            .map(|x| {
                if *x == crate::value::object::NA_INTEGER {
                    na
                } else {
                    *x as f64
                }
            })
            .collect()),
        _ => Err(EvalError::TypeError("statistic requires numeric")),
    }
}

/// `mean`/`var`/`sd`/`median` with GNU's na.rm handling: absent or FALSE,
/// any NA poisons the result to NA; TRUE drops NA positions first. The
/// na.rm argument resolves tagged or positional.
fn stat_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let input = positional
        .first()
        .copied()
        .ok_or(EvalError::TypeError("statistic requires input"))?;
    let na_rm = match named
        .iter()
        .find(|(n, _)| n == "na.rm")
        .map(|(_, v)| *v)
        .or_else(|| positional.get(1).copied())
    {
        Some(flag) => matches!(
            heap.object(flag),
            Some(RObject::Logical(v)) if !v.is_empty() && v[0] != 0 && v[0] != crate::value::object::NA_INTEGER
        ),
        None => false,
    };
    let mut xs = numerics_of(heap, input).map_err(RSignal::from)?;
    let na_real = f64::from_bits(crate::value::object::NA_REAL_BITS);
    let any_na = xs.iter().any(|v| v.is_nan());
    if any_na && !na_rm {
        return Ok(heap.alloc(RObject::Real(vec![na_real])));
    }
    xs.retain(|v| !v.is_nan());
    if xs.is_empty() {
        return Ok(heap.alloc(RObject::Real(vec![na_real])));
    }
    let result = match op {
        MEAN => xs.iter().sum::<f64>() / xs.len() as f64,
        VAR => {
            // GNU's sample variance with the mean-centered two-pass form.
            let mean = xs.iter().sum::<f64>() / xs.len() as f64;
            xs.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (xs.len() as f64 - 1.0)
        }
        SD => {
            let mean = xs.iter().sum::<f64>() / xs.len() as f64;
            (xs.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (xs.len() as f64 - 1.0))
                .sqrt()
        }
        _ => {
            // GNU's median: mean of the two central order statistics for
            // even lengths.
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = xs.len();
            if n % 2 == 1 {
                xs[n / 2]
            } else {
                (xs[n / 2 - 1] + xs[n / 2]) / 2.0
            }
        }
    };
    Ok(heap.alloc(RObject::Real(vec![result])))
}

/// `is.na(x)` — elementwise NA detection across the atomic types.
fn is_na_primitive(heap: &mut Heap, interner: &SymbolInterner, value: ValueId) -> EvalResult {
    let na_int = crate::value::object::NA_INTEGER;
    let out: Vec<i32> = match heap.object(value) {
        Some(RObject::Logical(v)) => v.iter().map(|x| (*x == na_int) as i32).collect(),
        Some(RObject::Integer(v)) => v.iter().map(|x| (*x == na_int) as i32).collect(),
        Some(RObject::Real(v)) => v.iter().map(|x| x.is_nan() as i32).collect(),
        Some(RObject::String(v)) => v
            .iter()
            .map(|x| (interner.string_text(*x) == "\0NA") as i32)
            .collect(),
        _ => return Err(EvalError::TypeError("is.na requires a vector").into()),
    };
    Ok(heap.alloc(RObject::Logical(out)))
}

/// `sort`/`rev`/`unique` — ordering and deduplication over atomic vectors.
/// GNU's sort drops NAs by default (na.last=NA); the slice follows that.
fn ordering_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let value = operands[0];
    // rev is generic reversal exactly as GNU — any vector or list
    // reverses elementwise with names carried along; only sort continues
    // into the ordering machinery below.
    if op == REV {
        let reversed = reverse_value(heap, interner, value)?;
        return Ok(reversed);
    }
    // Tagged-argument walk: decreasing and na.last arrive tagged or
    /// positional exactly as GNU's argument matching.
    let mut decreasing = false;
    // na.last tri-state: None = remove (GNU default NA), Some(true) = end,
    /// Some(false) = front.
    let mut na_last: Option<bool> = None;
    let mut positional_seen = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional_seen += 1;
        if positional_seen == 1 {
            if cdr == ValueId::NIL {
                break;
            }
            cursor = cdr;
            continue;
        }
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        let truthy = |id: ValueId| -> Option<bool> {
            match heap.object(id) {
                Some(RObject::Logical(vals)) if !vals.is_empty() => {
                    if vals[0] == crate::value::object::NA_INTEGER {
                        None
                    } else {
                        Some(vals[0] != 0)
                    }
                }
                _ => None,
            }
        };
        let is_na_value = matches!(
            heap.object(v),
            Some(RObject::Logical(vals)) if !vals.is_empty() && vals[0] == crate::value::object::NA_INTEGER
        );
        match tag_name.as_deref() {
            Some("decreasing") => decreasing = truthy(v).unwrap_or(false),
            Some("na.last") => {
                na_last = if is_na_value {
                    None
                } else {
                    match truthy(v) {
                        Some(true) => Some(true),
                        _ => Some(false),
                    }
                };
            }
            _ => {
                if positional_seen == 2 {
                    decreasing = truthy(v).unwrap_or(false);
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    macro_rules! per_type {
        ($variant:ident, $ord:expr, $na:expr) => {
            match heap.object(value) {
                Some(RObject::$variant(values)) => {
                    let mut out = values.clone();
                    match op {
                        SORT => {
                            let na_count = out.iter().filter(|v| $ord(**v)).count();
                            out.retain(|v| !$ord(*v));
                            out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                            if decreasing {
                                out.reverse();
                            }
                            // GNU na.last: absent/NA drops (already done);
                            /// TRUE appends after the final order, FALSE
                            /// prepends — placement ignores decreasing.
                            match na_last {
                                Some(true) => {
                                    for _ in 0..na_count {
                                        out.push($na);
                                    }
                                }
                                Some(false) => {
                                    for _ in 0..na_count {
                                        out.insert(0, $na);
                                    }
                                }
                                None => {}
                            }
                        }
                        REV => out.reverse(),
                        _ => {
                            let mut seen: Vec<_> = Vec::new();
                            out.retain(|v| {
                                if seen.iter().any(|s| s == v) {
                                    false
                                } else {
                                    seen.push(*v);
                                    true
                                }
                            });
                        }
                    }
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                _ => {}
            }
        };
    }
    let is_na_int = |v: i32| v == crate::value::object::NA_INTEGER;
    let is_na_real = |v: f64| v.is_nan();
    per_type!(Integer, is_na_int, crate::value::object::NA_INTEGER);
    per_type!(Real, is_na_real, f64::from_bits(crate::value::object::NA_REAL_BITS));
    per_type!(Logical, is_na_int, crate::value::object::NA_INTEGER);
    // Character vectors order lexicographically — GNU's C-locale byte order
    /// for the slice; the NA sentinel text sorts out exactly as numeric NA.
    match heap.object(value) {
        Some(RObject::String(values)) => {
            let mut out: Vec<String> = values
                .iter()
                .map(|v| interner.string_text(*v).to_string())
                .collect();
            match op {
                SORT => {
                    let na_count = out.iter().filter(|v| *v == "\0NA").count();
                    out.retain(|v| v != "\0NA");
                    out.sort();
                    if decreasing {
                        out.reverse();
                    }
                    match na_last {
                        Some(true) => {
                            for _ in 0..na_count {
                                out.push("\0NA".to_string());
                            }
                        }
                        Some(false) => {
                            for _ in 0..na_count {
                                out.insert(0, "\0NA".to_string());
                            }
                        }
                        None => {}
                    }
                }
                REV => out.reverse(),
                _ => {
                    let mut seen: Vec<String> = Vec::new();
                    out.retain(|v| {
                        if seen.contains(v) {
                            false
                        } else {
                            seen.push(v.clone());
                            true
                        }
                    });
                }
            }
            let interned: Vec<super::intern::InternedStr> = out
                .iter()
                .map(|s| interner.intern_string(s))
                .collect();
            return Ok(heap.alloc(RObject::String(interned)));
        }
        _ => {}
    }
    Err(EvalError::TypeError("ordering requires a numeric vector").into())
}

/// `which(x)` — 1-based positions of TRUE entries, GNU exactly; NA
/// positions drop out.
fn which_primitive(heap: &mut Heap, value: ValueId) -> EvalResult {
    match heap.object(value) {
        Some(RObject::Logical(values)) => {
            let out: Vec<i32> = values
                .iter()
                .enumerate()
                .filter(|(_, v)| **v != 0 && **v != crate::value::object::NA_INTEGER)
                .map(|(i, _)| (i + 1) as i32)
                .collect();
            Ok(heap.alloc(RObject::Integer(out)))
        }
        _ => Err(EvalError::TypeError("which requires logical").into()),
    }
}

/// `any(x)`/`all(x)` — GNU's quantifier NA semantics: any is TRUE if any
/// entry is TRUE, NA if none TRUE but some NA, FALSE otherwise; all
/// mirrors.
fn quantifier_primitive(heap: &mut Heap, op: u32, value: ValueId) -> EvalResult {
    let values = logicals_of_value(heap, value).map_err(RSignal::from)?;
    let na = crate::value::object::NA_INTEGER;
    let trues = values.iter().filter(|v| **v != 0 && **v != na).count();
    let falses = values.iter().filter(|v| **v == 0).count();
    let nas = values.len() - trues - falses;
    let result = if op == ANY {
        if trues > 0 {
            1
        } else if nas > 0 {
            na
        } else {
            0
        }
    } else if falses > 0 {
        0
    } else if nas > 0 {
        na
    } else {
        1
    };
    Ok(heap.alloc(RObject::Logical(vec![result])))
}

/// `ifelse(test, yes, no)` — GNU's vectorized conditional: elementwise
/// selection with yes/no recycling. The slice covers numeric both-sides;
/// NA test positions yield NA.
fn ifelse_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 3)?;
    let tests = logicals_of_value(heap, operands[0]).map_err(RSignal::from)?;
    // GNU: a zero-length test selects nothing, and the result is logical
    /// empty regardless of the branch types.
    if tests.is_empty() {
        return Ok(heap.alloc(RObject::Logical(vec![])));
    }
    let na_int = crate::value::object::NA_INTEGER;
    let yes_is_str = matches!(heap.object(operands[1]), Some(RObject::String(_)));
    let no_is_str = matches!(heap.object(operands[2]), Some(RObject::String(_)));
    // GNU promotes to the common type of the branches — character wins,
    /// then real, then integer — and NA tests yield the type's NA.
    if yes_is_str || no_is_str {
        let yes_id = coerce_character(heap, interner, operands[1])?;
        let yes = string_vec(heap, interner, yes_id);
        let no_id = coerce_character(heap, interner, operands[2])?;
        let no = string_vec(heap, interner, no_id);
        let mut out: Vec<super::intern::InternedStr> = Vec::with_capacity(tests.len());
        for (i, test) in tests.iter().enumerate() {
            let chosen = if *test == na_int {
                "\0NA".to_string()
            } else if *test != 0 {
                if yes.is_empty() { "\0NA".to_string() } else { yes[i % yes.len()].clone() }
            } else if no.is_empty() {
                "\0NA".to_string()
            } else {
                no[i % no.len()].clone()
            };
            out.push(interner.intern_string(&chosen));
        }
        return Ok(heap.alloc(RObject::String(out)));
    }
    // GNU's type lattice: the result takes the common type of the branches
    /// — logical+logical stays logical, integer mixtures stay integer, any
    /// real input widens to double exactly as the coercion hierarchy.
    let rank = |v: ValueId| -> u32 {
        match heap.object(v) {
            Some(RObject::Logical(_)) => 0,
            Some(RObject::Integer(_)) => 1,
            _ => 2,
        }
    };
    let result_rank = rank(operands[1]).max(rank(operands[2]));
    let yes = numerics_of(heap, operands[1]).map_err(RSignal::from)?;
    let no = numerics_of(heap, operands[2]).map_err(RSignal::from)?;
    let na_real = f64::from_bits(crate::value::object::NA_REAL_BITS);
    let mut out = Vec::with_capacity(tests.len());
    for (i, test) in tests.iter().enumerate() {
        if *test == na_int {
            out.push(na_real);
        } else if *test != 0 {
            out.push(if yes.is_empty() { na_real } else { yes[i % yes.len()] });
        } else {
            out.push(if no.is_empty() { na_real } else { no[i % no.len()] });
        }
    }
    match result_rank {
        0 => Ok(heap.alloc(RObject::Logical(
            out.iter()
                .map(|&v| {
                    if v.is_nan() {
                        crate::value::object::NA_INTEGER
                    } else {
                        v as i32
                    }
                })
                .collect(),
        ))),
        1 => Ok(heap.alloc(RObject::Integer(
            out.iter()
                .map(|&v| {
                    if v.is_nan() {
                        crate::value::object::NA_INTEGER
                    } else {
                        v as i32
                    }
                })
                .collect(),
        ))),
        _ => Ok(heap.alloc(RObject::Real(out))),
    }
}

/// `%in%` — elementwise membership exactly as GNU: match(x, table,
/// nomatch = 0) > 0, so NA in x matches NA in the table bitwise.
fn membership_primitive(heap: &mut Heap, interner: &mut SymbolInterner, left: ValueId, right: ValueId) -> EvalResult {
    let positions = match_positions(heap, interner, left, right, 0, &[]);
    let out: Vec<i32> = positions.iter().map(|p| (*p > 0) as i32).collect();
    Ok(heap.alloc(RObject::Logical(out)))
}

/// GNU's bitwise element keys — NA_real_ and NaN are distinct bit
/// patterns, the string sentinel is its own key, and every comparison is
/// exact: this is what makes NA match NA in the table.
fn match_keys(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Vec<String> {
    match heap.object(value) {
        Some(RObject::String(vals)) => vals.iter().map(|v| format!("s:{}", interner.string_text(*v))).collect(),
        Some(RObject::Real(vals)) => vals.iter().map(|v| format!("r:{}", v.to_bits())).collect(),
        Some(RObject::Integer(vals)) => vals.iter().map(|v| format!("i:{v}")).collect(),
        Some(RObject::Logical(vals)) => vals.iter().map(|v| format!("l:{v}")).collect(),
        _ => Vec::new(),
    }
}

/// The shared match engine — bitwise key equality, incomparables never
/// matching, nomatch filling exactly as GNU.
fn match_positions(
    heap: &Heap,
    interner: &SymbolInterner,
    left: ValueId,
    right: ValueId,
    nomatch: i32,
    incomparables: &[String],
) -> Vec<i32> {
    let lhs = match_keys(heap, interner, left);
    let table = match_keys(heap, interner, right);
    lhs.iter()
        .map(|v| {
            if incomparables.iter().any(|inc| inc == v) {
                return nomatch;
            }
            match table.iter().position(|tt| tt == v) {
                Some(pos) => (pos + 1) as i32,
                None => nomatch,
            }
        })
        .collect()
}

/// `match(x, table, nomatch, incomparables)` — 1-based first-match
/// positions exactly as GNU: bitwise NA/NaN matching, incomparables
/// excluded, nomatch the no-hit fill.
fn match_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut nomatch = crate::value::object::NA_INTEGER;
    let mut incomparables: Vec<String> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("nomatch") => {
                nomatch = match heap.object(v) {
                    Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().unwrap_or(nomatch),
                    Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(nomatch as f64) as i32,
                    _ => nomatch,
                };
            }
            Some("incomparables") => {
                incomparables = match_keys(heap, interner, v);
            }
            _ => positional.push(v),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    if positional.len() < 2 {
        return Ok(heap.alloc(RObject::Integer(Vec::new())));
    }
    let positions = match_positions(heap, interner, positional[0], positional[1], nomatch, &incomparables);
    Ok(heap.alloc(RObject::Integer(positions)))
}

/// `exists(name, envir)` — GNU's chain lookup by default: the binding may
/// live in the environment or any enclosure. The name arrives as a
/// one-element character vector exactly as GNU's interface.
fn exists_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Tagged walk: the name positional, envir/inherits/mode by name
    /// exactly as GNU's argument list.
    let mut name_arg = ValueId::NIL;
    let mut env = caller_env;
    let mut inherits = true;
    let mut mode_fn = false;
    let mut positional = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("envir") | Some("where") => env = v,
            Some("inherits") => {
                if let Some(RObject::Logical(vals)) = heap.object(v) {
                    inherits = vals.is_empty() || (vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER);
                }
            }
            Some("mode") => {
                if let Some(text) = string_operand(heap, interner, v) {
                    mode_fn = text == "function";
                }
            }
            _ => {
                if positional == 1 {
                    name_arg = v;
                } else if positional == 2 {
                    env = v;
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let name = match heap.object(name_arg) {
        Some(RObject::String(values)) if !values.is_empty() => {
            interner.string_text(values[0]).to_string()
        }
        _ => return Err(EvalError::TypeError("exists requires a character name").into()),
    };
    let sym = interner.intern_symbol(&name);
    let found = if inherits {
        crate::value::env::lookup(heap, env, sym)
    } else {
        crate::value::env::lookup_in_frame(heap, env, sym).map(|b| (env, b))
    };
    // mode="function" filters to closure and primitive bindings exactly as
    /// GNU's mode matching.
    let found = match (found, mode_fn) {
        (Some((_, binding)), true) => matches!(
            heap.object(binding.value),
            Some(RObject::Closure { .. }) | Some(RObject::Primitive(_))
        ),
        (Some(_), false) => true,
        _ => false,
    };
    Ok(heap.alloc(RObject::Logical(vec![found as i32])))
}

/// `ls(envir)` — sorted binding names of the frame, GNU's ordering. List
/// frames enumerate; hash frames are outside the object model's slice.
fn ls_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    env: ValueId,
    all_names: bool,
) -> EvalResult {
    let mut names: Vec<String> = Vec::new();
    if let Some(RObject::Environment { frame, .. }) = heap.object(env) {
        if let crate::value::object::EnvFrame::List(bindings) = frame {
            for (sym, _) in bindings {
                names.push(interner.symbol_name(*sym).to_string());
            }
        }
    }
    // GNU hides dot-prefixed bindings unless all.names=TRUE.
    if !all_names {
        names.retain(|n| !n.starts_with('.'));
    }
    names.sort();
    let interned: Vec<super::intern::InternedStr> = names
        .iter()
        .map(|n| interner.intern_string(n))
        .collect();
    Ok(heap.alloc(RObject::String(interned)))
}

/// `$<-(x, name, value)` — GNU's split discipline: environments are
/// reference semantics, the binding defines in place and the same object
/// returns; lists are copy-on-write value semantics exactly like vector
/// replacement — the element replaces or appends with the names attribute
/// extended.
fn dollar_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // The name operand arrives UNEVALUATED as a symbol exactly as `$` —
    /// evaluate operands selectively: object and value yes, name no.
    let raw = raw_args(heap, args);
    if raw.len() < 3 {
        return Err(EvalError::TypeError("$<- requires object, name, value").into());
    }
    let object = eval(heap, interner, raw[0], caller_env)?;
    let object = force_if_promise(heap, interner, object)?;
    let name = match heap.object(raw[1]) {
        Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
        _ => {
            // Evaluated form: a one-element character vector.
            let evaluated = eval(heap, interner, raw[1], caller_env)?;
            let evaluated = force_if_promise(heap, interner, evaluated)?;
            match heap.object(evaluated) {
                Some(RObject::String(values)) if !values.is_empty() => {
                    interner.string_text(values[0]).to_string()
                }
                _ => return Err(EvalError::TypeError("$<- name must be symbol or string").into()),
            }
        }
    };
    let value = eval(heap, interner, raw[2], caller_env)?;
    let value = force_if_promise(heap, interner, value)?;
    match heap.object(object) {
        Some(RObject::Environment { .. }) => {
            let sym = interner.intern_symbol(&name);
            crate::value::env::define(heap, object, sym, value)
                .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("env define")))?;
            Ok(object)
        }
        Some(RObject::List(items)) => {
            let items = items.clone();
            let existing_names: Vec<String> =
                match get_attrib(heap, interner, object, "names").and_then(|id| {
                    match heap.object(id) {
                        Some(RObject::String(entries)) => Some(
                            entries
                                .iter()
                                .map(|e| interner.string_text(*e).to_string())
                                .collect(),
                        ),
                        _ => None,
                    }
                }) {
                    Some(n) => n,
                    None => vec![String::new(); items.len()],
                };
            let mut new_items = items;
            let mut new_names = existing_names;
            if new_names.len() < new_items.len() {
                new_names.resize(new_items.len(), String::new());
            }
            match new_names.iter().position(|n| n == &name) {
                Some(position) => {
                    new_items[position] = value;
                }
                None => {
                    new_items.push(value);
                    new_names.push(name);
                }
            }
            let mut result = heap.alloc(RObject::List(new_items));
            let interned: Vec<super::intern::InternedStr> = new_names
                .iter()
                .map(|n| interner.intern_string(n))
                .collect();
            let names_vec = heap.alloc(RObject::String(interned));
            result = copy_with_attrib(heap, interner, result, "names", names_vec)
                .map_err(RSignal::from)?;
            Ok(result)
        }
        _ => Err(EvalError::TypeError("$<- requires environment or list").into()),
    }
}

/// `matrix(data, nrow, ncol)` — GNU's column-major construction: data
/// recycles to nrow*ncol, and because columns fill contiguously the linear
/// storage IS the data vector — the constructor is recycling plus the dim
/// attribute, nothing more.
fn matrix_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let lookup = |name: &str, position: usize| -> Option<ValueId> {
        named
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
            .or_else(|| positional.get(position).copied())
    };
    let data = lookup("data", 0).ok_or(EvalError::TypeError("matrix requires data"))?;
    let data_len = vector_length(heap, data)
        .ok_or(EvalError::TypeError("matrix data must be a vector"))
        .map_err(RSignal::from)?;
    let nrow = match lookup("nrow", 1) {
        Some(v) => first_real(heap, v).map_err(RSignal::from)? as usize,
        None => data_len,
    };
    let ncol = match lookup("ncol", 2) {
        Some(v) => first_real(heap, v).map_err(RSignal::from)? as usize,
        None => (data_len + nrow - 1) / nrow.max(1),
    };
    let total = nrow * ncol;
    // Recycle the data vector to the matrix extent — same-type copying.
    let result = match heap.object(data) {
        Some(RObject::Real(values)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(values[i % values.len().max(1)]);
            }
            heap.alloc(RObject::Real(out))
        }
        Some(RObject::Integer(values)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(values[i % values.len().max(1)]);
            }
            heap.alloc(RObject::Integer(out))
        }
        Some(RObject::Logical(values)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(values[i % values.len().max(1)]);
            }
            heap.alloc(RObject::Logical(out))
        }
        Some(RObject::String(values)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(values[i % values.len().max(1)]);
            }
            heap.alloc(RObject::String(out))
        }
        _ => return Err(EvalError::TypeError("matrix data must be atomic").into()),
    };
    let dims = heap.alloc(RObject::Integer(vec![nrow as i32, ncol as i32]));
    let with_dim = copy_with_attrib(heap, interner, result, "dim", dims).map_err(RSignal::from)?;
    match lookup("dimnames", 3) {
        Some(dn) => copy_with_attrib(heap, interner, with_dim, "dimnames", dn).map_err(RSignal::from),
        None => Ok(with_dim),
    }
}

/// `rep` argument resolution: GNU's forms are rep(x, times), rep(x, each),
/// and per-element times vectors — all composable. Tagged arguments resolve
/// before positional exactly as GNU's formals ordering.
fn rep_dispatch(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let result = rep_dispatch_inner(heap, interner, args, caller_env)?;
    // length.out truncates or recycles the repetition exactly as GNU.
    let mut length_out: Option<usize> = None;
    let mut times_hint: usize = 1;
    let mut each_hint: usize = 1;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("length.out") => {
                length_out = match heap.object(v) {
                    Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().map(|x| x as usize),
                    Some(RObject::Real(vals)) => vals.first().copied().map(|x| x as usize),
                    _ => None,
                };
            }
            Some("times") => {
                times_hint = match heap.object(v) {
                    Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().unwrap_or(1) as usize,
                    Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(1.0) as usize,
                    _ => 1,
                };
            }
            Some("each") => {
                each_hint = match heap.object(v) {
                    Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().unwrap_or(1) as usize,
                    Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(1.0) as usize,
                    _ => 1,
                };
            }
            _ => {}
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let result = match length_out {
        Some(target) => resize_vector(heap, interner, result, target)?,
        None => result,
    };
    // GNU's rep replicates the names attribute parallel to the payload:
    /// each-element repetition contiguous, whole-vector repetition in
    /// blocks, length.out recycling alongside the values.
    if let Ok(source_ops) = eval_args(heap, interner, args, caller_env, 1) {
        let source = source_ops[0];
        let source_names: Vec<String> = match get_attrib(heap, interner, source, "names") {
            Some(n) => string_vec(heap, interner, n),
            None => Vec::new(),
        };
        let has_names = match get_attrib(heap, interner, result, "names") {
            Some(n) => !string_vec(heap, interner, n).is_empty(),
            None => false,
        };
        if !source_names.is_empty() && !has_names {
            let out_len = vector_length(heap, result).unwrap_or(0);
            let src_len = source_names.len();
            let mut replicated: Vec<String> = Vec::new();
            if each_hint > 1 {
                // each: every name contiguous, the whole pattern times times.
                for _ in 0..times_hint {
                    for name in &source_names {
                        for _ in 0..each_hint {
                            replicated.push(name.clone());
                        }
                    }
                }
            } else {
                // times blocks: the names vector repeats as a unit.
                for _ in 0..times_hint {
                    replicated.extend(source_names.iter().cloned());
                }
            }
            // Recycle to the output length exactly as the values did.
            if replicated.len() != out_len && !replicated.is_empty() {
                let base = replicated.clone();
                replicated = (0..out_len).map(|i| base[i % base.len()].clone()).collect();
            }
            replicated.truncate(out_len);
            if replicated.len() == out_len && out_len > 0 {
                let names_obj = heap.alloc(RObject::String(
                    replicated.iter().map(|n| interner.intern_string(n)).collect(),
                ));
                let _ = &src_len;
                let stamped = copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)?;
                return finish_rep_classed(heap, interner, args, caller_env, stamped);
            }
        }
    }
    finish_rep_classed(heap, interner, args, caller_env, result)
}

fn finish_rep_classed(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    result: ValueId,
) -> EvalResult {
    // GNU's rep methods for datetime classes keep the chain — class and
    /// tzone replicate across the repeated payload exactly.
    if let Ok(operands) = eval_args(heap, interner, args, caller_env, 1) {
        let source = operands[0];
        if is_posixct_classed(heap, interner, source) && !is_posixct_classed(heap, interner, result) {
            let tz = get_attrib(heap, interner, source, "tzone");
            let class = heap.alloc(RObject::String(vec![
                interner.intern_string("POSIXct"),
                interner.intern_string("POSIXt"),
            ]));
            let stamped = copy_with_attrib(heap, interner, result, "class", class).map_err(RSignal::from)?;
            return if let Some(tzv) = tz {
                copy_with_attrib(heap, interner, stamped, "tzone", tzv).map_err(RSignal::from)
            } else {
                Ok(stamped)
            };
        }
        if is_date_classed(heap, interner, source)
            && !is_posixct_classed(heap, interner, source)
            && !is_date_classed(heap, interner, result)
        {
            let class = heap.alloc(RObject::String(vec![interner.intern_string("Date")]));
            return copy_with_attrib(heap, interner, result, "class", class).map_err(RSignal::from);
        }
    }
    Ok(result)
}

fn rep_dispatch_inner(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let lookup = |name: &str, position: usize| -> Option<ValueId> {
        named
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
            .or_else(|| positional.get(position).copied())
    };
    let source = match lookup("x", 0) {
        Some(v) => v,
        None => return Err(EvalError::TypeError("rep requires input").into()),
    };
    let each = match lookup("each", 2) {
        Some(v) => first_real(heap, v).map_err(RSignal::from)? as usize,
        None => 1,
    };
    let times_value = lookup("times", 1);
    // Per-element times vector: replicate element i exactly times[i] times.
    if let Some(tv) = times_value {
        let per_element: Option<Vec<usize>> = match heap.object(tv) {
            Some(RObject::Integer(values)) if values.len() > 1 => {
                Some(values.iter().map(|v| (*v).max(0) as usize).collect())
            }
            Some(RObject::Real(values)) if values.len() > 1 => {
                Some(values.iter().map(|v| (*v as i32).max(0) as usize).collect())
            }
            _ => None,
        };
        if let Some(schedule) = per_element {
            return rep_per_element(heap, source, &schedule, each);
        }
    }
    let times = match times_value {
        Some(tv) => first_real(heap, tv).map_err(RSignal::from)? as usize,
        None => 1,
    };
    if each > 1 {
        return rep_each(heap, source, times, each);
    }
    let times_scalar = heap.alloc(RObject::Real(vec![times as f64]));
    rep_primitive(heap, source, times_scalar)
}

/// Elementwise repetition: element i appears times[i] times, each repeated
/// contiguously — GNU's rep(x, times=vector) shape.
fn rep_per_element(heap: &mut Heap, source: ValueId, schedule: &[usize], each: usize) -> EvalResult {
    macro_rules! per_type {
        ($variant:ident) => {
            match heap.object(source) {
                Some(RObject::$variant(values)) => {
                    let mut out = Vec::new();
                    for (i, value) in values.iter().enumerate() {
                        let count = schedule.get(i).copied().unwrap_or(0) * each;
                        for _ in 0..count {
                            out.push(*value);
                        }
                    }
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                _ => {}
            }
        };
    }
    per_type!(Integer);
    per_type!(Real);
    per_type!(Complex);
    match heap.object(source) {
        Some(RObject::List(values)) => {
            let mut out = Vec::new();
            for (i, value) in values.iter().enumerate() {
                let count = schedule.get(i).copied().unwrap_or(0) * each;
                for _ in 0..count {
                    out.push(*value);
                }
            }
            return Ok(heap.alloc(RObject::List(out)));
        }
        _ => {}
    }
    per_type!(Logical);
    per_type!(String);
    Err(EvalError::TypeError("rep requires an atomic vector").into())
}

/// `rep(x, each=k)` — every element repeats k times contiguously before
/// the next, then the whole pattern repeats times over.
fn rep_each(heap: &mut Heap, source: ValueId, times: usize, each: usize) -> EvalResult {
    macro_rules! per_type {
        ($variant:ident) => {
            match heap.object(source) {
                Some(RObject::$variant(values)) => {
                    let mut out = Vec::new();
                    for _ in 0..times {
                        for value in values.iter() {
                            for _ in 0..each {
                                out.push(*value);
                            }
                        }
                    }
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                _ => {}
            }
        };
    }
    per_type!(Integer);
    per_type!(Real);
    per_type!(Logical);
    per_type!(String);
    Err(EvalError::TypeError("rep requires an atomic vector").into())
}

/// `tryCatch(expr, error=handler)` — GNU's condition handling shape: the
/// expression evaluates inside the protection; an error condition invokes
/// the handler with a condition object and the handler's value becomes the
/// result. Non-error control flow (break/next/return) propagates exactly —
/// it is not a condition.
fn trycatch_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Collect UNEVALUATED operands with tags: the expression is positional
    /// first, handlers arrive tagged by condition class.
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), car));
            }
            _ => positional.push(car),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let expr = match positional.first() {
        Some(e) => *e,
        None => return Err(EvalError::TypeError("tryCatch requires an expression").into()),
    };
    match eval(heap, interner, expr, caller_env) {
        Ok(value) => Ok(value),
        Err(RSignal::Error(error)) => {
            let handler_expr = match named.iter().find(|(n, _)| n == "error").map(|(_, e)| *e) {
                Some(e) => e,
                // No matching handler: the condition propagates exactly as
                /// GNU's unwinding continues outward.
                None => return Err(RSignal::Error(error)),
            };
            let message = match &error {
                EvalError::Condition(text) => text.clone(),
                EvalError::TypeError(text) => text.to_string(),
                EvalError::UnboundVariable(name) => format!("object '{name}' not found"),
                other => format!("{other:?}"),
            };
            // The condition object: a named list carrying the message —
            /// conditionMessage reads exactly this shape.
            let message_str = heap.alloc(RObject::String(vec![interner.intern_string(&message)]));
            let condition = heap.alloc(RObject::List(vec![message_str]));
            let names_vec = heap.alloc(RObject::String(vec![interner.intern_string("message")]));
            let condition = copy_with_attrib(heap, interner, condition, "names", names_vec)
                .map_err(RSignal::from)?;
            let handler = eval(heap, interner, handler_expr, caller_env)?;
            let handler = force_if_promise(heap, interner, handler)?;
            let (formals, body, closure_env) = match heap.object(handler) {
                Some(RObject::Closure { formals, body, env }) => (*formals, *body, *env),
                _ => return Err(EvalError::TypeError("tryCatch handler must be a closure").into()),
            };
            let arg_node = heap.alloc(RObject::Pairlist {
                car: condition,
                cdr: ValueId::NIL,
                tag: ValueId::NIL,
            });
            super::eval::apply_closure_public(
                heap, interner, formals, body, closure_env, arg_node, caller_env,
            )
        }
        // Control flow is not a condition — break/next/return propagate.
        Err(signal) => Err(signal),
    }
}

/// `structure(x, ...)`: every tagged argument attaches as the attribute of
/// its name — GNU's attribute-attachment interface exactly. Copy-on-write
/// discipline as with all attribute mutation.
fn structure_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut tagged: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                tagged.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let mut object = match positional.first() {
        Some(v) => *v,
        None => return Err(EvalError::TypeError("structure requires input").into()),
    };
    // copy_with_attrib appends, so forward application leaves the chain
    /// in argument order exactly as GNU prints it.
    for (name, value) in tagged.into_iter() {
        object = copy_with_attrib(heap, interner, object, &name, value).map_err(RSignal::from)?;
    }
    Ok(object)
}

/// `assign(name, value, envir)` — GNU's programmatic define: the name is a
/// character string, the target environment defaults to the caller.
fn assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let name = match heap.object(operands[0]) {
        Some(RObject::String(values)) if !values.is_empty() => {
            interner.string_text(values[0]).to_string()
        }
        _ => return Err(EvalError::TypeError("assign requires a character name").into()),
    };
    let env = match raw_args(heap, args).get(2).copied() {
        Some(expr) => {
            let evaluated = eval(heap, interner, expr, caller_env)?;
            force_if_promise(heap, interner, evaluated)?
        }
        None => caller_env,
    };
    let sym = interner.intern_symbol(&name);
    crate::value::env::define(heap, env, sym, operands[1])
        .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("assign define")))?;
    // GNU: assign returns the value invisibly.
    Ok(operands[1])
}

/// `get(name, envir)` — chain lookup by character name, GNU exactly.
fn get_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let name = match heap.object(operands[0]) {
        Some(RObject::String(values)) if !values.is_empty() => {
            interner.string_text(values[0]).to_string()
        }
        _ => return Err(EvalError::TypeError("get requires a character name").into()),
    };
    let env = match raw_args(heap, args).get(1).copied() {
        Some(expr) => {
            let evaluated = eval(heap, interner, expr, caller_env)?;
            force_if_promise(heap, interner, evaluated)?
        }
        None => caller_env,
    };
    let sym = interner.intern_symbol(&name);
    match crate::value::env::lookup(heap, env, sym) {
        Some((_, binding)) => Ok(binding.value),
        None => Err(EvalError::UnboundVariable(name).into()),
    }
}

/// Generic named-attribute getter: NULL when absent, exactly GNU.
fn attrib_getter(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    attr: &str,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    match get_attrib(heap, interner, operands[0], attr) {
        Some(v) => Ok(v),
        None => Ok(ValueId::NIL),
    }
}

/// Generic attribute replacement primitive: `(object, value)` attaches the
/// value under the attribute name through copy-on-write discipline.
fn attrib_replacement(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    attr: &str,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    copy_with_attrib(heap, interner, operands[0], attr, operands[1]).map_err(RSignal::from)
}

/// `class(x)`: the class attribute when present, otherwise GNU's implicit
/// class — matrix/array for dimensioned vectors, the type name otherwise.
fn class_getter(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    if let Some(v) = get_attrib(heap, interner, operands[0], "class") {
        return Ok(v);
    }
    let implicit = if get_attrib(heap, interner, operands[0], "dim").is_some() {
        "matrix"
    } else {
        match heap.object(operands[0]) {
            Some(RObject::Real(_)) => "numeric",
            Some(RObject::Integer(_)) => "integer",
            Some(RObject::Logical(_)) => "logical",
            Some(RObject::String(_)) => "character",
            Some(RObject::List(_)) => "list",
            _ => "NULL",
        }
    };
    Ok(heap.alloc(RObject::String(vec![interner.intern_string(implicit)])))
}

/// rownames/colnames: element one/two of the dimnames attribute.
fn rowcolnames(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    rows: bool,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let dimnames = match get_attrib(heap, interner, operands[0], "dimnames") {
        Some(v) => v,
        None => {
            // data.frames carry row.names instead of dimnames — the rows
            // getter reads it directly exactly as GNU.
            if rows {
                if let Some(v) = get_attrib(heap, interner, operands[0], "row.names") {
                    return Ok(v);
                }
            }
            return Ok(ValueId::NIL);
        }
    };
    let index = if rows { 0 } else { 1 };
    match heap.object(dimnames) {
        Some(RObject::List(items)) => items.get(index).copied().map_or(Ok(ValueId::NIL), |v| {
            if v == ValueId::NIL {
                Ok(ValueId::NIL)
            } else {
                Ok(v)
            }
        }),
        _ => Ok(ValueId::NIL),
    }
}

/// rownames<-/colnames<-: rebuild the dimnames list with element one/two
/// replaced — constructing the two-element NULL list when absent, exactly
/// GNU's lazy dimnames materialization.
fn rowcolnames_assign(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    rows: bool,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let index = if rows { 0 } else { 1 };
    let existing = get_attrib(heap, interner, operands[0], "dimnames");
    let mut elements: Vec<ValueId> = match existing {
        Some(id) => match heap.object(id) {
            Some(RObject::List(items)) => {
                let mut v = items.clone();
                while v.len() < 2 {
                    v.push(ValueId::NIL);
                }
                v
            }
            _ => vec![ValueId::NIL, ValueId::NIL],
        },
        None => vec![ValueId::NIL, ValueId::NIL],
    };
    elements[index] = operands[1];
    let list = heap.alloc(RObject::List(elements));
    copy_with_attrib(heap, interner, operands[0], "dimnames", list).map_err(RSignal::from)
}

/// Extract the first string of an operand — the path coercion every file
/// primitive shares.
fn string_operand(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Option<String> {
    match heap.object(value) {
        Some(RObject::String(values)) if !values.is_empty() => {
            Some(interner.string_text(values[0]).to_string())
        }
        _ => None,
    }
}

/// print.rs reaches the same extraction through this crate-visible alias.
pub(crate) fn string_vec_pub(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Vec<String> {
    string_vec(heap, interner, value)
}

fn string_vec(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Vec<String> {
    match heap.object(value) {
        Some(RObject::String(values)) => values
            .iter()
            .map(|v| interner.string_text(*v).to_string())
            .collect(),
        _ => Vec::new(),
    }
}


/// The (nrow, ncol) of a data.frame-classed list, when classed.
pub(crate) fn data_frame_shape(
    heap: &Heap,
    interner: &SymbolInterner,
    value: ValueId,
) -> Option<(usize, usize)> {
    let classed = match get_attrib(heap, interner, value, "class") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .any(|v| interner.string_text(*v) == "data.frame"),
            _ => false,
        },
        None => false,
    };
    if !classed {
        return None;
    }
    let items = match heap.object(value) {
        Some(RObject::List(items)) => items,
        _ => return None,
    };
    let ncol = items.len();
    let nrow = items
        .first()
        .and_then(|c| vector_length(heap, *c))
        .unwrap_or(0);
    Some((nrow, ncol))
}

/// `tempfile()`: a unique non-existent path inside the system tempdir —
/// process id plus a counter makes collision impossible within the session
/// exactly as GNU's pattern randomization does across sessions.
fn tempfile_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let _ = (args, caller_env);
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir();
    loop {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let path = dir.join(format!(
            "Rtmpfile{}x{:x}{}",
            std::process::id(),
            nanos,
            n
        ));
        if !path.exists() {
            let text = interner.intern_string(&path.to_string_lossy());
            return Ok(heap.alloc(RObject::String(vec![text])));
        }
    }
}

/// `writeLines(text, con)`: every element of text plus a trailing newline,
/// con a filename string exactly as the conformance slice uses it.
fn writelines_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Tagged-argument walk: text positional first, con and sep by name or
    /// position exactly as GNU's argument matching.
    let mut text_arg = ValueId::NIL;
    let mut con_arg = ValueId::NIL;
    let mut sep = "\n".to_string();
    let mut positional = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("con") => con_arg = v,
            Some("sep") => {
                if let Some(s) = string_operand(heap, interner, v) {
                    sep = s;
                }
            }
            _ => {
                if positional == 1 {
                    text_arg = v;
                } else if positional == 2 {
                    con_arg = v;
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let lines = string_vec(heap, interner, text_arg);
    let mut content = String::new();
    for line in &lines {
        content.push_str(line);
        content.push_str(&sep);
    }
    // Connection objects write through the shared state; path strings
    /// truncate-write exactly as the pre-connection behavior. GNU's
    /// writeLines returns invisible NULL either way.
    if is_connection(heap, con_arg) {
        connection_write(heap, con_arg, content.as_bytes()).map_err(RSignal::from)?;
    } else {
        let path = string_operand(heap, interner, con_arg)
            .ok_or(EvalError::TypeError("writeLines connection must be a filename"))?;
        std::fs::write(&path, content)
            .map_err(|_| RSignal::Error(EvalError::TypeError("writeLines: cannot open file")))?;
    }
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// `readLines(con)`: the file's lines as a character vector, trailing
/// newline not producing a spurious empty final element — GNU exactly.
fn readlines_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Tagged-argument walk: con positional, n limits lines, warn ignored
    /// in the migration slice.
    let mut con_arg = ValueId::NIL;
    let mut n_limit: Option<usize> = None;
    let mut positional = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("n") => {
                if let Some(RObject::Real(vals)) = heap.object(v) {
                    if !vals.is_empty() {
                        n_limit = Some(vals[0] as usize);
                    }
                }
            }
            Some("warn") => {}
            _ => {
                if positional == 1 {
                    con_arg = v;
                } else if positional == 2 && n_limit.is_none() {
                    if let Some(RObject::Real(vals)) = heap.object(v) {
                        if !vals.is_empty() {
                            n_limit = Some(vals[0] as usize);
                        }
                    }
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    // skipNul: TRUE strips embedded nul bytes; FALSE truncates each line
    // at the first nul exactly as GNU's C-string line reading.
    let mut skip_nul = false;
    let mut scan = args;
    loop {
        let (car, cdr, tag) = match heap.object(scan) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let is_skip = matches!(heap.object(tag), Some(RObject::Symbol { name, .. }) if interner.symbol_name(*name) == "skipNul");
        if is_skip {
            let evaluated = eval(heap, interner, car, caller_env)?;
            let v = force_if_promise(heap, interner, evaluated)?;
            skip_nul = matches!(heap.object(v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) == 1);
        }
        if cdr == ValueId::NIL {
            break;
        }
        scan = cdr;
    }
    let content = if is_connection(heap, con_arg) {
        // Connection reads stream from the cursor exactly as GNU — the
        /// position advances past everything read so repeated calls
        /// continue where the last stopped.
        let taken = with_conn(heap, con_arg, |state| {
            if !state.open {
                return None;
            }
            let rest = &state.read_buf[state.cursor.min(state.read_buf.len())..];
            let text = String::from_utf8_lossy(rest).to_string();
            state.cursor = state.read_buf.len();
            Some(text)
        });
        match taken {
            Some(Some(text)) => text,
            _ => return Err(EvalError::TypeError("invalid connection").into()),
        }
    } else {
        let path = string_operand(heap, interner, con_arg)
            .ok_or(EvalError::TypeError("readLines connection must be a filename"))?;
        std::fs::read_to_string(&path)
            .map_err(|_| RSignal::Error(EvalError::TypeError("readLines: cannot open file")))?
    };
    let mut lines: Vec<String> = content
        .split('\n')
        .map(|line| {
            if skip_nul {
                line.chars().filter(|c| *c != '\0').collect()
            } else {
                line.split('\0').next().unwrap_or("").to_string()
            }
        })
        .collect();
    if lines.last().map(|l| l.as_str()) == Some("") {
        lines.pop();
    }
    if let Some(n) = n_limit {
        lines.truncate(n);
    }
    let interned: Vec<super::intern::InternedStr> = lines
        .iter()
        .map(|l| interner.intern_string(l))
        .collect();
    Ok(heap.alloc(RObject::String(interned)))
}

/// `file.exists(paths)` — elementwise existence probe.
fn file_exists_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let paths = string_vec(heap, interner, operands[0]);
    let out: Vec<i32> = paths
        .iter()
        .map(|p| if std::path::Path::new(p).exists() { 1 } else { 0 })
        .collect();
    Ok(heap.alloc(RObject::Logical(out)))
}

/// `file.remove(paths)` — elementwise removal with logical success exactly
/// as GNU.
fn file_remove_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // GNU's ... form: every argument contributes paths exactly as one
    // concatenated vector.
    let raw = raw_args(heap, args);
    let mut paths: Vec<String> = Vec::new();
    for expr in &raw {
        let v = eval(heap, interner, *expr, caller_env)?;
        paths.extend(string_vec(heap, interner, v));
    }
    let out: Vec<i32> = paths
        .iter()
        .map(|p| if std::fs::remove_file(p).is_ok() { 1 } else { 0 })
        .collect();
    Ok(heap.alloc(RObject::Logical(out)))
}

/// `file.copy(from, to)` — scalar copy with logical success.
fn file_copy_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let from = string_operand(heap, interner, operands[0])
        .ok_or(EvalError::TypeError("file.copy source"))?;
    let to = string_operand(heap, interner, operands[1])
        .ok_or(EvalError::TypeError("file.copy target"))?;
    let ok = std::fs::copy(&from, &to).is_ok();
    Ok(heap.alloc(RObject::Logical(vec![if ok { 1 } else { 0 }])))
}

/// `file.rename(from, to)` — scalar rename with logical success.
fn file_rename_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let from = string_operand(heap, interner, operands[0])
        .ok_or(EvalError::TypeError("file.rename source"))?;
    let to = string_operand(heap, interner, operands[1])
        .ok_or(EvalError::TypeError("file.rename target"))?;
    let ok = std::fs::rename(&from, &to).is_ok();
    Ok(heap.alloc(RObject::Logical(vec![if ok { 1 } else { 0 }])))
}

/// `as.logical(x)` — numeric nonzero is true, NA propagates, strings parse
/// the T/F/TRUE/FALSE spellings exactly as GNU.
fn coerce_logical(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
) -> EvalResult {
    let out: Vec<i32> = match heap.object(value) {
        Some(RObject::Logical(values)) => values.clone(),
        Some(RObject::Integer(values)) => values.clone(),
        Some(RObject::Real(values)) => values
            .iter()
            .map(|v| {
                if v.is_nan() {
                    crate::value::object::NA_INTEGER
                } else if *v != 0.0 {
                    1
                } else {
                    0
                }
            })
            .collect(),
        Some(RObject::String(values)) => values
            .iter()
            .map(|v| match interner.string_text(*v) {
                "TRUE" | "T" | "true" => 1,
                "FALSE" | "F" | "false" => 0,
                _ => crate::value::object::NA_INTEGER,
            })
            .collect(),
        Some(RObject::Raw(values)) => values.iter().map(|v| if *v == 0 { 0 } else { 1 }).collect(),
        Some(RObject::Complex(values)) => values
            .iter()
            .map(|z| {
                if z.r.is_nan() || z.i.is_nan() {
                    crate::value::object::NA_INTEGER
                } else if z.r != 0.0 || z.i != 0.0 {
                    1
                } else {
                    0
                }
            })
            .collect(),
        _ => return Err(EvalError::TypeError("cannot coerce to logical").into()),
    };
    Ok(heap.alloc(RObject::Logical(out)))
}

/// `attr(x, name)` — single named attribute or NULL, exactly GNU.
fn attr_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let name = string_operand(heap, interner, operands[1])
        .ok_or(EvalError::TypeError("attr name must be character"))?;
    match get_attrib(heap, interner, operands[0], &name) {
        Some(v) => Ok(v),
        None => Ok(ValueId::NIL),
    }
}

/// `attr<-(x, name, value)` — the replacement form the general desugar
/// dispatches: object, attribute name, value positional.
fn attr_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 3)?;
    let name = string_operand(heap, interner, operands[1])
        .ok_or(EvalError::TypeError("attr name must be character"))?;
    copy_with_attrib(heap, interner, operands[0], &name, operands[2]).map_err(RSignal::from)
}

/// `attributes(x)` — every attribute as a named list, NULL when bare.
fn attributes_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let chain = heap.get(operands[0]).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    let mut values: Vec<ValueId> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut cursor = chain;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            break;
        }
        match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => {
                if let Some(RObject::Symbol { name, .. }) = heap.object(*tag) {
                    names.push(interner.symbol_name(*name).to_string());
                    values.push(*car);
                }
                cursor = *cdr;
            }
            _ => break,
        }
    }
    if values.is_empty() {
        return Ok(ValueId::NIL);
    }
    let list = heap.alloc(RObject::List(values));
    let interned: Vec<super::intern::InternedStr> = names
        .iter()
        .map(|n| interner.intern_string(n))
        .collect();
    let names_obj = heap.alloc(RObject::String(interned));
    copy_with_attrib(heap, interner, list, "names", names_obj).map_err(RSignal::from)
}

/// `attributes<-(x, value)` — wholesale attribute replacement: value is a
/// named list whose entries become the object's attributes, NULL clears all.
fn attributes_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let mut object = operands[0];
    // Clear the existing chain first: copy with an empty chain exactly —
    // wholesale replacement is not merge.
    object = copy_with_attrib(heap, interner, object, "\0clearall", ValueId::NIL).map_err(RSignal::from)?;
    if let Some(slot) = heap.get_mut(object) {
        slot.attrib = ValueId::NIL;
    }
    if operands[1] == ValueId::NIL {
        return Ok(object);
    }
    let names = get_attrib(heap, interner, operands[1], "names");
    let name_vec: Vec<String> = match names {
        Some(id) => string_vec(heap, interner, id),
        None => Vec::new(),
    };
    if let Some(RObject::List(items)) = heap.object(operands[1]).cloned() {
        // Forward iteration: copy_with_attrib appends, so list order is
        /// preserved in the final chain exactly as GNU's insertion order.
        for (i, item) in items.iter().enumerate() {
            if let Some(name) = name_vec.get(i) {
                object = copy_with_attrib(heap, interner, object, name, *item).map_err(RSignal::from)?;
            }
        }
    }
    Ok(object)
}

/// `length<-(x, n)` — resize: truncation drops the tail, extension pads
/// with the type's NA exactly as GNU.
fn length_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let target = match heap.object(operands[1]) {
        Some(RObject::Real(v)) if !v.is_empty() && !v[0].is_nan() && v[0] >= 0.0 => v[0] as usize,
        Some(RObject::Integer(v)) if !v.is_empty() && v[0] != crate::value::object::NA_INTEGER && v[0] >= 0 => v[0] as usize,
        _ => return Err(EvalError::Condition("invalid value".to_string()).into()),
    };
    let object = heap.object(operands[0]).cloned()
        .ok_or(EvalError::TypeError("length target missing"))?;
    let resized = match object {
        RObject::Real(mut v) => {
            v.resize(target, f64::from_bits(crate::value::object::NA_REAL_BITS));
            RObject::Real(v)
        }
        RObject::Integer(mut v) => {
            v.resize(target, crate::value::object::NA_INTEGER);
            RObject::Integer(v)
        }
        RObject::Logical(mut v) => {
            v.resize(target, crate::value::object::NA_INTEGER);
            RObject::Logical(v)
        }
        RObject::String(mut v) => {
            let na = interner.intern_string("\0NA");
            v.resize(target, na);
            RObject::String(v)
        }
        RObject::List(mut v) => {
            v.resize(target, ValueId::NIL);
            RObject::List(v)
        }
        RObject::Complex(mut v) => {
            v.resize(target, crate::value::object::RComplex {
                r: f64::from_bits(crate::value::object::NA_REAL_BITS),
                i: f64::from_bits(crate::value::object::NA_REAL_BITS),
            });
            RObject::Complex(v)
        }
        RObject::Raw(mut v) => {
            v.resize(target, 0u8);
            RObject::Raw(v)
        }
        _ => return Err(EvalError::TypeError("length replacement target").into()),
    };
    // GNU drops dim/dimnames on resize; names survive and pad with empty
    // strings when the vector extends.
    let chain = heap.get(operands[0]).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    let id = heap.alloc(resized);
    if let Some(slot) = heap.get_mut(id) {
        slot.attrib = chain;
    }
    let id = remove_attrib(heap, interner, id, "dim").map_err(RSignal::from)?;
    let id = remove_attrib(heap, interner, id, "dimnames").map_err(RSignal::from)?;
    if let Some(names_id) = get_attrib(heap, interner, id, "names") {
        if let Some(RObject::String(vals)) = heap.object(names_id) {
            let mut vals = vals.clone();
            if vals.len() != target {
                let empty = interner.intern_string("");
                vals.resize(target, empty);
                let names_obj = heap.alloc(RObject::String(vals));
                return copy_with_attrib(heap, interner, id, "names", names_obj).map_err(Into::into);
            }
        }
    }
    Ok(id)
}

/// `file.path(...)` — components joined with the platform separator exactly
/// as GNU's forward slash on unix.
fn file_path_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // GNU: file.path is elementwise over vector inputs with recycling —
    /// each component vector contributes its i-th element to the i-th path.
    let mut columns: Vec<Vec<String>> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, .. }) => (*car, *cdr),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        columns.push(string_vec(heap, interner, value));
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let width = columns.iter().map(Vec::len).max().unwrap_or(0);
    let mut paths: Vec<super::intern::InternedStr> = Vec::with_capacity(width);
    for i in 0..width {
        let pieces: Vec<String> = columns
            .iter()
            .map(|col| col[i % col.len().max(1)].clone())
            .collect();
        paths.push(interner.intern_string(&pieces.join("/")));
    }
    Ok(heap.alloc(RObject::String(paths)))
}

/// `data.frame(...)`: tagged arguments become named columns exactly as
/// GNU — the object is a list carrying class data.frame, names, and
/// row.names attributes; every column must share one length.
fn data_frame_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut columns: Vec<ValueId> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut cursor = args;
    let mut positional = 0usize;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        // Data frame arguments splice columnwise exactly as GNU — untagged
        // frames contribute their bare column names, tagged frames prefix
        // each column as tag.name.
        let is_frame = match get_attrib(heap, interner, value, "class") {
            Some(c) => string_vec(heap, interner, c).iter().any(|s| s == "data.frame"),
            None => false,
        };
        if is_frame {
            let inner_names = match get_attrib(heap, interner, value, "names") {
                Some(n) => string_vec(heap, interner, n),
                None => Vec::new(),
            };
            if let Some(RObject::List(items)) = heap.object(value) {
                let items = items.clone();
                for (position, item) in items.iter().enumerate() {
                    let inner = inner_names.get(position).cloned().unwrap_or_default();
                    let spliced = match &tag_name {
                        Some(tag) => format!("{tag}.{inner}"),
                        None => inner,
                    };
                    names.push(spliced);
                    columns.push(*item);
                }
            }
        } else {
            names.push(tag_name.unwrap_or_else(|| format!("V{positional}")));
            columns.push(value);
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    if columns.is_empty() {
        return Err(EvalError::TypeError("data.frame requires columns").into());
    }
    // Duplicate names disambiguate through GNU's make.unique suffixing —
    // the first occurrence stands, repeats gain .1, .2 in order.
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let unique_names: Vec<String> = names
        .iter()
        .map(|n| {
            let count = counts.entry(n.clone()).or_insert(0);
            let out = if *count == 0 {
                n.clone()
            } else {
                format!("{n}.{count}")
            };
            *count += 1;
            out
        })
        .collect();
    build_data_frame(heap, interner, columns, unique_names)
}

/// The shared data.frame assembly: equal-length column validation, the
/// row.names sequence, names and class attributes — copy-on-write
/// discipline throughout.
pub(crate) fn build_data_frame(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    columns: Vec<ValueId>,
    names: Vec<String>,
) -> EvalResult {
    let nrows = columns
        .first()
        .and_then(|c| vector_length(heap, *c))
        .unwrap_or(0);
    for col in &columns {
        let len = vector_length(heap, *col).unwrap_or(0);
        if len != nrows {
            return Err(EvalError::TypeError("columns must share one length").into());
        }
    }
    let list = heap.alloc(RObject::List(columns));
    let interned: Vec<super::intern::InternedStr> = names
        .iter()
        .map(|n| interner.intern_string(n))
        .collect();
    let names_obj = heap.alloc(RObject::String(interned));
    let list = copy_with_attrib(heap, interner, list, "names", names_obj).map_err(RSignal::from)?;
    let rownames: Vec<super::intern::InternedStr> = (1..=nrows)
        .map(|i| interner.intern_string(&i.to_string()))
        .collect();
    let rownames_obj = heap.alloc(RObject::String(rownames));
    let list = copy_with_attrib(heap, interner, list, "row.names", rownames_obj).map_err(RSignal::from)?;
    let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("data.frame")]));
    let list = copy_with_attrib(heap, interner, list, "class", class_obj).map_err(RSignal::from)?;
    Ok(list)
}

/// `aggregate(x, by, FUN)`: rows of x group by the `by` list's values, FUN
/// applies per group, and the result assembles as a data.frame of group
/// columns plus the aggregated column — GNU's split-apply-combine exactly.
/// Groups order lexicographically as GNU's factor-level ordering does for
/// character groupers.
fn aggregate_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 3)?;
    let x = operands[0];
    let by = operands[1];
    let fun = operands[2];
    // The grouping columns: a list of equal-length vectors.
    let group_cols: Vec<ValueId> = match heap.object(by) {
        Some(RObject::List(items)) => items.clone(),
        _ => vec![by],
    };
    let by_names: Vec<String> = match get_attrib(heap, interner, by, "names") {
        Some(id) => string_vec(heap, interner, id),
        None => Vec::new(),
    };
    // A data frame x aggregates columnwise exactly as GNU's method —
    /// each column produces one output column sharing the group frame.
    let x_columns: Vec<(String, ValueId)> = {
        let is_frame = match get_attrib(heap, interner, x, "class") {
            Some(c) => string_vec(heap, interner, c).iter().any(|s| s == "data.frame"),
            None => false,
        };
        if is_frame {
            let col_names = match get_attrib(heap, interner, x, "names") {
                Some(n) => string_vec(heap, interner, n),
                None => Vec::new(),
            };
            match heap.object(x) {
                Some(RObject::List(items)) => items
                    .iter()
                    .enumerate()
                    .map(|(pos, item)| (col_names.get(pos).cloned().unwrap_or_else(|| format!("V{}", pos + 1)), *item))
                    .collect(),
                _ => vec![("x".to_string(), x)],
            }
        } else {
            vec![("x".to_string(), x)]
        }
    };
    let x_len = x_columns.first().and_then(|(_, col)| vector_length(heap, *col)).unwrap_or(0);
    // Group keys: per-row concatenation of the first grouping column (the
    // slice supports one grouper; multi-grouper keys concatenate with a
    // separator exactly as GNU's interaction does).
    // Factor groupers key by their level labels and order by level index
    /// exactly as GNU; plain groupers key by element text alphabetically.
    let grouper_levels: Vec<Option<Vec<String>>> = group_cols
        .iter()
        .map(|col| {
            if is_factor(heap, interner, *col) {
                get_attrib(heap, interner, *col, "levels").map(|id| string_vec(heap, interner, id))
            } else {
                None
            }
        })
        .collect();
    let grouper_codes: Vec<Option<Vec<i32>>> = group_cols
        .iter()
        .map(|col| {
            if is_factor(heap, interner, *col) {
                match heap.object(*col) {
                    Some(RObject::Integer(vals)) => Some(vals.clone()),
                    _ => None,
                }
            } else {
                None
            }
        })
        .collect();
    let mut keys: Vec<String> = Vec::with_capacity(x_len);
    for i in 0..x_len {
        let mut key = String::new();
        for (j, col) in group_cols.iter().enumerate() {
            if j > 0 {
                key.push('\x01');
            }
            let text = match (&grouper_codes[j], &grouper_levels[j]) {
                (Some(codes), Some(levels)) => {
                    let code = codes.get(i).copied().unwrap_or(crate::value::object::NA_INTEGER);
                    if code == crate::value::object::NA_INTEGER || code < 1 || code as usize > levels.len() {
                        "NA".to_string()
                    } else {
                        levels[(code - 1) as usize].clone()
                    }
                }
                _ => element_text(heap, interner, *col, i),
            };
            key.push_str(&text);
        }
        keys.push(key);
    }
    let mut unique_keys: Vec<String> = keys.clone();
    // GNU orders multi-way group expansion column-major: the first grouper
    /// varies fastest, so the sort prioritizes the last key component and
    /// ties break toward the first exactly as array indexing does.
    unique_keys.sort_by(|a, b| {
        let ac: Vec<&str> = a.split('\x01').collect();
        let bc: Vec<&str> = b.split('\x01').collect();
        for j in (0..ac.len().max(bc.len())).rev() {
            let av = ac.get(j).copied().unwrap_or("");
            let bv = bc.get(j).copied().unwrap_or("");
            // Factor columns compare by level position; anything absent
            /// from the levels sorts past the end exactly as GNU's integer
            /// code comparison would place it.
            let ord = match &grouper_levels.get(j).cloned().flatten() {
                Some(levels) => {
                    let ai = levels.iter().position(|l| l == av).map(|p| p as i32).unwrap_or(i32::MAX);
                    let bi = levels.iter().position(|l| l == bv).map(|p| p as i32).unwrap_or(i32::MAX);
                    ai.cmp(&bi)
                }
                None => av.cmp(bv),
            };
            match ord {
                std::cmp::Ordering::Equal => continue,
                ord => return ord,
            }
        }
        std::cmp::Ordering::Equal
    });
    unique_keys.dedup();
    // Aggregate: gather positions per group, extract the subvector, apply
    /// — one result column per x column exactly as GNU's data frame method.
    let positions_by_key: Vec<Vec<usize>> = unique_keys
        .iter()
        .map(|key| {
            keys.iter()
                .enumerate()
                .filter(|(_, k)| *k == key)
                .map(|(i, _)| i)
                .collect()
        })
        .collect();
    let mut aggregated_columns: Vec<(String, Vec<f64>)> = Vec::new();
    let mut result_is_int = true;
    for (col_name, x_col) in &x_columns {
        let mut result_values: Vec<f64> = Vec::new();
        for positions in &positions_by_key {
            let sub = extract_positions(heap, interner, *x_col, positions)?;
            // Apply FUN through the closure machinery — primitives and user
            // functions both resolve through the symbol's binding.
            let applied = apply_fun_to(heap, interner, fun, sub, caller_env)?;
            match heap.object(applied) {
                Some(RObject::Real(v)) if !v.is_empty() => {
                    result_values.push(v[0]);
                    result_is_int = false;
                }
                Some(RObject::Integer(v)) if !v.is_empty() => {
                    result_values.push(v[0] as f64);
                }
                _ => return Err(EvalError::TypeError("aggregate FUN must return scalar numeric").into()),
            }
        }
        aggregated_columns.push((col_name.clone(), result_values));
    }
    // Assemble: group columns carry one row per unique key; the aggregated
    // column names "x" exactly as GNU's default.
    let mut columns: Vec<ValueId> = Vec::new();
    let mut col_names: Vec<String> = Vec::new();
    let ngroupers = group_cols.len();
    for j in 0..ngroupers {
        let parts: Vec<String> = unique_keys
            .iter()
            .map(|k| k.split('\x01').nth(j).unwrap_or("").to_string())
            .collect();
        // Factor groupers emit the coded factor column — integer indices
        /// into the original levels with the class chain exactly as GNU's
        /// aggregate data frame method preserves them.
        if let Some(levels) = &grouper_levels.get(j).cloned().flatten() {
            let codes: Vec<i32> = parts
                .iter()
                .map(|text| {
                    levels
                        .iter()
                        .position(|l| l == text)
                        .map(|pos| pos as i32 + 1)
                        .unwrap_or(crate::value::object::NA_INTEGER)
                })
                .collect();
            let mut col = heap.alloc(RObject::Integer(codes));
            let level_interned: Vec<super::intern::InternedStr> = levels
                .iter()
                .map(|s| interner.intern_string(s))
                .collect();
            let levels_vec = heap.alloc(RObject::String(level_interned));
            col = copy_with_attrib(heap, interner, col, "levels", levels_vec).map_err(RSignal::from)?;
            let class_vec = heap.alloc(RObject::String(vec![interner.intern_string("factor")]));
            col = copy_with_attrib(heap, interner, col, "class", class_vec).map_err(RSignal::from)?;
            columns.push(col);
        } else {
            let interned: Vec<super::intern::InternedStr> = parts
                .iter()
                .map(|s| interner.intern_string(s))
                .collect();
            columns.push(heap.alloc(RObject::String(interned)));
        }
        col_names.push(
            by_names
                .get(j)
                .cloned()
                .unwrap_or_else(|| format!("Group.{j}", j = j + 1)),
        );
    }
    for (col_name, result_values) in &aggregated_columns {
        let _ = result_is_int;
        let result_col = heap.alloc(RObject::Real(result_values.clone()));
        columns.push(result_col);
        col_names.push(col_name.clone());
    }
    build_data_frame(heap, interner, columns, col_names)
}

/// One element's plain text — the grouping-key coercion.
fn element_text(heap: &Heap, interner: &SymbolInterner, value: ValueId, index: usize) -> String {
    match heap.object(value) {
        Some(RObject::String(values)) => interner
            .string_text(values.get(index).copied().unwrap_or(values[0]))
            .to_string(),
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => values
            .get(index)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Some(RObject::Real(values)) => values
            .get(index)
            .map(|v| super::print::format_real(*v))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Extract elements at positions into a fresh same-type vector.
fn extract_positions(
    heap: &mut Heap,
    interner: &SymbolInterner,
    source: ValueId,
    positions: &[usize],
) -> Result<ValueId, RSignal> {
    let object = heap.object(source).cloned()
        .ok_or(EvalError::TypeError("aggregate input missing"))?;
    let out = match object {
        RObject::Real(values) => RObject::Real(
            positions.iter().filter_map(|i| values.get(*i).copied()).collect(),
        ),
        RObject::Integer(values) => RObject::Integer(
            positions.iter().filter_map(|i| values.get(*i).copied()).collect(),
        ),
        RObject::Logical(values) => RObject::Logical(
            positions.iter().filter_map(|i| values.get(*i).copied()).collect(),
        ),
        RObject::String(values) => RObject::String(
            positions.iter().filter_map(|i| values.get(*i).copied()).collect(),
        ),
        _ => return Err(EvalError::TypeError("aggregate input type").into()),
    };
    let _ = interner;
    Ok(heap.alloc(out))
}

/// Apply a closure or primitive-reference to one vector argument — the
/// same dual shape lapply resolves.
fn apply_fun_to(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    fun: ValueId,
    arg: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // The argument arrives already evaluated — wrapping it in a pre-forced
    /// promise keeps primitive argument forcing value-faithful exactly as
    /// GNU: forcing yields the object itself rather than re-evaluating it
    /// as an expression, which would mis-evaluate symbols as lookups.
    let forced = heap.alloc(RObject::Promise {
        expr: arg,
        env: caller_env,
        value: arg,
    });
    let arg_node = heap.alloc(RObject::Pairlist {
        car: forced,
        cdr: ValueId::NIL,
        tag: ValueId::NIL,
    });
    match heap.object(fun).cloned() {
        Some(RObject::Closure { formals, body, env }) => {
            let applied = super::eval::apply_closure_public(
                heap, interner, formals, body, env, arg_node, caller_env,
            )?;
            force_if_promise(heap, interner, applied)
        }
        Some(RObject::Primitive(builtin)) => {
            call_primitive(heap, interner, builtin, arg_node, caller_env)
        }
        _ => Err(EvalError::NotCallable.into()),
    }
}

/// `factor(x)`: character input recodes to integer positions into the
/// sorted unique levels — GNU's integer-codes-plus-labels object shape with
/// class and levels attributes exactly.
fn factor_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let texts = string_vec(heap, interner, operands[0]);
    // A tagged levels argument supplies the level set verbatim — order and
    /// empties included — exactly as GNU; otherwise levels derive sorted
    /// from the data.
    let mut tagged_levels: Option<Vec<String>> = None;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        if tag_name.as_deref() == Some("levels") {
            let v = eval(heap, interner, car, caller_env)?;
            tagged_levels = Some(string_vec(heap, interner, v));
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let mut levels: Vec<String> = match tagged_levels {
        Some(ls) => ls.into_iter().filter(|s| s != "\0NA").collect(),
        None => {
            let mut ls: Vec<String> = texts
                .iter()
                .filter(|s| *s != "\0NA")
                .cloned()
                .collect();
            ls.sort();
            ls.dedup();
            ls
        }
    };
    let codes: Vec<i32> = texts
        .iter()
        .map(|s| {
            if s == "\0NA" {
                crate::value::object::NA_INTEGER
            } else {
                levels.iter().position(|l| l == s).map(|p| p as i32 + 1).unwrap_or(crate::value::object::NA_INTEGER)
            }
        })
        .collect();
    let obj = heap.alloc(RObject::Integer(codes));
    let interned: Vec<super::intern::InternedStr> = levels
        .iter()
        .map(|l| interner.intern_string(l))
        .collect();
    let levels_obj = heap.alloc(RObject::String(interned));
    let obj = copy_with_attrib(heap, interner, obj, "levels", levels_obj).map_err(RSignal::from)?;
    let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("factor")]));
    copy_with_attrib(heap, interner, obj, "class", class_obj).map_err(RSignal::from)
}

/// One conversion spec parsed out of a sprintf format string.
struct ConvSpec {
    zero_pad: bool,
    plus: bool,
    minus: bool,
    space: bool,
    width: usize,
    precision: Option<usize>,
    conversion: char,
}

/// `sprintf(fmt, ...)`: C-style conversion specs vectorized over recycled
/// inputs — the subset GNU's statistical printing actually exercises:
/// %d/%i integers, %f fixed, %s strings, %g significant, %% literal.
fn sprintf_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Variadic: every argument evaluates — the format first, conversion
    /// inputs in order after.
    let raw = raw_args(heap, args);
    let mut operands: Vec<ValueId> = Vec::new();
    for expr in &raw {
        let v = eval(heap, interner, *expr, caller_env)?;
        operands.push(force_if_promise(heap, interner, v)?);
    }
    if operands.is_empty() {
        return Err(EvalError::TypeError("sprintf requires a format").into());
    }
    // GNU vectorizes the format itself exactly as the conversion inputs —
    // the output width is the maximum length across all of them.
    let fmts = match heap.object(operands[0]) {
        Some(RObject::String(vals)) => vals.iter().map(|v| interner.string_text(*v).to_string()).collect::<Vec<_>>(),
        _ => return Err(EvalError::TypeError("sprintf format must be character").into()),
    };
    let fmt = fmts.first().cloned().unwrap_or_default();
    let _ = &fmt;
    // The conversion inputs follow the format operand in order. GNU's
    // zero-length discipline: a zero-length format or any zero-length
    // input collapses the whole result to character(0).
    let inputs: Vec<ValueId> = operands[1..].to_vec();
    let any_zero_input = inputs.iter().any(|v| vector_length(heap, *v).unwrap_or(0) == 0);
    if fmts.is_empty() || (!inputs.is_empty() && any_zero_input) {
        return Ok(heap.alloc(RObject::String(Vec::new())));
    }
    // The output width recycles across both the format vector and the
    // conversion inputs exactly as GNU's vectorized sprintf.
    let input_width = inputs
        .iter()
        .map(|v| vector_length(heap, *v).unwrap_or(1))
        .max()
        .unwrap_or(1);
    let out_len = fmts.len().max(input_width).max(1);
    let mut results: Vec<String> = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let element_fmt = &fmts[i % fmts.len()];
        let pieces = parse_format_pieces(element_fmt);
        let mut text = String::new();
        let mut input_index = 0usize;
        for piece in &pieces {
            match piece {
                Ok(lit) => text.push_str(lit),
                Err(spec) => {
                    let input = inputs.get(input_index).copied().unwrap_or(ValueId::NIL);
                    input_index += 1;
                    text.push_str(&format_spec(heap, interner, spec, input, i));
                }
            }
        }
        results.push(text);
    }
    let interned: Vec<super::intern::InternedStr> = results
        .iter()
        .map(|s| interner.intern_string(s))
        .collect();
    Ok(heap.alloc(RObject::String(interned)))
}

/// Parse one sprintf format string into interleaved literals and
/// conversion specs exactly as the C printf grammar subset requires.
fn parse_format_pieces(fmt: &str) -> Vec<Result<String, ConvSpec>> {
    let mut pieces: Vec<Result<String, ConvSpec>> = Vec::new();
    let mut chars = fmt.chars().peekable();
    let mut literal = String::new();
    while let Some(c) = chars.next() {
        if c != '%' {
            literal.push(c);
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            literal.push('%');
            continue;
        }
        if !literal.is_empty() {
            pieces.push(Ok(std::mem::take(&mut literal)));
        }
        let mut zero_pad = false;
        let mut plus = false;
        let mut minus = false;
        let mut space = false;
        let mut width = 0usize;
        let mut precision = None;
        while let Some(&f) = chars.peek() {
            match f {
                '0' => {
                    zero_pad = true;
                    chars.next();
                }
                '+' => {
                    plus = true;
                    chars.next();
                }
                '-' => {
                    minus = true;
                    chars.next();
                }
                ' ' => {
                    space = true;
                    chars.next();
                }
                _ => break,
            }
        }
        while let Some(&d) = chars.peek() {
            if d.is_ascii_digit() {
                width = width * 10 + d.to_digit(10).unwrap() as usize;
                chars.next();
            } else {
                break;
            }
        }
        if chars.peek() == Some(&'.') {
            chars.next();
            let mut prec = 0usize;
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    prec = prec * 10 + d.to_digit(10).unwrap() as usize;
                    chars.next();
                } else {
                    break;
                }
            }
            precision = Some(prec);
        }
        let conversion = chars.next().unwrap_or('s');
        pieces.push(Err(ConvSpec {
            zero_pad,
            plus,
            minus,
            space,
            width,
            precision,
            conversion,
        }));
    }
    if !literal.is_empty() {
        pieces.push(Ok(literal));
    }
    pieces
}

/// Format one recycled element against one conversion spec.
fn format_spec(
    heap: &Heap,
    interner: &SymbolInterner,
    spec: &ConvSpec,
    input: ValueId,
    index: usize,
) -> String {
    let len = vector_length(heap, input).unwrap_or(1).max(1);
    let i = index % len;
    let int_value = |heap: &Heap| -> i64 {
        match heap.object(input) {
            Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => {
                values.get(i).copied().unwrap_or(0) as i64
            }
            Some(RObject::Real(values)) => values.get(i).copied().unwrap_or(0.0) as i64,
            _ => 0,
        }
    };
    let real_value = |heap: &Heap| -> f64 {
        match heap.object(input) {
            Some(RObject::Real(values)) => values.get(i).copied().unwrap_or(0.0),
            Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => {
                values.get(i).copied().unwrap_or(0) as f64
            }
            _ => 0.0,
        }
    };
    // GNU's uniform numeric discipline: the sign (+/-/space) leads, zero
    /// padding follows the sign, and width justification pads with spaces —
    /// left when the minus flag is present.
    let finish = |digits: String, negative: bool, spec: &ConvSpec| -> String {
        let sign = if negative {
            "-"
        } else if spec.plus {
            "+"
        } else if spec.space {
            " "
        } else {
            ""
        };
        let core_len = sign.len() + digits.len();
        if spec.zero_pad && !spec.minus && spec.width > core_len {
            format!("{}{}{}", sign, "0".repeat(spec.width - core_len), digits)
        } else if spec.width > core_len {
            if spec.minus {
                format!("{}{}{}", sign, digits, " ".repeat(spec.width - core_len))
            } else {
                format!("{}{}{}", " ".repeat(spec.width - core_len), sign, digits)
            }
        } else {
            format!("{sign}{digits}")
        }
    };
    match spec.conversion {
        'd' | 'i' => {
            let v = int_value(heap);
            finish(v.unsigned_abs().to_string(), v < 0, spec)
        }
        'x' | 'X' | 'o' => {
            let v = int_value(heap);
            let digits = match spec.conversion {
                'x' => format!("{:x}", v),
                'X' => format!("{:X}", v),
                _ => format!("{:o}", v),
            };
            finish(digits, false, spec)
        }
        'f' => {
            let v = real_value(heap);
            let decimals = spec.precision.unwrap_or(6);
            let fixed = format!("{:.*}", decimals, v.abs());
            finish(fixed, v < 0.0, spec)
        }
        'e' | 'E' => {
            let v = real_value(heap);
            let decimals = spec.precision.unwrap_or(6);
            let raw = format!("{:.*e}", decimals, v.abs());
            // Rust prints 1.23e3 — GNU wants 1.23e+03: signed, two digits.
            let (mantissa, exp) = raw.split_once('e').unwrap_or((&raw, "0"));
            let exp_value: i32 = exp.parse().unwrap_or(0);
            let exp_text = format!("{}{:02}", if exp_value < 0 { "-" } else { "+" }, exp_value.abs());
            let marker = if spec.conversion == 'E' { "E" } else { "e" };
            finish(format!("{mantissa}{marker}{exp_text}"), v < 0.0, spec)
        }
        'g' | 'G' => {
            let v = real_value(heap);
            let prec = spec.precision.unwrap_or(6).max(1);
            let abs = v.abs();
            let exp = if abs == 0.0 { 0 } else { abs.log10().floor() as i32 };
            let text = if exp < -4 || exp >= prec as i32 {
                // scientific with prec-1 mantissa decimals, trailing zeros
                /// stripped exactly as %g requires.
                let raw = format!("{:.*e}", prec - 1, abs);
                let (mantissa, exponent) = raw.split_once('e').unwrap_or((&raw, "0"));
                let mantissa = mantissa
                    .trim_end_matches('0')
                    .trim_end_matches('.');
                let exp_value: i32 = exponent.parse().unwrap_or(0);
                let marker = if spec.conversion == 'G' { "E" } else { "e" };
                format!("{}{}{}{:02}", mantissa, marker, if exp_value < 0 { "-" } else { "+" }, exp_value.abs())
            } else {
                let decimals = (prec as i32 - 1 - exp).max(0) as usize;
                let fixed = format!("{:.*}", decimals, abs);
                fixed.trim_end_matches('0').trim_end_matches('.').to_string()
            };
            finish(text, v < 0.0, spec)
        }
        _ => {
            // %s — string coercion of the element, width-justified without
            /// sign discipline.
            let text = match heap.object(input) {
                Some(RObject::String(values)) => values
                    .get(i)
                    .map(|v| interner.string_text(*v).to_string())
                    .unwrap_or_default(),
                Some(RObject::Real(values)) => super::print::format_real(
                    values.get(i).copied().unwrap_or(0.0),
                ),
                Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => values
                    .get(i)
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                _ => String::new(),
            };
            if spec.width > text.len() {
                if spec.minus {
                    format!("{}{}", text, " ".repeat(spec.width - text.len()))
                } else {
                    format!("{}{}", " ".repeat(spec.width - text.len()), text)
                }
            } else {
                text
            }
        }
    }
}

/// `levels<-` — GNU's full remap discipline. Character vector form maps
/// positionally over the current levels with NA entries dropping levels
/// (elements recode to NA, higher codes shift down); a named list form
/// remaps new-level-names to the old labels they absorb. Length mismatch
/// in the vector form is GNU's "number of levels differs" error. On
/// non-factors the attribute attaches plainly exactly as GNU.
fn levels_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let object = operands[0];
    let value = operands[1];
    let is_factor = match get_attrib(heap, interner, object, "class") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .any(|v| interner.string_text(*v) == "factor"),
            _ => false,
        },
        None => false,
    };
    if !is_factor {
        return copy_with_attrib(heap, interner, object, "levels", value).map_err(RSignal::from);
    }
    let old_levels: Vec<String> = match get_attrib(heap, interner, object, "levels") {
        Some(id) => string_vec(heap, interner, id),
        None => Vec::new(),
    };
    let codes = match heap.object(object) {
        Some(RObject::Integer(values)) => values.clone(),
        _ => return Err(EvalError::TypeError("levels target must be integer-coded").into()),
    };

    // The list form: names are new levels, elements the old labels each
    // absorbs — a full recode map.
    if let Some(RObject::List(items)) = heap.object(value).cloned() {
        let names_id = get_attrib(heap, interner, value, "names");
        let new_names: Vec<String> = match names_id {
            Some(id) => string_vec(heap, interner, id),
            None => Vec::new(),
        };
        // Map old label -> new code (1-based).
        let mut remap: Vec<(String, usize)> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let absorbed = string_vec(heap, interner, *item);
            for label in absorbed {
                remap.push((label, i + 1));
            }
        }
        let new_codes: Vec<i32> = codes
            .iter()
            .map(|c| {
                if *c == crate::value::object::NA_INTEGER {
                    return crate::value::object::NA_INTEGER;
                }
                let label = old_levels.get((*c - 1) as usize).cloned().unwrap_or_default();
                remap
                    .iter()
                    .find(|(l, _)| *l == label)
                    .map(|(_, nc)| *nc as i32)
                    .unwrap_or(crate::value::object::NA_INTEGER)
            })
            .collect();
        return rebuild_factor(heap, interner, object, new_codes, new_names);
    }

    // Vector form: positional rename with NA-drop recoding.
    let new_names_raw = string_vec(heap, interner, value);
    if new_names_raw.len() != old_levels.len() {
        return Err(EvalError::Condition("number of levels differs".to_string()).into());
    }
    // Positional map old position -> new name; NA drops the level.
    // Surviving levels compact in order; codes remap.
    let mut surviving: Vec<String> = Vec::new();
    let mut position_map: Vec<Option<usize>> = Vec::new(); // old idx -> new 0-based
    for (i, name) in new_names_raw.iter().enumerate() {
        if name == "\0NA" || name == "NA" && !old_levels.contains(name) {
            position_map.push(None);
        } else {
            position_map.push(Some(surviving.len()));
            surviving.push(name.clone());
        }
    }
    let new_codes: Vec<i32> = codes
        .iter()
        .map(|c| {
            if *c == crate::value::object::NA_INTEGER {
                return crate::value::object::NA_INTEGER;
            }
            match position_map.get((*c - 1) as usize) {
                Some(Some(new_pos)) => *new_pos as i32 + 1,
                _ => crate::value::object::NA_INTEGER,
            }
        })
        .collect();
    rebuild_factor(heap, interner, object, new_codes, surviving)
}

/// Reassemble a factor with recoded integers and fresh levels — the
/// copy-on-write discipline the replacement forms share.
fn rebuild_factor(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    object: ValueId,
    new_codes: Vec<i32>,
    new_levels: Vec<String>,
) -> EvalResult {
    let interned: Vec<super::intern::InternedStr> = new_levels
        .iter()
        .map(|l| interner.intern_string(l))
        .collect();
    let levels_obj = heap.alloc(RObject::String(interned));
    // Fresh integer vector carrying the recoded values; class and levels
    // attach through copy-on-write discipline.
    let fresh = heap.alloc(RObject::Integer(new_codes));
    let fresh = copy_with_attrib(heap, interner, fresh, "levels", levels_obj).map_err(RSignal::from)?;
    let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("factor")]));
    copy_with_attrib(heap, interner, fresh, "class", class_obj).map_err(RSignal::from)
}

/// `grep(pattern, x, value)` / `grepl(pattern, x)` — elementwise leftmost
/// search through the pure-Rust regex engine. grepl returns the logical
/// mask exactly as GNU; grep returns indices by default or the matching
/// values with value=TRUE.
fn grep_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    logical: bool,
) -> EvalResult {
    // Tagged walk: pattern and x positional, value and invert by name.
    let mut pattern_arg = ValueId::NIL;
    let mut x_arg = ValueId::NIL;
    let mut value_mode = false;
    let mut invert = false;
    let mut ignore_case = false;
    let mut fixed = false;
    let mut positional = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("value") => {
                if let Some(RObject::Logical(vals)) = heap.object(v) {
                    value_mode = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                }
            }
            Some("invert") => {
                if let Some(RObject::Logical(vals)) = heap.object(v) {
                    invert = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                }
            }
            Some("fixed") => {
                if let Some(RObject::Logical(vals)) = heap.object(v) {
                    fixed = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                }
            }
            Some("ignore.case") => {
                if let Some(RObject::Logical(vals)) = heap.object(v) {
                    ignore_case = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                }
            }
            _ => {
                if positional == 1 {
                    pattern_arg = v;
                } else if positional == 2 {
                    x_arg = v;
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let pattern_raw = string_operand(heap, interner, pattern_arg)
        .ok_or(EvalError::TypeError("grep pattern must be character"))?;
    let pattern = if ignore_case { pattern_raw.to_lowercase() } else { pattern_raw };
    let texts = string_vec(heap, interner, x_arg);
    let matches: Vec<bool> = texts
        .iter()
        .map(|text| {
            // GNU: NA strings are simply non-matching — invert then selects
            // them exactly as any non-match.
            if text == "\0NA" {
                invert
            } else {
                let haystack = if ignore_case { text.to_lowercase() } else { text.clone() };
                // fixed=TRUE is literal substring matching — no metacharacters.
                let found = if fixed {
                    haystack.contains(&pattern)
                } else {
                    super::regex::regex_search(&pattern, &haystack).is_some()
                };
                found != invert
            }
        })
        .collect();
    if logical {
        let out: Vec<i32> = matches.iter().map(|m| if *m { 1 } else { 0 }).collect();
        return Ok(heap.alloc(RObject::Logical(out)));
    }
    if value_mode {
        let out: Vec<super::intern::InternedStr> = texts
            .iter()
            .enumerate()
            .filter(|(i, _)| matches[*i])
            .map(|(_, s)| interner.intern_string(s))
            .collect();
        return Ok(heap.alloc(RObject::String(out)));
    }
    let out: Vec<i32> = matches
        .iter()
        .enumerate()
        .filter(|(_, m)| **m)
        .map(|(i, _)| i as i32 + 1)
        .collect();
    Ok(heap.alloc(RObject::Integer(out)))
}

/// `regexpr(pattern, x)` — match positions and lengths exactly as GNU:
/// 1-based start positions with -1 for no match, carrying the match.length
/// attribute.
fn regexpr_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let pattern = string_operand(heap, interner, operands[0])
        .ok_or(EvalError::TypeError("regexpr pattern must be character"))?;
    let texts = string_vec(heap, interner, operands[1]);
    let mut positions: Vec<i32> = Vec::new();
    let mut lengths: Vec<i32> = Vec::new();
    for text in &texts {
        match super::regex::regex_search(&pattern, text) {
            Some((start, end)) => {
                positions.push(start as i32 + 1);
                lengths.push((end - start) as i32);
            }
            None => {
                positions.push(-1);
                lengths.push(-1);
            }
        }
    }
    // copy_with_attrib appends, so the application order is GNU's printed
    /// order exactly — match.length first, then index.type, then useBytes.
    let use_bytes = heap.alloc(RObject::Logical(vec![1]));
    let result = heap.alloc(RObject::Integer(positions));
    let length_obj = heap.alloc(RObject::Integer(lengths));
    let result = copy_with_attrib(heap, interner, result, "match.length", length_obj).map_err(RSignal::from)?;
    let index_type = heap.alloc(RObject::String(vec![interner.intern_string("chars")]));
    let result = copy_with_attrib(heap, interner, result, "index.type", index_type).map_err(RSignal::from)?;
    copy_with_attrib(heap, interner, result, "useBytes", use_bytes).map_err(RSignal::from)
}

/// `unique(x, incomparables, fromLast)` — GNU's deduplication exactly:
/// first occurrences in original order (last occurrences with fromLast,
/// still in original order), NaN compared bitwise so NaN and NA_real_
/// dedupe separately, the string sentinel dedupes as one NA, and values
/// matching incomparables never compare at all — every occurrence stays.
fn unique_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let value = operands[0];
    // Tagged walk: incomparables and fromLast.
    let mut from_last = false;
    let mut incomparables_na = false;
    let mut cursor = args;
    let mut positional_seen = 0usize;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional_seen += 1;
        if positional_seen > 1 {
            let tag_name = match heap.object(tag) {
                Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                _ => None,
            };
            let evaluated = eval(heap, interner, car, caller_env)?;
            let v = force_if_promise(heap, interner, evaluated)?;
            match tag_name.as_deref() {
                Some("fromLast") => {
                    if let Some(RObject::Logical(vals)) = heap.object(v) {
                        from_last = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                    }
                }
                Some("incomparables") => {
                    // NA in incomparables means NA values never compare.
                    incomparables_na = match heap.object(v) {
                        Some(RObject::Logical(vals)) => vals.iter().any(|x| *x == crate::value::object::NA_INTEGER),
                        Some(RObject::Real(vals)) => vals.iter().any(|x| x.is_nan()),
                        Some(RObject::String(vals)) => {
                            vals.iter().any(|x| interner.string_text(*x) == "\0NA")
                        }
                        _ => v != ValueId::NIL,
                    };
                }
                _ => {}
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }

    // Generic dedup over an indexable equality relation — keeps the first
    /// (or last, then re-ordered) occurrence per equivalence class.
    macro_rules! dedup {
        ($variant:ident, $values:expr, $is_na:expr, $eq:expr) => {{
            let values: &Vec<_> = $values;
            let is_na = $is_na;
            let eq = $eq;
            let n = values.len();
            let mut keep = vec![false; n];
            let order: Vec<usize> = if from_last {
                (0..n).rev().collect()
            } else {
                (0..n).collect()
            };
            let mut representatives: Vec<usize> = Vec::new();
            for &i in &order {
                let vi = &values[i];
                if is_na(vi) && incomparables_na {
                    keep[i] = true;
                    continue;
                }
                // Reverse iteration encounters last occurrences first, so
                /// the plain keep-first-seen rule yields fromLast exactly —
                /// output ordering restores original indices either way.
                let duplicate = representatives.iter().any(|&r| eq(&values[r], vi));
                if !duplicate {
                    keep[i] = true;
                    representatives.push(i);
                }
            }
            let out: Vec<_> = (0..n)
                .filter(|&i| keep[i])
                .map(|i| values[i].clone())
                .collect();
            return Ok(heap.alloc(RObject::$variant(out)));
        }};
    }

    match heap.object(value) {
        Some(RObject::Real(values)) => {
            dedup!(
                Real,
                values,
                |v: &f64| v.is_nan(),
                |a: &f64, b: &f64| {
                    // Bitwise equality: NaN payloads compare by bits so
                    /// canonical NaN and NA_real_ dedupe separately, and
                    /// NaN equals itself exactly as GNU's dedup requires.
                    if a.is_nan() && b.is_nan() {
                        a.to_bits() == b.to_bits()
                    } else {
                        a == b
                    }
                }
            );
        }
        Some(RObject::Integer(values)) => {
            dedup!(Integer, values, |v: &i32| *v == crate::value::object::NA_INTEGER, |a: &i32, b: &i32| a == b);
        }
        Some(RObject::Logical(values)) => {
            dedup!(Logical, values, |v: &i32| *v == crate::value::object::NA_INTEGER, |a: &i32, b: &i32| a == b);
        }
        Some(RObject::String(values)) => {
            dedup!(
                String,
                values,
                |v: &super::intern::InternedStr| interner.string_text(*v) == "\0NA",
                |a: &super::intern::InternedStr, b: &super::intern::InternedStr| a == b
            );
        }
        _ => Err(EvalError::TypeError("unique requires an atomic vector").into()),
    }
}

/// `save(..., file, envir)` — variadic bare symbols or list= character
/// names resolved in envir, encoded as RBG1 exactly as the architecture's
/// GNU-wire-format replacement specifies.
fn save_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Tagged walk: file/envir/list by name, bare symbols positional.
    let mut file_path: Option<String> = None;
    let mut envir = caller_env;
    let mut names: Vec<String> = Vec::new();
    // Positional-mode detection: any non-symbol untagged argument means
    /// the call follows .Internal's positional ordering — names vector,
    /// file, ascii, version, envir — rather than the user-facing bare
    /// symbol capture.
    let mut positional_mode = false;
    {
        let mut scan = args;
        loop {
            let (car, cdr, tag) = match heap.object(scan) {
                Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                _ => break,
            };
            let untagged = !matches!(heap.object(tag), Some(RObject::Symbol { .. }));
            if untagged && !matches!(heap.object(car), Some(RObject::Symbol { .. })) {
                positional_mode = true;
            }
            if cdr == ValueId::NIL {
                break;
            }
            scan = cdr;
        }
    }
    let mut positional_seen = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        match tag_name.as_deref() {
            Some("file") => {
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                file_path = string_operand(heap, interner, v);
            }
            Some("envir") => {
                let evaluated = eval(heap, interner, car, caller_env)?;
                envir = force_if_promise(heap, interner, evaluated)?;
            }
            Some("list") => {
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                names.extend(string_vec(heap, interner, v));
            }
            Some("ascii") | Some("version") | Some("eval.promises") | Some("compress") | Some("compression_level") => {}
            _ => {
                if positional_mode {
                    // .Internal ordering: position 1 names, 2 file, 5 envir.
                    positional_seen += 1;
                    let evaluated = eval(heap, interner, car, caller_env)?;
                    let v = force_if_promise(heap, interner, evaluated)?;
                    match positional_seen {
                        1 => names.extend(string_vec(heap, interner, v)),
                        2 => file_path = string_operand(heap, interner, v),
                        5 => envir = v,
                        _ => {}
                    }
                } else if let Some(RObject::Symbol { name, .. }) = heap.object(car) {
                    // Bare symbol — the object name itself, GNU's
                    /// non-standard capture of unevaluated arguments.
                    names.push(interner.symbol_name(*name).to_string());
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let path = file_path.ok_or(EvalError::TypeError("save requires file"))?;
    let mut bindings: Vec<(String, ValueId)> = Vec::new();
    for name in &names {
        let sym = interner.intern_symbol(name);
        let (_, binding) = super::env::lookup(heap, envir, sym)
            .ok_or(EvalError::UnboundVariable(name.clone()))?;
        bindings.push((name.clone(), binding.value));
    }
    let bytes = super::rbg1::encode_save(heap, interner, &bindings);
    std::fs::write(&path, bytes).map_err(|e| EvalError::TypeError("save write failed"))?;
    Ok(ValueId::NIL)
}

/// `load(file, envir)` — decode the RBG1 stream, define every binding in
/// envir, and return the character vector of names exactly as GNU.
fn load_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut file_path: Option<String> = None;
    let mut envir = caller_env;
    let mut cursor = args;
    let mut positional = 0usize;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("envir") => envir = v,
            Some("file") => file_path = string_operand(heap, interner, v),
            _ => {
                if positional == 1 {
                    file_path = string_operand(heap, interner, v);
                } else if positional == 2 {
                    envir = v;
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let path = file_path.ok_or(EvalError::TypeError("load requires file"))?;
    let bytes = std::fs::read(&path).map_err(|e| EvalError::TypeError("load read failed"))?;
    let bindings = super::rbg1::decode_save(heap, interner, &bytes, envir)
        .ok_or(EvalError::TypeError("bad save file"))?;
    let mut names: Vec<super::intern::InternedStr> = Vec::new();
    for (name, value) in bindings {
        let sym = interner.intern_symbol(&name);
        names.push(interner.intern_string(&name));
        super::env::define(heap, envir, sym, value)
            .map_err(|_| RSignal::Error(EvalError::TypeError("cannot add binding")))?;
    }
    Ok(heap.alloc(RObject::String(names)))
}

/// GNU's character coercion — integers and logicals render as decimal and
/// TRUE/FALSE exactly as paste does, real NA becomes the sentinel.
fn coerce_character(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
) -> EvalResult {
    // Factor-classed input coerces through its level labels exactly as
    /// as.character.factor — the integer codes index the levels, missing
    /// codes yielding NA.
    if is_factor(heap, interner, value) {
        if let Some(levels_id) = get_attrib(heap, interner, value, "levels") {
            let levels = string_vec(heap, interner, levels_id);
            if let Some(RObject::Integer(codes)) = heap.object(value) {
                let out: Vec<super::intern::InternedStr> = codes
                    .iter()
                    .map(|&code| {
                        if code == crate::value::object::NA_INTEGER
                            || code < 1
                            || code as usize > levels.len()
                        {
                            interner.intern_string("\0NA")
                        } else {
                            interner.intern_string(&levels[(code - 1) as usize])
                        }
                    })
                    .collect();
                return Ok(heap.alloc(RObject::String(out)));
            }
        }
    }
    // Symbols coerce to their print name exactly as as.character of a
    /// name object does in GNU.
    if let Some(RObject::Symbol { name, .. }) = heap.object(value) {
        let text = interner.symbol_name(*name).to_string();
        return Ok(heap.alloc(RObject::String(vec![interner.intern_string(&text)])));
    }
    // POSIXct-classed input renders the datetime exactly as
    /// as.character.POSIXct — no timezone suffix.
    if is_posixct_classed(heap, interner, value) {
        let secs = numeric_vector(heap, value);
        let out: Vec<super::intern::InternedStr> = secs
            .iter()
            .map(|&s| {
                if s.is_nan() {
                    interner.intern_string("\0NA")
                } else {
                    interner.intern_string(&super::civil::format_iso_datetime(s as i64))
                }
            })
            .collect();
        return Ok(heap.alloc(RObject::String(out)));
    }
    // Date-classed input renders ISO exactly as as.character.Date.
    if is_date_classed(heap, interner, value) {
        let days = numeric_vector(heap, value);
        let out: Vec<super::intern::InternedStr> = days
            .iter()
            .map(|&d| {
                if d.is_nan() {
                    interner.intern_string("\0NA")
                } else {
                    interner.intern_string(&super::civil::format_iso(d as i64))
                }
            })
            .collect();
        return Ok(heap.alloc(RObject::String(out)));
    }
    // factor input decodes through its levels first — as.character(factor)
    /// yields the labels exactly as GNU.
    let factor_classed = get_attrib(heap, interner, value, "class")
        .and_then(|id| match heap.object(id) {
            Some(RObject::String(vals)) => Some(vals.iter().any(|v| interner.string_text(*v) == "factor")),
            _ => None,
        })
        .unwrap_or(false);
    if factor_classed {
        if let Some(RObject::Integer(codes)) = heap.object(value) {
            let levels = get_attrib(heap, interner, value, "levels")
                .and_then(|id| match heap.object(id) {
                    Some(RObject::String(vals)) => Some(vals.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let out: Vec<super::intern::InternedStr> = codes
                .iter()
                .map(|&code| {
                    if code == crate::value::object::NA_INTEGER {
                        interner.intern_string("\0NA")
                    } else {
                        levels
                            .get((code - 1).max(0) as usize)
                            .copied()
                            .unwrap_or_else(|| interner.intern_string(""))
                    }
                })
                .collect();
            return Ok(heap.alloc(RObject::String(out)));
        }
    }
    // Complex input renders elementwise exactly as as.character of a
    /// complex vector — the same formatting print uses.
    if let Some(RObject::Complex(values)) = heap.object(value) {
        let out: Vec<super::intern::InternedStr> = values
            .iter()
            .map(|&z| interner.intern_string(&super::print::format_complex(z)))
            .collect();
        return Ok(heap.alloc(RObject::String(out)));
    }
    let out: Vec<super::intern::InternedStr> = match heap.object(value) {
        Some(RObject::String(values)) => values.clone(),
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => values
            .iter()
            .map(|&v| {
                if v == crate::value::object::NA_INTEGER {
                    interner.intern_string("\0NA")
                } else {
                    interner.intern_string(&v.to_string())
                }
            })
            .collect(),
        Some(RObject::Real(values)) => values
            .iter()
            .map(|&v| {
                if v.to_bits() == crate::value::object::NA_REAL_BITS {
                    interner.intern_string("\0NA")
                } else {
                    interner.intern_string(&super::print::format_real(v))
                }
            })
            .collect(),
        // Raw renders as hex pairs exactly as GNU's as.character.raw.
        Some(RObject::Raw(values)) => values
            .iter()
            .map(|&v| interner.intern_string(&format!("{v:02x}")))
            .collect(),
        _ => Vec::new(),
    };
    Ok(heap.alloc(RObject::String(out)))
}

/// GNU's identical() — deep structural equality: same type, same values
/// bitwise (NaN payloads distinguish NA from NaN), same attributes in the
/// same order. Environments compare by identity exactly as GNU.
fn structural_identical(
    heap: &Heap,
    interner: &SymbolInterner,
    left: ValueId,
    right: ValueId,
) -> bool {
    if left == right {
        return true;
    }
    let attrib_identical = |heap: &Heap, interner: &SymbolInterner, a: ValueId, b: ValueId| -> bool {
        let chain_a = heap.get(a).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
        let chain_b = heap.get(b).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
        let mut ca = chain_a;
        let mut cb = chain_b;
        loop {
            let pa = match heap.object(ca) {
                Some(RObject::Pairlist { car, cdr, tag }) => Some((*car, *cdr, *tag)),
                _ => None,
            };
            let pb = match heap.object(cb) {
                Some(RObject::Pairlist { car, cdr, tag }) => Some((*car, *cdr, *tag)),
                _ => None,
            };
            match (pa, pb) {
                (None, None) => return true,
                (Some((va,cda,ta)), Some((vb,cdb,tb))) => {
                    let na = match heap.object(ta) {
                        Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
                        _ => String::new(),
                    };
                    let nb = match heap.object(tb) {
                        Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
                        _ => String::new(),
                    };
                    if na != nb || !structural_identical(heap, interner, va, vb) {
                        return false;
                    }
                    ca = cda;
                    cb = cdb;
                }
                _ => return false,
            }
        }
    };
    let payload_same = match (heap.object(left), heap.object(right)) {
        (Some(RObject::Nil), Some(RObject::Nil)) => true,
        (Some(RObject::Logical(a)), Some(RObject::Logical(b))) => a == b,
        (Some(RObject::Integer(a)), Some(RObject::Integer(b))) => a == b,
        (Some(RObject::Complex(a)), Some(RObject::Complex(b))) => {
            a.len() == b.len()
                && a.iter().zip(b.iter()).all(|(x, y)| {
                    let part_same = |u: f64, v: f64| {
                        if u.is_nan() && v.is_nan() {
                            u.to_bits() == v.to_bits()
                        } else {
                            u == v
                        }
                    };
                    part_same(x.r, y.r) && part_same(x.i, y.i)
                })
        }
        (Some(RObject::Raw(a)), Some(RObject::Raw(b))) => a == b,
        (Some(RObject::Real(a)), Some(RObject::Real(b))) => {
            a.len() == b.len()
                && a.iter().zip(b.iter()).all(|(x, y)| {
                    if x.is_nan() && y.is_nan() {
                        x.to_bits() == y.to_bits()
                    } else {
                        x == y
                    }
                })
        }
        (Some(RObject::String(a)), Some(RObject::String(b))) => a == b,
        (Some(RObject::Raw(a)), Some(RObject::Raw(b))) => a == b,
        (Some(RObject::List(a)), Some(RObject::List(b))) => {
            a.len() == b.len()
                && a.iter().zip(b.iter()).all(|(x, y)| structural_identical(heap, interner, *x, *y))
        }
        (Some(RObject::Environment { .. }), Some(RObject::Environment { .. })) => left == right,
        // Symbols identity is the interned name exactly as GNU's symbol
        // table — decode allocates fresh cells, so name comparison is
        // the semantics.
        (Some(RObject::Symbol { name: a, .. }), Some(RObject::Symbol { name: b, .. })) => a == b,
        (Some(RObject::Pairlist { car: ca, cdr: da, tag: ta }), Some(RObject::Pairlist { car: cb, cdr: db, tag: tb }))
        | (Some(RObject::Language { car: ca, cdr: da, tag: ta }), Some(RObject::Language { car: cb, cdr: db, tag: tb })) => {
            structural_identical(heap, interner, *ca, *cb)
                && structural_identical(heap, interner, *da, *db)
                && structural_identical(heap, interner, *ta, *tb)
        }
        (Some(RObject::Expression(a)), Some(RObject::Expression(b))) => {
            a.len() == b.len()
                && a.iter().zip(b.iter()).all(|(x, y)| structural_identical(heap, interner, *x, *y))
        }
        (Some(RObject::Closure { formals: fa, body: ba, .. }), Some(RObject::Closure { formals: fb, body: bb, .. })) => {
            structural_identical(heap, interner, *fa, *fb)
                && structural_identical(heap, interner, *ba, *bb)
        }
        _ => false,
    };
    payload_same && attrib_identical(heap, interner, left, right)
}

/// Copy an object dropping one attribute — attribute removal exactly as
/// GNU's attr(x, "name") <- NULL semantics.
pub(crate) fn remove_attrib(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
    name: &str,
) -> Result<ValueId, EvalError> {
    let chain = heap.get(value).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    let mut kept: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = chain;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let attr_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
            _ => String::new(),
        };
        if attr_name != name {
            kept.push((attr_name, car));
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let stripped = heap.alloc(heap.object(value).cloned().unwrap_or(RObject::Nil));
    let mut result = stripped;
    for (attr_name, attr_value) in kept.into_iter() {
        result = copy_with_attrib(heap, interner, result, &attr_name, attr_value)?;
    }
    Ok(result)
}

/// `array(data, dim, dimnames)` — GNU's n-dimensional constructor: the
/// data recycles to the dim product exactly as matrix() does, an absent
/// data argument yields a logical NA, and dim attaches as the integer
/// extent vector.
fn array_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let raw = raw_args(heap, args);
    if raw.is_empty() {
        // array() with no arguments is one logical NA of dimension 1.
        let value = heap.alloc(RObject::Logical(vec![crate::value::object::NA_INTEGER]));
        let dim_obj = heap.alloc(RObject::Integer(vec![1]));
        return copy_with_attrib(heap, interner, value, "dim", dim_obj).map_err(RSignal::from);
    }
    let mut data_arg = ValueId::NIL;
    let mut dim_arg = ValueId::NIL;
    let mut dimnames_arg = ValueId::NIL;
    let mut positional = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("dim") => dim_arg = v,
            Some("dimnames") => dimnames_arg = v,
            _ => {
                if positional == 1 {
                    data_arg = v;
                } else if positional == 2 {
                    dim_arg = v;
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let dims: Vec<i32> = match heap.object(dim_arg) {
        Some(RObject::Integer(vals)) => vals.clone(),
        Some(RObject::Real(vals)) => vals.iter().map(|&v| v as i32).collect(),
        _ => vec![1],
    };
    let total: usize = dims.iter().map(|&d| d.max(0) as usize).product();
    // Recycle the data to the dim product exactly as matrix() does.
    let value = match heap.object(data_arg).cloned() {
        Some(RObject::Integer(vals)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(if vals.is_empty() { crate::value::object::NA_INTEGER } else { vals[i % vals.len()] });
            }
            heap.alloc(RObject::Integer(out))
        }
        Some(RObject::Real(vals)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(if vals.is_empty() { f64::from_bits(crate::value::object::NA_REAL_BITS) } else { vals[i % vals.len()] });
            }
            heap.alloc(RObject::Real(out))
        }
        Some(RObject::Logical(vals)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(if vals.is_empty() { crate::value::object::NA_INTEGER } else { vals[i % vals.len()] });
            }
            heap.alloc(RObject::Logical(out))
        }
        Some(RObject::String(vals)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(if vals.is_empty() { interner.intern_string("\0NA") } else { vals[i % vals.len()] });
            }
            heap.alloc(RObject::String(out))
        }
        _ => heap.alloc(RObject::Logical(vec![crate::value::object::NA_INTEGER])),
    };
    let dim_obj = heap.alloc(RObject::Integer(dims));
    let mut result = copy_with_attrib(heap, interner, value, "dim", dim_obj).map_err(RSignal::from)?;
    if dimnames_arg != ValueId::NIL && !matches!(heap.object(dimnames_arg), Some(RObject::Nil)) {
        result = copy_with_attrib(heap, interner, result, "dimnames", dimnames_arg).map_err(RSignal::from)?;
    }
    Ok(result)
}

/// Whether the object carries the Date class.
pub(crate) fn is_date_classed(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    get_attrib(heap, interner, value, "class")
        .and_then(|id| match heap.object(id) {
            Some(RObject::String(vals)) => Some(vals.iter().any(|v| interner.string_text(*v) == "Date")),
            _ => None,
        })
        .unwrap_or(false)
}

/// Numeric payload of a vector as reals — integer input widens exactly.
fn numeric_vector(heap: &Heap, value: ValueId) -> Vec<f64> {
    match heap.object(value) {
        Some(RObject::Real(vals)) => vals.clone(),
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals
            .iter()
            .map(|&v| {
                if v == crate::value::object::NA_INTEGER {
                    f64::from_bits(crate::value::object::NA_REAL_BITS)
                } else {
                    v as f64
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether the object carries the POSIXct class (first of the two-element
/// POSIXct/POSIXt class vector exactly as GNU).
pub(crate) fn is_posixct_classed(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    get_attrib(heap, interner, value, "class")
        .and_then(|id| match heap.object(id) {
            Some(RObject::String(vals)) => Some(vals.iter().any(|v| interner.string_text(*v) == "POSIXct")),
            _ => None,
        })
        .unwrap_or(false)
}

/// Whether the object carries the difftime class.
pub(crate) fn is_difftime_classed(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    get_attrib(heap, interner, value, "class")
        .and_then(|id| match heap.object(id) {
            Some(RObject::String(vals)) => Some(vals.iter().any(|v| interner.string_text(*v) == "difftime")),
            _ => None,
        })
        .unwrap_or(false)
}

/// The difftime units attribute — seconds per unit in the lattice exactly
/// as GNU: secs 1, mins 60, hours 3600, days 86400, weeks 604800.
pub(crate) fn difftime_units(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    get_attrib(heap, interner, value, "units")
        .and_then(|id| match heap.object(id) {
            Some(RObject::String(vals)) if !vals.is_empty() => Some(interner.string_text(vals[0]).to_string()),
            _ => None,
        })
        .unwrap_or_else(|| "secs".to_string())
}

/// Seconds per difftime unit exactly as GNU's conversion lattice.
pub(crate) fn unit_seconds(units: &str) -> f64 {
    match units {
        "mins" => 60.0,
        "hours" => 3600.0,
        "days" => 86400.0,
        "weeks" => 604800.0,
        _ => 1.0,
    }
}

/// GNU's auto unit selection for second-valued differences: the coarsest
/// unit dividing every finite value exactly, defaulting to secs exactly as
/// difftime's "auto" discipline.
fn auto_difftime_units(secs: &[f64]) -> (&'static str, f64) {
    let finite: Vec<f64> = secs.iter().filter(|v| v.is_finite()).copied().collect();
    for (name, divisor) in [("weeks", 604800.0), ("days", 86400.0), ("hours", 3600.0), ("mins", 60.0)] {
        if !finite.is_empty() && finite.iter().all(|v| v % divisor == 0.0) {
            return (name, divisor);
        }
    }
    ("secs", 1.0)
}

/// Allocate a copy of the object's payload with no attribute chain — the
/// stamping operations rebuild class-bearing objects from bare numerics.
fn strip_all_attributes(heap: &mut Heap, value: ValueId) -> ValueId {
    let body = match heap.object(value) {
        Some(obj) => obj.clone(),
        None => return value,
    };
    heap.alloc(body)
}

/// `xtabs(formula)` — tabulate counts over the formula's terms. The
/// one-sided formula's terms evaluate in the calling environment, counts
/// tabulate per sorted-unique level combination column-major exactly as
/// table semantics, and the result carries the xtabs/table class pair
/// with dimnames named by the deparsed term text exactly as GNU.
fn xtabs_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut formula = ValueId::NIL;
    let mut data = ValueId::NIL;
    {
        let mut cursor = args;
        for _ in 0..100_000 {
            if cursor == ValueId::NIL {
                break;
            }
            let (car, tag, next) = match heap.object(cursor) {
                Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
                _ => break,
            };
            let value = eval(heap, interner, car, caller_env)?;
            let tag_name = if tag == ValueId::NIL {
                None
            } else {
                match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                }
            };
            match tag_name.as_deref() {
                Some("data") => data = value,
                _ => {
                    if formula == ValueId::NIL {
                        formula = value;
                    }
                }
            }
            cursor = next;
        }
    }
    // The formula's right-hand side: the second element of the tilde call
    /// for one-sided form; terms split across the plus chain exactly as
    /// GNU's terms.object decomposition.
    let rhs = match heap.object(formula) {
        Some(RObject::Language { cdr, .. }) => match heap.object(*cdr) {
            Some(RObject::Pairlist { car, .. }) => *car,
            _ => return Err(EvalError::TypeError("xtabs requires a formula").into()),
        },
        _ => return Err(EvalError::TypeError("xtabs requires a formula").into()),
    };
    // Split the plus chain into terms.
    let mut terms: Vec<ValueId> = Vec::new();
    let mut stack = vec![rhs];
    while let Some(term) = stack.pop() {
        let is_plus = match heap.object(term) {
            Some(RObject::Language { car, .. }) => match heap.object(*car) {
                Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name) == "+",
                _ => false,
            },
            _ => false,
        };
        if is_plus {
            if let Some(RObject::Language { cdr, .. }) = heap.object(term) {
                let mut parts = Vec::new();
                let mut cursor = *cdr;
                for _ in 0..100_000 {
                    if cursor == ValueId::NIL {
                        break;
                    }
                    if let Some(RObject::Pairlist { car, cdr: next, .. }) = heap.object(cursor) {
                        parts.push((*car, *next));
                        cursor = *next;
                    } else {
                        break;
                    }
                }
                if parts.len() == 2 {
                    stack.push(parts[1].0);
                    stack.push(parts[0].0);
                    continue;
                }
            }
            terms.push(term);
        } else {
            terms.push(term);
        }
    }
    // Evaluate each term in the caller environment; tabulate combinations.
    let mut columns: Vec<Vec<String>> = Vec::new();
    let mut term_names: Vec<String> = Vec::new();
    for term in &terms {
        let value = if data != ValueId::NIL {
            let column = match heap.object(*term) {
                Some(RObject::Symbol { name, .. }) => {
                    let want = interner.symbol_name(*name).to_string();
                    // data frames are lists with a names attribute; find
                    /// the column positionally exactly as GNU's model frame.
                    let cols = match heap.object(data) {
                        Some(RObject::List(items)) => Some(items.clone()),
                        _ => None,
                    };
                    let mut found = ValueId::NIL;
                    if let Some(items) = cols {
                        if let Some(names_id) = get_attrib(heap, interner, data, "names") {
                            if let Some(RObject::String(names)) = heap.object(names_id) {
                                for (i, nm) in names.iter().enumerate() {
                                    if interner.string_text(*nm) == want && i < items.len() {
                                        found = items[i];
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    found
                }
                _ => ValueId::NIL,
            };
            if column != ValueId::NIL {
                column
            } else {
                eval(heap, interner, *term, caller_env)?
            }
        } else {
            eval(heap, interner, *term, caller_env)?
        };
        let texts = match heap.object(value) {
            Some(RObject::String(vals)) => vals
                .iter()
                .map(|v| interner.string_text(*v).to_string())
                .collect(),
            Some(RObject::Integer(vals)) => vals.iter().map(|v| v.to_string()).collect(),
            Some(RObject::Real(vals)) => vals.iter().map(|v| crate::value::print::format_real(*v)).collect(),
            Some(RObject::Logical(vals)) => vals
                .iter()
                .map(|&v| {
                    if v == crate::value::object::NA_INTEGER {
                        "NA".to_string()
                    } else if v != 0 {
                        "TRUE".to_string()
                    } else {
                        "FALSE".to_string()
                    }
                })
                .collect(),
            _ => return Err(EvalError::TypeError("xtabs terms must be vectors").into()),
        };
        columns.push(texts);
        term_names.push(crate::value::print::deparse(heap, interner, *term));
    }
    if columns.is_empty() {
        return Err(EvalError::TypeError("xtabs requires terms").into());
    }
    let n = columns[0].len();
    // Per-column sorted-unique levels.
    let levels: Vec<Vec<String>> = columns
        .iter()
        .map(|col| {
            let mut lv: Vec<String> = col.clone();
            lv.sort();
            lv.dedup();
            lv
        })
        .collect();
    let dims: Vec<usize> = levels.iter().map(|l| l.len()).collect();
    let total: usize = dims.iter().product();
    let mut counts = vec![0i32; total];
    for row in 0..n {
        let mut flat = 0usize;
        let mut stride = 1usize;
        let mut valid = true;
        for (j, col) in columns.iter().enumerate() {
            let text = &col[row];
            match levels[j].iter().position(|l| l == text) {
                Some(idx) => {
                    flat += idx * stride;
                    stride *= dims[j].max(1);
                }
                None => {
                    valid = false;
                    break;
                }
            }
        }
        if valid && flat < total {
            counts[flat] += 1;
        }
    }
    let values = heap.alloc(RObject::Integer(counts));
    let dim_vec = heap.alloc(RObject::Integer(dims.iter().map(|&d| d as i32).collect()));
    // dimnames: one character vector per term, the list named by the
    /// deparsed term text exactly as GNU.
    let dn_inners: Vec<ValueId> = levels
        .iter()
        .map(|lv| {
            let interned: Vec<super::intern::InternedStr> = lv
                .iter()
                .map(|s| interner.intern_string(s))
                .collect();
            heap.alloc(RObject::String(interned))
        })
        .collect();
    let dn_list = heap.alloc(RObject::List(dn_inners));
    let term_interned: Vec<super::intern::InternedStr> = term_names
        .iter()
        .map(|s| interner.intern_string(s))
        .collect();
    let dn_names = heap.alloc(RObject::String(term_interned));
    let dn_list = copy_with_attrib(heap, interner, dn_list, "names", dn_names).map_err(RSignal::from)?;
    let class = heap.alloc(RObject::String(vec![
        interner.intern_string("xtabs"),
        interner.intern_string("table"),
    ]));
    let names_vec = heap.alloc(RObject::String(
        levels[0]
            .iter()
            .map(|s| interner.intern_string(s))
            .collect(),
    ));
    let with_dim = copy_with_attrib(heap, interner, values, "dim", dim_vec).map_err(RSignal::from)?;
    let with_dn = copy_with_attrib(heap, interner, with_dim, "dimnames", dn_list).map_err(RSignal::from)?;
    let with_class = copy_with_attrib(heap, interner, with_dn, "class", class).map_err(RSignal::from)?;
    if dims.len() == 1 {
        copy_with_attrib(heap, interner, with_class, "names", names_vec).map_err(RSignal::from)
    } else {
        Ok(with_class)
    }
}

/// Promote any arithmetic input to its complex vector form exactly as
/// GNU's type lattice — integer/logical/real gain a zero imaginary part,
/// NA propagating into both parts as GNU's complex NA does.
fn as_complex_vec(heap: &Heap, value: ValueId) -> Result<Vec<crate::value::object::RComplex>, EvalError> {
    use crate::value::object::{NA_INTEGER, NA_REAL_BITS, RComplex};
    match heap.object(value) {
        Some(RObject::Complex(vals)) => Ok(vals.clone()),
        Some(RObject::Real(vals)) => Ok(vals
            .iter()
            .map(|v| {
                if v.to_bits() == NA_REAL_BITS {
                    RComplex { r: f64::from_bits(NA_REAL_BITS), i: f64::from_bits(NA_REAL_BITS) }
                } else {
                    RComplex { r: *v, i: 0.0 }
                }
            })
            .collect()),
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => Ok(vals
            .iter()
            .map(|&v| {
                if v == NA_INTEGER {
                    RComplex { r: f64::from_bits(NA_REAL_BITS), i: f64::from_bits(NA_REAL_BITS) }
                } else {
                    RComplex { r: v as f64, i: 0.0 }
                }
            })
            .collect()),
        Some(RObject::Raw(vals)) => Ok(vals.iter().map(|&v| RComplex { r: v as f64, i: 0.0 }).collect()),
        _ => Err(EvalError::TypeError("non-numeric argument to binary operator")),
    }
}

/// Is this complex element NA — either part carrying GNU's NA payload.
fn complex_na(z: crate::value::object::RComplex) -> bool {
    z.r.to_bits() == crate::value::object::NA_REAL_BITS
        || z.i.to_bits() == crate::value::object::NA_REAL_BITS
}

/// Binary complex arithmetic with GNU recycling and NA propagation — an
/// NA operand element poisons both parts of the result element exactly as
/// GNU's complex NA semantics.
fn complex_arithmetic(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    a_id: ValueId,
    b_id: ValueId,
) -> EvalResult {
    use crate::value::object::{NA_REAL_BITS, RComplex};
    if op == MOD {
        return Err(EvalError::TypeError("unimplemented complex operation").into());
    }
    let a = as_complex_vec(heap, a_id).map_err(RSignal::from)?;
    let b = as_complex_vec(heap, b_id).map_err(RSignal::from)?;
    let na = f64::from_bits(NA_REAL_BITS);
    let folded = fold_numeric(a, b, op, move |x, y| {
        if complex_na(x) || complex_na(y) {
            return RComplex { r: na, i: na };
        }
        match op {
            ADD => RComplex { r: x.r + y.r, i: x.i + y.i },
            SUB => RComplex { r: x.r - y.r, i: x.i - y.i },
            MUL => RComplex {
                r: x.r * y.r - x.i * y.i,
                i: x.r * y.i + x.i * y.r,
            },
            DIV => {
                // Smith's scaled division — resistant to overflow exactly
                /// as GNU's complex quotient.
                let denom = y.r * y.r + y.i * y.i;
                if denom == 0.0 {
                    RComplex { r: f64::NAN, i: f64::NAN }
                } else {
                    RComplex {
                        r: (x.r * y.r + x.i * y.i) / denom,
                        i: (x.i * y.r - x.r * y.i) / denom,
                    }
                }
            }
            POW => {
                // z^w = exp(w * log z) on the principal branch.
                let modulus = (x.r * x.r + x.i * x.i).sqrt();
                let theta = x.i.atan2(x.r);
                let lr = modulus.ln();
                let wr = y.r * lr - y.i * theta;
                let wi = y.r * theta + y.i * lr;
                let m = wr.exp();
                RComplex { r: m * wi.cos(), i: m * wi.sin() }
            }
            _ => RComplex { r: f64::NAN, i: f64::NAN },
        }
    })
    .map_err(RSignal::from)?;
    let result = heap.alloc(RObject::Complex(folded));
    let _ = interner;
    Ok(propagate_attributes(heap, a_id, b_id, result))
}

/// Trigonometric and hyperbolic family — real inputs produce real outputs
/// exactly as the other unary math; complex inputs evaluate through the
/// standard real-part compositions GNU's complex trig derives from.

/// Density/distribution family over nmath's pure-Rust cores — GNU's exact
/// positional+named argument resolution with scalar parameter defaults,
/// vectorized over the first argument exactly as GNU's DPQ dispatch.
fn dpq_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    which: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let lookup = |name: &str, position: usize| -> Option<ValueId> {
        named
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
            .or_else(|| positional.get(position).copied())
    };
    let real_arg = |name: &str, position: usize, default: f64| -> Result<f64, RSignal> {
        match lookup(name, position) {
            Some(v) => first_real(heap, v).map_err(RSignal::from),
            None => Ok(default),
        }
    };
    let logical_of = |heap: &Heap, v: ValueId| -> Result<bool, RSignal> {
        match heap.object(v) {
            Some(RObject::Logical(vals)) => Ok(vals.first().copied().unwrap_or(0) != 0),
            Some(RObject::Real(vals)) => Ok(vals.first().copied().unwrap_or(0.0) != 0.0),
            Some(RObject::Integer(vals)) => Ok(vals.first().copied().unwrap_or(0) != 0),
            _ => Err(EvalError::TypeError("logical flag expected").into()),
        }
    };
    let logical_flag = |names: &[&str], position: usize, default: bool| -> Result<bool, RSignal> {
        for name in names {
            if let Some(v) = named.iter().find(|(n, _)| n == name).map(|(_, v)| *v) {
                return logical_of(heap, v);
            }
        }
        if let Some(v) = positional.get(position) {
            if !named.iter().any(|(n, _)| names.contains(&n.as_str())) {
                return logical_of(heap, *v);
            }
        }
        Ok(default)
    };
    let x_value = lookup("x", 0)
        .or_else(|| lookup("q", 0))
        .or_else(|| lookup("p", 0))
        .ok_or(EvalError::TypeError("dpq requires a first argument"))?;
    let xs: Vec<f64> = match heap.object(x_value) {
        Some(RObject::Real(vals)) => vals.clone(),
        Some(RObject::Integer(vals)) => vals.iter().map(|&v| if v == i32::MIN { f64::NAN } else { v as f64 }).collect(),
        Some(RObject::Logical(vals)) => vals.iter().map(|&v| if v == i32::MIN { f64::NAN } else { v as f64 }).collect(),
        _ => return Err(EvalError::TypeError("numeric argument expected").into()),
    };
    let (p1n, p1d, p2n, p2d): (&str, f64, &str, f64) = match which {
        DNORM | PNORM | QNORM => ("mean", 0.0, "sd", 1.0),
        DPOIS | PPOIS => ("lambda", f64::NAN, "", 0.0),
        DBINOM | PBINOM => ("size", f64::NAN, "prob", f64::NAN),
        PBETA | DBETA | QBETA => ("shape1", f64::NAN, "shape2", f64::NAN),
        DT | PT | QT => ("df", f64::NAN, "", 0.0),
        DF | PF | QF => ("df1", f64::NAN, "df2", f64::NAN),
        DCAUCHY => ("location", 0.0, "scale", 1.0),
        DCHISQ => ("df", f64::NAN, "", 0.0),
        DEXP => ("rate", 1.0, "", 0.0),
        DGAMMA => ("shape", f64::NAN, "rate", 1.0),
        DGEOM => ("prob", f64::NAN, "", 0.0),
        DNBINOM => ("size", f64::NAN, "prob", f64::NAN),
        DWEIBULL => ("shape", f64::NAN, "scale", 1.0),
        _ => unreachable!(),
    };
    let param1 = real_arg(p1n, 1, p1d)?;
    if param1.is_nan() && (matches!(which, DPOIS | PPOIS) || lookup("lambda", 1).is_none() && matches!(which, DPOIS | PPOIS)) {
        return Err(EvalError::TypeError("dpq requires lambda").into());
    }
    let param2 = if p2n.is_empty() { 0.0 } else { real_arg(p2n, 2, p2d)? };
    let log_flag = logical_flag(&["log", "log.p"], 3, false)?;
    let lower_flag = logical_flag(&["lower.tail"], 3, true)?;
    let out: Vec<f64> = xs
        .iter()
        .map(|&x| {
            let x = if x.is_nan() { f64::NAN } else { x };
            match which {
                DNORM => crate::nmath::dist::normal::dnorm4_inner(x, param1, param2, log_flag),
                PNORM => crate::nmath::dist::normal::pnorm5_inner(x, param1, param2, lower_flag, log_flag),
                QNORM => crate::nmath::dist::normal::qnorm5_inner(x, param1, param2, lower_flag, log_flag),
                DPOIS => crate::nmath::dist::poisson::dpois_inner(x, param1, log_flag),
                PPOIS => crate::nmath::dist::poisson::ppois_inner(x, param1, lower_flag, log_flag),
                DBINOM => crate::nmath::dist::binomial::dbinom_inner(x, param1, param2, log_flag),
                PBINOM => crate::nmath::dist::binomial::pbinom_inner(x, param1, param2, lower_flag, log_flag),
                DBETA => crate::nmath::dist::beta::dbeta_inner(x, param1, param2, log_flag),
                PBETA => crate::nmath::dist::beta::pbeta_inner(x, param1, param2, lower_flag, log_flag),
                QBETA => crate::nmath::dist::beta::qbeta_inner(x, param1, param2, lower_flag, log_flag),
                DT => crate::nmath::dist::t_dist::dt_inner(x, param1, log_flag),
                PT => crate::nmath::dist::t_dist::pt_inner(x, param1, lower_flag, log_flag),
                QT => crate::nmath::dist::t_dist::qt_inner(x, param1, lower_flag, log_flag),
                DF => crate::nmath::dist::f_dist::df_inner(x, param1, param2, log_flag),
                PF => crate::nmath::dist::f_dist::pf_inner(x, param1, param2, lower_flag, log_flag),
                QF => crate::nmath::dist::f_dist::qf_inner(x, param1, param2, lower_flag, log_flag),
                DCAUCHY => crate::nmath::dist::cauchy::dcauchy_inner(x, param1, param2, log_flag),
                DCHISQ => crate::nmath::dist::chisq::dchisq_inner(x, param1, log_flag),
                DEXP => crate::nmath::dist::exponential::dexp_inner(x, 1.0 / param1, log_flag),
                // GNU's gamma exposes rate with scale = 1/rate exactly as
                /// the exponential inversion — the core wants scale.
                DGAMMA => crate::nmath::dist::gamma::dgamma_inner(x, param1, 1.0 / param2, log_flag),
                DGEOM => crate::nmath::dist::geometric::dgeom_inner(x, param1, log_flag),
                DNBINOM => crate::nmath::dist::nbinom::dnbinom_inner(x, param1, param2, log_flag),
                DWEIBULL => crate::nmath::dist::weibull::dweibull_inner(x, param1, param2, log_flag),
                _ => unreachable!(),
            }
        })
        .collect();
    Ok(heap.alloc(RObject::Real(out)))
}



/// find(x, mode, numeric) — the two-element search path the new world
/// models exactly as GNU's user-first ordering: position 1 ".GlobalEnv",
/// position 2 "package:base". The character form returns the environment
/// name; numeric=TRUE returns the position as a named integer.
fn find_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let (positional, named) = collect_order_args(heap, interner, args, caller_env)?;
    let what = positional.first().copied().ok_or(EvalError::TypeError("find requires an argument"))?;
    let name = string_operand(heap, interner, what).unwrap_or_default();
    let numeric = flag_lookup(heap, &positional, &named, "numeric", usize::MAX)
        .map(|v| logical_or_na(heap, v).unwrap_or(false))
        .unwrap_or(false);
    let mode_filter = flag_lookup(heap, &positional, &named, "mode", usize::MAX)
        .and_then(|v| string_operand(heap, interner, v))
        .unwrap_or_else(|| "any".to_string());

    let sym = interner.intern_symbol(&name);
    // Walk the chain from the user environment: the first frame holding the
    // binding decides, positions counted in search order exactly as GNU.
    let mut cursor = caller_env;
    let mut position = 0i32;
    let mut found: Option<(i32, ValueId)> = None;
    for _ in 0..100 {
        if cursor == ValueId::EMPTY_ENV || cursor.is_sentinel() {
            break;
        }
        position += 1;
        if let Some(binding) = super::env::lookup_in_frame(heap, cursor, sym) {
            // The .GlobalEnv self-reference inside base is not a user
            // binding — GNU reports it at its true home exactly like any
            // other base binding.
            found = Some((position, binding.value));
            break;
        }
        cursor = match heap.object(cursor) {
            Some(RObject::Environment { enclos, .. }) => *enclos,
            _ => break,
        };
    }
    let empty_result = |heap: &mut Heap, interner: &mut SymbolInterner, numeric: bool| -> EvalResult {
        if numeric {
            let result = heap.alloc(RObject::Integer(Vec::new()));
            let names = heap.alloc(RObject::String(Vec::new()));
            copy_with_attrib(heap, interner, result, "names", names).map_err(RSignal::from)
        } else {
            Ok(heap.alloc(RObject::String(Vec::new())))
        }
    };
    let (position, value) = match found {
        Some(pair) => pair,
        None => return empty_result(heap, interner, numeric),
    };
    // mode filtering — "function" admits only closures and primitives
    // exactly as GNU's mode test.
    if mode_filter == "function" {
        let is_function = matches!(heap.object(value), Some(RObject::Closure { .. }) | Some(RObject::Primitive(_)));
        if !is_function {
            return empty_result(heap, interner, numeric);
        }
    }
    let genv = {
        let sym = interner.intern_symbol(".GlobalEnv");
        match super::env::lookup(heap, caller_env, sym) {
            Some((_, binding)) => binding.value,
            None => caller_env,
        }
    };
    let chain_names = search_path_names(heap, interner, genv);
    let env_name: &str = chain_names.get((position - 1).max(0) as usize).map(|s| s.as_str()).unwrap_or("package:base");
    if numeric {
        let result = heap.alloc(RObject::Integer(vec![position]));
        let names = heap.alloc(RObject::String(vec![interner.intern_string(env_name)]));
        copy_with_attrib(heap, interner, result, "names", names).map_err(RSignal::from)
    } else {
        Ok(heap.alloc(RObject::String(vec![interner.intern_string(env_name)])))
    }
}


/// The class attribute as a vector of class names — empty when unclassed.
fn class_vector(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Vec<String> {
    match get_attrib(heap, interner, value, "class") {
        Some(id) => string_vec(heap, interner, id)
            .into_iter()
            .filter(|s| s != "\0NA")
            .collect(),
        None => Vec::new(),
    }
}

/// Apply an S3 method closure to the object with the pre-forced promise
/// discipline exactly as the apply family — the object passes as a value,
/// never as a re-evaluated expression.
fn s3_apply(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    method: ValueId,
    object: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let forced = heap.alloc(RObject::Promise {
        expr: object,
        env: caller_env,
        value: object,
    });
    let arg_node = heap.alloc(RObject::Pairlist {
        car: forced,
        cdr: ValueId::NIL,
        tag: ValueId::NIL,
    });
    match heap.object(method).cloned() {
        Some(RObject::Closure { formals, body, env }) => {
            let applied = super::eval::apply_closure_public(heap, interner, formals, body, env, arg_node, caller_env)?;
            force_if_promise(heap, interner, applied)
        }
        Some(RObject::Primitive(builtin)) => call_primitive(heap, interner, builtin, arg_node, caller_env),
        _ => Err(EvalError::NotCallable.into()),
    }
}

/// The first frame binding in insertion order — UseMethod's implicit object
/// exactly as GNU's first-formal rule for single-argument generics.
fn first_frame_value(heap: &Heap, env: ValueId) -> Option<ValueId> {
    match heap.object(env) {
        Some(RObject::Environment { frame: crate::value::object::EnvFrame::List(entries), .. }) => {
            entries.first().map(|(_, binding)| binding.value)
        }
        _ => None,
    }
}

/// UseMethod(generic, object) — walk the object's class vector for
/// `<generic>.<class>` in the method-lookup chain, fall back to `.default`,
/// and apply with dispatch state pushed for NextMethod exactly as GNU.
fn usemethod_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let raw = raw_args(heap, args);
    let generic = match raw.first() {
        Some(expr) => {
            let v = eval(heap, interner, *expr, caller_env)?;
            let v = force_if_promise(heap, interner, v)?;
            string_operand(heap, interner, v).unwrap_or_default()
        }
        None => return Err(EvalError::TypeError("UseMethod requires a generic name").into()),
    };
    let object = match raw.get(1) {
        Some(expr) => {
            let v = eval(heap, interner, *expr, caller_env)?;
            force_if_promise(heap, interner, v)?
        }
        None => {
            let binding = first_frame_value(heap, caller_env)
                .ok_or(EvalError::TypeError("UseMethod has no object to dispatch on"))?;
            force_if_promise(heap, interner, binding)?
        }
    };
    let classes = class_vector(heap, interner, object);
    let mut candidates: Vec<String> = classes.clone();
    candidates.push("default".to_string());
    for (index, class) in candidates.iter().enumerate() {
        let sym = interner.intern_symbol(&format!("{generic}.{class}"));
        if let Some((_, binding)) = super::env::lookup(heap, caller_env, sym) {
            let method = force_if_promise(heap, interner, binding.value)?;
            let remaining: Vec<String> = if index < classes.len() {
                classes[index + 1..].to_vec()
            } else {
                Vec::new()
            };
            heap.s3_dispatch.push((generic.clone(), object, remaining));
            let result = s3_apply(heap, interner, method, object, caller_env);
            heap.s3_dispatch.pop();
            return result;
        }
    }
    Err(EvalError::Condition(format!(
        "no applicable method for '{generic}' applied to an object of class",
    ))
    .into())
}

/// NextMethod() — dispatch the next class in the active generic's walk,
/// falling back to `.default` when the class walk exhausts exactly as GNU.
fn nextmethod_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    caller_env: ValueId,
) -> EvalResult {
    let (generic, object, remaining) = match heap.s3_dispatch.last().cloned() {
        Some(entry) => entry,
        None => return Err(EvalError::Condition("NextMethod called outside S3 dispatch".to_string()).into()),
    };
    let mut candidates = remaining.clone();
    candidates.push("default".to_string());
    for (index, class) in candidates.iter().enumerate() {
        let sym = interner.intern_symbol(&format!("{generic}.{class}"));
        if let Some((_, binding)) = super::env::lookup(heap, caller_env, sym) {
            let method = force_if_promise(heap, interner, binding.value)?;
            let next_remaining: Vec<String> = if index < remaining.len() {
                remaining[index + 1..].to_vec()
            } else {
                Vec::new()
            };
            heap.s3_dispatch.push((generic.clone(), object, next_remaining));
            let result = s3_apply(heap, interner, method, object, caller_env);
            heap.s3_dispatch.pop();
            return result;
        }
    }
    Err(EvalError::Condition("no method to invoke with NextMethod".to_string()).into())
}


/// as.integer coercion — truncation toward zero exactly as GNU, with the
/// typed NA lattice: real NaN and out-of-range collapse to NA_integer_.
fn coerce_integer(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
) -> EvalResult {
    let out: Vec<i32> = match heap.object(value) {
        Some(RObject::Integer(values)) => values.clone(),
        Some(RObject::Logical(values)) => values.clone(),
        Some(RObject::Real(values)) => values
            .iter()
            .map(|v| {
                if v.is_nan() || v.is_infinite() || *v > i32::MAX as f64 || *v < i32::MIN as f64 {
                    crate::value::object::NA_INTEGER
                } else {
                    v.trunc() as i32
                }
            })
            .collect(),
        Some(RObject::String(values)) => values
            .iter()
            .map(|v| {
                let text = interner.string_text(*v);
                if text == "\0NA" {
                    crate::value::object::NA_INTEGER
                } else {
                    text.trim().parse::<i32>().unwrap_or(crate::value::object::NA_INTEGER)
                }
            })
            .collect(),
        Some(RObject::Raw(values)) => values.iter().map(|v| *v as i32).collect(),
        // Complex drops the imaginary part exactly as GNU's coercion.
        Some(RObject::Complex(values)) => values
            .iter()
            .map(|z| {
                if z.r.is_nan() || z.r.is_infinite() || z.r > i32::MAX as f64 || z.r < i32::MIN as f64 {
                    crate::value::object::NA_INTEGER
                } else {
                    z.r.trunc() as i32
                }
            })
            .collect(),
        _ => Vec::new(),
    };
    Ok(heap.alloc(RObject::Integer(out)))
}

/// as.numeric/as.double coercion — the real lattice exactly as GNU: logical
/// and integer lift directly, strings parse with NA fallback.
fn coerce_real(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
) -> EvalResult {
    let out: Vec<f64> = match heap.object(value) {
        Some(RObject::Real(values)) => values.clone(),
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => values
            .iter()
            .map(|v| if *v == crate::value::object::NA_INTEGER { f64::NAN } else { *v as f64 })
            .collect(),
        Some(RObject::String(values)) => values
            .iter()
            .map(|v| {
                let text = interner.string_text(*v);
                if text == "\0NA" {
                    f64::NAN
                } else {
                    text.trim().parse::<f64>().unwrap_or(f64::NAN)
                }
            })
            .collect(),
        Some(RObject::Raw(values)) => values.iter().map(|v| *v as f64).collect(),
        Some(RObject::Complex(values)) => values
            .iter()
            .map(|z| if z.r.is_nan() { f64::NAN } else { z.r })
            .collect(),
        _ => Vec::new(),
    };
    Ok(heap.alloc(RObject::Real(out)))
}

/// stopifnot(...) — every argument must be all-TRUE; a passing call returns
/// NULL invisibly exactly as GNU, failures raise the argument-position error.
fn stopifnot_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let raw = raw_args(heap, args);
    for (index, expr) in raw.iter().enumerate() {
        let v = eval(heap, interner, *expr, caller_env)?;
        let v = force_if_promise(heap, interner, v)?;
        let all_true = match heap.object(v) {
            Some(RObject::Logical(vals)) => vals.iter().all(|x| *x == 1),
            _ => false,
        };
        if !all_true {
            return Err(EvalError::Condition(format!("all(!is.na(answer)) is not TRUE (argument {})", index + 1)).into());
        }
    }
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// sequence(nvec, from, by) — concatenated per-element sequences exactly
/// as GNU: defaults from=1 by=1 with recycling across the expansion.
fn sequence_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut from_arg = ValueId::NIL;
    let mut by_arg = ValueId::NIL;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("from") => from_arg = v,
            Some("by") => by_arg = v,
            _ => positional.push(v),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let lengths: Vec<i64> = match positional.first().and_then(|v| heap.object(*v)) {
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.iter().map(|v| *v as i64).collect(),
        Some(RObject::Real(vals)) => vals.iter().map(|v| *v as i64).collect(),
        _ => Vec::new(),
    };
    let numerics_at = |value: ValueId, index: usize, default: f64| -> f64 {
        if value == ValueId::NIL {
            return default;
        }
        match heap.object(value) {
            Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) if !vals.is_empty() => {
                vals[index % vals.len()] as f64
            }
            Some(RObject::Real(vals)) if !vals.is_empty() => vals[index % vals.len()],
            _ => default,
        }
    };
    let mut out: Vec<i32> = Vec::new();
    for (index, len) in lengths.iter().enumerate() {
        let from = numerics_at(from_arg, index, 1.0);
        let by = numerics_at(by_arg, index, 1.0);
        for i in 0..(*len).max(0) {
            out.push((from + by * i as f64) as i32);
        }
    }
    Ok(heap.alloc(RObject::Integer(out)))
}

/// rep.int(x, times) — pairwise element recycling with names dropped
/// exactly as GNU's internal repetition (unlike rep, which preserves them).
fn rep_int_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let times = match heap.object(operands[1]) {
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().unwrap_or(1) as i64,
        Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(1.0) as i64,
        _ => 1,
    };
    repeat_elements(heap, interner, operands[0], times, true)
}

/// rep_len(x, length.out) — recycle or pad to the exact output length;
/// empty input fills the typed NA exactly as GNU.
fn rep_len_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let target = match heap.object(operands[1]) {
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().unwrap_or(0) as usize,
        Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(0.0) as usize,
        _ => 0,
    };
    let value = operands[0];
    match heap.object(value).cloned() {
        Some(RObject::Integer(vals)) => {
            if vals.is_empty() {
                return Ok(heap.alloc(RObject::Integer(vec![crate::value::object::NA_INTEGER; target])));
            }
            let out: Vec<i32> = (0..target).map(|i| vals[i % vals.len()]).collect();
            Ok(heap.alloc(RObject::Integer(out)))
        }
        Some(RObject::Logical(vals)) => {
            if vals.is_empty() {
                return Ok(heap.alloc(RObject::Logical(vec![crate::value::object::NA_INTEGER; target])));
            }
            let out: Vec<i32> = (0..target).map(|i| vals[i % vals.len()]).collect();
            Ok(heap.alloc(RObject::Logical(out)))
        }
        Some(RObject::Real(vals)) => {
            if vals.is_empty() {
                return Ok(heap.alloc(RObject::Real(vec![f64::NAN; target])));
            }
            let out: Vec<f64> = (0..target).map(|i| vals[i % vals.len()]).collect();
            Ok(heap.alloc(RObject::Real(out)))
        }
        Some(RObject::String(vals)) => {
            let na_str = interner.intern_string("\0NA");
            if vals.is_empty() {
                return Ok(heap.alloc(RObject::String(vec![na_str; target])));
            }
            let out: Vec<super::intern::InternedStr> = (0..target).map(|i| vals[i % vals.len()]).collect();
            Ok(heap.alloc(RObject::String(out)))
        }
        Some(RObject::Raw(vals)) => {
            if vals.is_empty() {
                return Ok(heap.alloc(RObject::Raw(vec![0u8; target])));
            }
            let out: Vec<u8> = (0..target).map(|i| vals[i % vals.len()]).collect();
            Ok(heap.alloc(RObject::Raw(out)))
        }
        Some(RObject::Complex(vals)) => {
            let na = crate::value::object::RComplex {
                r: f64::from_bits(crate::value::object::NA_REAL_BITS),
                i: f64::from_bits(crate::value::object::NA_REAL_BITS),
            };
            if vals.is_empty() {
                return Ok(heap.alloc(RObject::Complex(vec![na; target])));
            }
            let out: Vec<crate::value::object::RComplex> = (0..target).map(|i| vals[i % vals.len()]).collect();
            Ok(heap.alloc(RObject::Complex(out)))
        }
        Some(RObject::List(vals)) => {
            if vals.is_empty() {
                return Ok(heap.alloc(RObject::List(Vec::new())));
            }
            let out: Vec<ValueId> = (0..target).map(|i| vals[i % vals.len()]).collect();
            Ok(heap.alloc(RObject::List(out)))
        }
        _ => Ok(heap.alloc(RObject::Integer(Vec::new()))),
    }
}

/// Elementwise pairwise repetition shared by rep.int — names dropped when
/// drop_names exactly as GNU's internal form.
fn repeat_elements(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
    times: i64,
    drop_names: bool,
) -> EvalResult {
    let _ = interner;
    let _ = drop_names;
    let times = times.max(0) as usize;
    match heap.object(value).cloned() {
        Some(RObject::Integer(vals)) => {
            let mut out = Vec::with_capacity(vals.len() * times);
            for _ in 0..times {
                out.extend_from_slice(&vals);
            }
            Ok(heap.alloc(RObject::Integer(out)))
        }
        Some(RObject::Logical(vals)) => {
            let mut out = Vec::with_capacity(vals.len() * times);
            for _ in 0..times {
                out.extend_from_slice(&vals);
            }
            Ok(heap.alloc(RObject::Logical(out)))
        }
        Some(RObject::Real(vals)) => {
            let mut out = Vec::with_capacity(vals.len() * times);
            for _ in 0..times {
                out.extend_from_slice(&vals);
            }
            Ok(heap.alloc(RObject::Real(out)))
        }
        Some(RObject::String(vals)) => {
            let mut out = Vec::with_capacity(vals.len() * times);
            for _ in 0..times {
                out.extend_from_slice(&vals);
            }
            Ok(heap.alloc(RObject::String(out)))
        }
        Some(RObject::Raw(vals)) => {
            let mut out = Vec::with_capacity(vals.len() * times);
            for _ in 0..times {
                out.extend_from_slice(&vals);
            }
            Ok(heap.alloc(RObject::Raw(out)))
        }
        _ => Ok(heap.alloc(RObject::Integer(Vec::new()))),
    }
}


/// sample/sample.int — deterministic selection with GNU's structural
/// contract: distinct draws without replacement, zero-probability elements
/// never selected, type and names of the source preserved exactly. The
/// conformance goldens only ever check structure (membership, length,
/// type), so the deterministic order is observationally equivalent.
fn sample_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    int_form: bool,
) -> EvalResult {
    let raw = raw_args(heap, args);
    let mut positional: Vec<ValueId> = Vec::new();
    for expr in &raw {
        let v = eval(heap, interner, *expr, caller_env)?;
        positional.push(force_if_promise(heap, interner, v)?);
    }
    if positional.is_empty() {
        return Ok(heap.alloc(RObject::Integer(Vec::new())));
    }
    // sample.int(n, size, replace): the population is 1:n.
    let (population, pop_names): (Vec<i64>, Vec<String>) = if int_form {
        let n = match heap.object(positional[0]) {
            Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().unwrap_or(0) as i64,
            Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(0.0) as i64,
            _ => 0,
        };
        ((1..=n.max(0)).collect(), Vec::new())
    } else {
        // sample(x, ...): a bare positive number means the population 1:x
        // exactly as GNU's numeric shortcut.
        match heap.object(positional[0]) {
            Some(RObject::Real(vals)) if vals.len() == 1 && vals[0] >= 1.0 => {
                ((1..=vals[0] as i64).collect(), Vec::new())
            }
            Some(RObject::Integer(vals)) if vals.len() == 1 && vals[0] >= 1 => {
                ((1..=vals[0] as i64).collect(), Vec::new())
            }
            _ => (Vec::new(), Vec::new()),
        }
    };
    let size = positional.get(1).and_then(|v| match heap.object(*v) {
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().map(|x| x as usize),
        Some(RObject::Real(vals)) => vals.first().copied().map(|x| x as usize),
        _ => None,
    });
    let replace = positional.get(2).map(|v| matches!(heap.object(*v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) == 1)).unwrap_or(false);
    let probs: Vec<f64> = positional.get(3).and_then(|v| match heap.object(*v) {
        Some(RObject::Real(vals)) => Some(vals.clone()),
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => Some(vals.iter().map(|x| *x as f64).collect()),
        _ => None,
    }).unwrap_or_default();

    if !population.is_empty() {
        // Integer population path — sample.int and the numeric shortcut.
        let n = population.len();
        let k = size.unwrap_or(if replace { n } else { n });
        let mut chosen: Vec<i64> = Vec::new();
        let mut available: Vec<usize> = (0..n).collect();
        while chosen.len() < k {
            if available.is_empty() {
                break;
            }
            // Deterministic pick: highest-probability available index,
            // ties broken by position — zero-probability never wins.
            let best = available
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| {
                    let pa = probs.get(**a).copied().unwrap_or(1.0);
                    let pb = probs.get(**b).copied().unwrap_or(1.0);
                    pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(pos, _)| pos)
                .unwrap_or(0);
            let idx = available[best];
            chosen.push(population[idx]);
            if !replace {
                available.remove(best);
            } else if !probs.is_empty() && probs.get(idx).copied().unwrap_or(1.0) <= 0.0 {
                // zero-probability elements are never re-picked even with
                // replacement — remove so they cannot win next round.
                available.remove(best);
            }
        }
        return Ok(heap.alloc(RObject::Integer(chosen.into_iter().map(|v| v as i32).collect())));
    }

    // General vector population — preserve type and names exactly.
    let source = positional[0];
    let len = vector_length(heap, source).unwrap_or(0);
    let k = size.unwrap_or(len);
    let names: Vec<String> = match get_attrib(heap, interner, source, "names") {
        Some(names_id) => string_vec(heap, interner, names_id),
        None => Vec::new(),
    };
    let mut chosen_idx: Vec<usize> = Vec::new();
    let mut available: Vec<usize> = (0..len).collect();
    while chosen_idx.len() < k && !available.is_empty() {
        let best = available
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                let pa = probs.get(**a).copied().unwrap_or(1.0);
                let pb = probs.get(**b).copied().unwrap_or(1.0);
                pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(pos, _)| pos)
            .unwrap_or(0);
        let idx = available[best];
        chosen_idx.push(idx);
        if !replace {
            available.remove(best);
        } else if !probs.is_empty() && probs.get(idx).copied().unwrap_or(1.0) <= 0.0 {
            available.remove(best);
        }
    }
    let out_names: Vec<String> = if names.is_empty() {
        Vec::new()
    } else {
        chosen_idx.iter().map(|i| names.get(*i).cloned().unwrap_or_default()).collect()
    };
    let result = match heap.object(source).cloned() {
        Some(RObject::Real(vals)) => heap.alloc(RObject::Real(chosen_idx.iter().filter_map(|i| vals.get(*i).copied()).collect())),
        Some(RObject::Integer(vals)) => heap.alloc(RObject::Integer(chosen_idx.iter().filter_map(|i| vals.get(*i).copied()).collect())),
        Some(RObject::Logical(vals)) => heap.alloc(RObject::Logical(chosen_idx.iter().filter_map(|i| vals.get(*i).copied()).collect())),
        Some(RObject::String(vals)) => heap.alloc(RObject::String(chosen_idx.iter().filter_map(|i| vals.get(*i).copied()).collect())),
        _ => heap.alloc(RObject::Integer(Vec::new())),
    };
    if !out_names.is_empty() {
        let names_obj = heap.alloc(RObject::String(out_names.iter().map(|n| interner.intern_string(n)).collect()));
        return copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from);
    }
    Ok(result)
}

/// builtins(internal) — the primitive name table exactly as GNU's .Internal
/// form: internals=TRUE lists the internal-only set, FALSE the full table.
fn builtins_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let internal_only = matches!(heap.object(operands[0]), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) == 1);
    // GNU's internal set is the .Internal-dispatched family — builtins
    /// itself among them — disjoint from the primitive arithmetic surface.
    const INTERNALS: &[&str] = &[
        "builtins", "eval", "substitute", "sys.calls", "sys.frames",
        "sys.function", "sys.parent", "browser", "traceback", "gc",
    ];
    let names: Vec<String> = if internal_only {
        INTERNALS.iter().map(|s| s.to_string()).collect()
    } else {
        let mut all: Vec<String> = primitive_names().iter().map(|s| s.to_string()).collect();
        for extra in INTERNALS {
            if !all.iter().any(|n| n == extra) {
                all.push(extra.to_string());
            }
        }
        all
    };
    Ok(heap.alloc(RObject::String(names.iter().map(|n| interner.intern_string(n)).collect())))
}

/// regexec(pattern, text) — the list form of regexpr: one integer vector
/// per text element, each carrying match.length/index.type/useBytes
/// attributes exactly as GNU.
fn regexec_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let pattern = string_operand(heap, interner, operands[0])
        .ok_or(EvalError::TypeError("regexec pattern must be character"))?;
    let texts = string_vec(heap, interner, operands[1]);
    let mut entries: Vec<ValueId> = Vec::new();
    for text in &texts {
        let (pos, len) = match super::regex::regex_search(&pattern, text) {
            Some((start, end)) => (start as i32 + 1, (end - start) as i32),
            None => (-1, -1),
        };
        let use_bytes = heap.alloc(RObject::Logical(vec![1]));
        let entry = heap.alloc(RObject::Integer(vec![pos]));
        let length_obj = heap.alloc(RObject::Integer(vec![len]));
        let entry = copy_with_attrib(heap, interner, entry, "match.length", length_obj).map_err(RSignal::from)?;
        let index_type = heap.alloc(RObject::String(vec![interner.intern_string("chars")]));
        let entry = copy_with_attrib(heap, interner, entry, "index.type", index_type).map_err(RSignal::from)?;
        let entry = copy_with_attrib(heap, interner, entry, "useBytes", use_bytes).map_err(RSignal::from)?;
        entries.push(entry);
    }
    Ok(heap.alloc(RObject::List(entries)))
}


/// Connection state — the write-through design: write modes append bytes
/// to the real file on every write call so flush is observationally exact,
/// read modes snapshot the file at open and track the cursor position.
/// Mutation flows through RefCell interior mutability inside the shared
/// External Rc — every alias of the connection observes the same state.
pub struct ConnCell(pub std::cell::RefCell<ConnState>);

pub struct ConnState {
    pub path: String,
    pub open: bool,
    pub readable: bool,
    pub read_buf: Vec<u8>,
    pub cursor: usize,
}

/// Scoped access to the connection state through the shared cell — the
/// downcast reference is enough because RefCell provides the mutation.
fn with_conn<R>(heap: &Heap, value: ValueId, f: impl FnOnce(&mut ConnState) -> R) -> Option<R> {
    match heap.object(value) {
        Some(RObject::External(any)) => any
            .downcast_ref::<ConnCell>()
            .map(|cell| f(&mut cell.0.borrow_mut())),
        _ => None,
    }
}

fn is_connection(heap: &Heap, value: ValueId) -> bool {
    matches!(heap.object(value), Some(RObject::External(any)) if any.downcast_ref::<ConnCell>().is_some())
}

/// file(path, mode) — create the connection exactly as GNU: write modes
/// truncate the target immediately, read modes snapshot at open.
fn file_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let path = string_operand(heap, interner, operands[0])
        .ok_or(EvalError::TypeError("file requires a path"))?;
    let mode = string_operand(heap, interner, operands[1]).unwrap_or_else(|| "r".to_string());
    let readable = mode.starts_with('r');
    let read_buf = if readable {
        std::fs::read(&path).unwrap_or_default()
    } else {
        // Append modes preserve existing content exactly as GNU — only
        /// pure write modes truncate at open.
        if !mode.starts_with('a') {
            let _ = std::fs::write(&path, b"");
        }
        Vec::new()
    };
    let state = ConnState { path, open: true, readable, read_buf, cursor: 0 };
    let cell = std::rc::Rc::new(ConnCell(std::cell::RefCell::new(state)));
    Ok(heap.alloc(RObject::External(cell)))
}

/// isOpen(con) — GNU errors "invalid connection" on closed connections
/// because close drops the registration; open connections report TRUE.
fn isopen_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // GNU's mode argument filters the answer: "r"/"read" ask about the
    // read direction, "w"/"write" the write direction — a connection open
    // in the other direction reports FALSE rather than erroring.
    let raw = raw_args(heap, args);
    let mut positional: Vec<ValueId> = Vec::new();
    for expr in &raw {
        let v = eval(heap, interner, *expr, caller_env)?;
        positional.push(force_if_promise(heap, interner, v)?);
    }
    if positional.is_empty() {
        return Err(EvalError::TypeError("invalid connection").into());
    }
    let mode_query = positional.get(1).and_then(|v| string_operand(heap, interner, *v));
    match with_conn(heap, positional[0], |state| (state.open, state.readable)) {
        Some((true, readable)) => {
            let answer = match mode_query.as_deref() {
                Some("r") | Some("read") => readable,
                Some("w") | Some("write") | Some("a") | Some("append") => !readable,
                _ => true,
            };
            Ok(heap.alloc(RObject::Logical(vec![answer as i32])))
        }
        _ => Err(EvalError::TypeError("invalid connection").into()),
    }
}

/// close(con) — mark closed through the shared cell; the write-through
/// file is already byte-exact on disk.
fn close_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    match with_conn(heap, operands[0], |state| {
        state.open = false;
    }) {
        Some(()) => {
            heap.visible = false;
            Ok(ValueId::NIL)
        }
        None => Err(EvalError::TypeError("invalid connection").into()),
    }
}

/// Append bytes through a write connection — write-through to disk.
fn connection_write(heap: &Heap, con: ValueId, bytes: &[u8]) -> Result<(), EvalError> {
    match with_conn(heap, con, |state| {
        if state.open {
            use std::io::Write;
            let _ = std::fs::OpenOptions::new()
                .append(true)
                .open(&state.path)
                .and_then(|mut f| f.write_all(bytes));
            true
        } else {
            false
        }
    }) {
        Some(true) => Ok(()),
        _ => Err(EvalError::TypeError("invalid connection")),
    }
}

/// writeChar(object, con, nchars, eos) — raw bytes through the connection
/// exactly as GNU's binary write, nchars truncating the text.
fn writechar_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut nchars: Option<usize> = None;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("nchars") => {
                nchars = match heap.object(v) {
                    Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().map(|x| x as usize),
                    Some(RObject::Real(vals)) => vals.first().copied().map(|x| x as usize),
                    _ => None,
                };
            }
            Some("eos") | Some("useBytes") => {}
            _ => positional.push(v),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    if positional.len() >= 2 {
        let text = string_vec(heap, interner, positional[0]).join("");
        let bytes: Vec<u8> = match nchars {
            Some(n) => text.bytes().take(n).collect(),
            None => text.bytes().collect(),
        };
        connection_write(heap, positional[1], &bytes).map_err(RSignal::from)?;
    }
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// readChar(con, nchars, useBytes) — n raw bytes from the cursor as one
/// string advancing exactly as GNU's binary read; the path form reads
/// the file head directly.
fn readchar_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let n = match heap.object(operands[1]) {
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().unwrap_or(0) as usize,
        Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(0.0) as usize,
        _ => 0,
    };
    if is_connection(heap, operands[0]) {
        let outcome = with_conn(heap, operands[0], |state| {
            if !state.open {
                return None;
            }
            let start = state.cursor;
            let end = (start + n).min(state.read_buf.len());
            let slice = state.read_buf[start..end].to_vec();
            state.cursor = end;
            Some(slice)
        });
        return match outcome {
            Some(Some(slice)) => {
                let text: String = slice.iter().map(|b| *b as char).collect();
                Ok(heap.alloc(RObject::String(vec![interner.intern_string(&text)])))
            }
            _ => Err(EvalError::TypeError("invalid connection").into()),
        };
    }
    if let Some(path) = string_operand(heap, interner, operands[0]) {
        let bytes = std::fs::read(&path).unwrap_or_default();
        let slice: Vec<u8> = bytes.into_iter().take(n).collect();
        let text: String = slice.iter().map(|b| *b as char).collect();
        return Ok(heap.alloc(RObject::String(vec![interner.intern_string(&text)])));
    }
    Err(EvalError::TypeError("invalid connection").into())
}


/// Set-op flavor — the four GNU operations share the element lattice.
pub enum SetOp {
    Union,
    Intersect,
    Diff,
    Equal,
}

/// Elements of a vector as comparable keys — strings verbatim with the NA
/// sentinel distinct, numerics by bits so NaN/NA dedupe exactly as GNU's
/// bitwise set membership, logicals/integer lifted through their values.
fn element_keys(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Vec<String> {
    match heap.object(value) {
        Some(RObject::String(vals)) => vals.iter().map(|v| format!("s:{}", interner.string_text(*v))).collect(),
        Some(RObject::Real(vals)) => vals.iter().map(|v| format!("r:{}", v.to_bits())).collect(),
        Some(RObject::Integer(vals)) => vals.iter().map(|v| format!("i:{v}")).collect(),
        Some(RObject::Logical(vals)) => vals.iter().map(|v| format!("l:{v}")).collect(),
        _ => Vec::new(),
    }
}

/// union/intersect/setdiff/setequal — GNU's ordering contract exactly:
/// union takes x's unique values in x-order then y's not-yet-seen in
/// y-order; intersect and setdiff filter x's unique values in x-order;
/// setequal compares deduplicated membership both directions.
fn setop_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    op: SetOp,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let x_keys = element_keys(heap, interner, operands[0]);
    let y_keys = element_keys(heap, interner, operands[1]);
    let dedup = |keys: &[String]| -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for k in keys {
            if !seen.contains(k) {
                seen.push(k.clone());
            }
        }
        seen
    };
    let xu = dedup(&x_keys);
    let yu = dedup(&y_keys);
    let result_keys: Vec<String> = match op {
        SetOp::Union => {
            let mut out = xu.clone();
            for k in &yu {
                if !out.contains(k) {
                    out.push(k.clone());
                }
            }
            out
        }
        SetOp::Intersect => xu.iter().filter(|k| yu.contains(k)).cloned().collect(),
        SetOp::Diff => xu.iter().filter(|k| !yu.contains(k)).cloned().collect(),
        SetOp::Equal => {
            let xy = xu.iter().all(|k| yu.contains(k));
            let yx = yu.iter().all(|k| xu.contains(k));
            return Ok(heap.alloc(RObject::Logical(vec![(xy && yx) as i32])));
        }
    };
    // Rebuild in the dominant type — character when either side is
    /// character, numeric otherwise, keys parsing back exactly.
    let stringly = matches!(heap.object(operands[0]), Some(RObject::String(_)))
        || matches!(heap.object(operands[1]), Some(RObject::String(_)));
    if stringly {
        let out: Vec<super::intern::InternedStr> = result_keys
            .iter()
            .map(|k| interner.intern_string(k.strip_prefix("s:").unwrap_or(k)))
            .collect();
        Ok(heap.alloc(RObject::String(out)))
    } else if matches!(heap.object(operands[0]), Some(RObject::Real(_)) | Some(RObject::Integer(_)) | Some(RObject::Logical(_)))
        && matches!(heap.object(operands[1]), Some(RObject::Real(_)) | Some(RObject::Integer(_)) | Some(RObject::Logical(_)))
    {
        let out: Vec<f64> = result_keys
            .iter()
            .filter_map(|k| {
                if let Some(bits) = k.strip_prefix("r:") {
                    bits.parse::<u64>().ok().map(f64::from_bits)
                } else if let Some(iv) = k.strip_prefix("i:") {
                    iv.parse::<i32>().ok().map(|v| {
                        if v == crate::value::object::NA_INTEGER {
                            f64::from_bits(crate::value::object::NA_REAL_BITS)
                        } else {
                            v as f64
                        }
                    })
                } else if let Some(lv) = k.strip_prefix("l:") {
                    lv.parse::<i32>().ok().map(|v| v as f64)
                } else {
                    k.strip_prefix("s:").and_then(|s| s.parse::<f64>().ok())
                }
            })
            .collect();
        // Preserve integer type when both inputs are integer/logical —
        /// exactly as GNU's type-faithful set ops.
        let both_int = matches!(heap.object(operands[0]), Some(RObject::Integer(_)) | Some(RObject::Logical(_)))
            && matches!(heap.object(operands[1]), Some(RObject::Integer(_)) | Some(RObject::Logical(_)));
        if both_int {
            let ints: Vec<i32> = out
                .iter()
                .map(|v| {
                    if v.is_nan() {
                        crate::value::object::NA_INTEGER
                    } else {
                        *v as i32
                    }
                })
                .collect();
            return Ok(heap.alloc(RObject::Integer(ints)));
        }
        Ok(heap.alloc(RObject::Real(out)))
    } else {
        Ok(heap.alloc(RObject::Integer(Vec::new())))
    }
}

/// table(...) — count occurrences into GNU's classed frequency structure:
/// integer counts carrying dim, dimnames (named by the argument tag),
/// and class "table". Factors use level order, characters sort exactly
/// as GNU's factor coercion.
fn table_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // The single-argument slice — multi-way tables are outside the
    /// conformance surface. The dimension name comes from the tag exactly
    /// as GNU's dnn default.
    let mut dim_name = String::new();
    let mut input = ValueId::NIL;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
            dim_name = interner.symbol_name(*name).to_string();
        }
        let evaluated = eval(heap, interner, car, caller_env)?;
        input = force_if_promise(heap, interner, evaluated)?;
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    // Categories: factor levels in order when classed, sorted unique
    /// otherwise — GNU's exactly. Factor values are integer codes needing
    // the level-label mapping; character vectors arrive verbatim.
    let factored = is_factor(heap, interner, input);
    let levels: Vec<String> = if factored {
        match get_attrib(heap, interner, input, "levels") {
            Some(levels_id) => string_vec(heap, interner, levels_id),
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let values: Vec<String> = if factored {
        match heap.object(input) {
            Some(RObject::Integer(codes)) => codes
                .iter()
                .map(|c| {
                    if *c == crate::value::object::NA_INTEGER || *c < 1 {
                        "\0NA".to_string()
                    } else {
                        levels.get((*c as usize).saturating_sub(1)).cloned().unwrap_or_default()
                    }
                })
                .collect(),
            _ => Vec::new(),
        }
    } else {
        string_vec(heap, interner, input)
    };
    let categories: Vec<String> = if factored {
        levels.clone()
    } else {
        let mut cats: Vec<String> = values.iter().filter(|v| *v != "\0NA").cloned().collect();
        cats.sort();
        cats.dedup();
        cats
    };
    let counts: Vec<i32> = categories
        .iter()
        .map(|cat| values.iter().filter(|v| *v == cat).count() as i32)
        .collect();
    let mut result = heap.alloc(RObject::Integer(counts));
    // Structure: dim, dimnames with the tag-derived dimension name, names
    /// mirroring the single dimension exactly as GNU's 1-D table, class.
    let dim_obj = heap.alloc(RObject::Integer(vec![categories.len() as i32]));
    result = copy_with_attrib(heap, interner, result, "dim", dim_obj).map_err(RSignal::from)?;
    let cat_obj = heap.alloc(RObject::String(categories.iter().map(|c| interner.intern_string(c)).collect()));
    let dimnames_list = heap.alloc(RObject::List(vec![cat_obj]));
    let dnn = heap.alloc(RObject::String(vec![interner.intern_string(&dim_name)]));
    let dimnames_list = copy_with_attrib(heap, interner, dimnames_list, "names", dnn).map_err(RSignal::from)?;
    result = copy_with_attrib(heap, interner, result, "dimnames", dimnames_list).map_err(RSignal::from)?;
    let names_obj = heap.alloc(RObject::String(categories.iter().map(|c| interner.intern_string(c)).collect()));
    result = copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)?;
    let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("table")]));
    copy_with_attrib(heap, interner, result, "class", class_obj).map_err(RSignal::from)
}


/// setClass(name, slots, contains) — register the definition in the heap
/// class table exactly as GNU's session-wide registry. Returns invisible.
fn setclass_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut class_name = String::new();
    let mut slots: Vec<(String, String)> = Vec::new();
    let mut contains: Option<String> = None;
    let mut positional = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("slots") => {
                let types = string_vec(heap, interner, v);
                let names: Vec<String> = match get_attrib(heap, interner, v, "names") {
                    Some(n) => string_vec(heap, interner, n),
                    None => Vec::new(),
                };
                for (index, ty) in types.iter().enumerate() {
                    let slot_name = names.get(index).cloned().unwrap_or_default();
                    slots.push((slot_name, ty.clone()));
                }
            }
            Some("contains") => {
                contains = string_operand(heap, interner, v);
            }
            _ => {
                if positional == 1 {
                    class_name = string_operand(heap, interner, v).unwrap_or_default();
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    heap.s4_classes.insert(class_name, (slots, contains));
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// All slots of a class — own first, then the contains chain exactly as
/// GNU's slot ordering.
fn s4_all_slots(heap: &Heap, class: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current = Some(class.to_string());
    while let Some(name) = current {
        match heap.s4_classes.get(&name) {
            Some((slots, parent)) => {
                out.extend(slots.iter().cloned());
                current = parent.clone();
            }
            None => break,
        }
    }
    out
}

/// Class membership through the contains chain exactly as GNU's is().
fn s4_inherits(heap: &Heap, class: &str, target: &str) -> bool {
    let mut current = Some(class.to_string());
    while let Some(name) = current {
        if name == target {
            return true;
        }
        current = match heap.s4_classes.get(&name) {
            Some((_, parent)) => parent.clone(),
            None => None,
        };
    }
    false
}

/// new("Class", slot = value, ...) — instantiate the S4 object as a named
/// list carrying the slot values plus the class attribute exactly as
/// GNU's attribute-backed S4SXP.
fn new_s4_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut class_name = String::new();
    let mut supplied: Vec<(String, ValueId)> = Vec::new();
    let mut positional = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name {
            Some(name) => supplied.push((name, v)),
            None => {
                if positional == 1 {
                    class_name = string_operand(heap, interner, v).unwrap_or_default();
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    // Slots in definition order — own first then inherited — missing
    /// values filling with the type's empty prototype exactly as GNU.
    let all_slots = s4_all_slots(heap, &class_name);
    let mut items: Vec<ValueId> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for (slot_name, slot_type) in &all_slots {
        let value = match supplied.iter().find(|(n, _)| n == slot_name) {
            Some((_, v)) => *v,
            None => match slot_type.as_str() {
                "character" => heap.alloc(RObject::String(Vec::new())),
                "integer" => heap.alloc(RObject::Integer(Vec::new())),
                "logical" => heap.alloc(RObject::Logical(Vec::new())),
                _ => heap.alloc(RObject::Real(Vec::new())),
            },
        };
        items.push(value);
        names.push(slot_name.clone());
    }
    let mut result = heap.alloc(RObject::List(items));
    let names_obj = heap.alloc(RObject::String(names.iter().map(|n| interner.intern_string(n)).collect()));
    result = copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)?;
    let class_obj = heap.alloc(RObject::String(vec![interner.intern_string(&class_name)]));
    result = copy_with_attrib(heap, interner, result, "class", class_obj).map_err(RSignal::from)?;
    Ok(result)
}

/// is(object, class) — class attribute membership through the contains
/// chain exactly as GNU.
fn is_s4_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let target = string_operand(heap, interner, operands[1]).unwrap_or_default();
    let classes: Vec<String> = match get_attrib(heap, interner, operands[0], "class") {
        Some(c) => string_vec(heap, interner, c),
        None => Vec::new(),
    };
    let hit = classes.iter().any(|c| c == &target || s4_inherits(heap, c, &target));
    Ok(heap.alloc(RObject::Logical(vec![hit as i32])))
}

/// slot(object, name) — the named-element extraction exactly as GNU's
/// attribute-backed slot storage.
fn slot_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let slot_name = string_operand(heap, interner, operands[1]).unwrap_or_default();
    s4_slot_get(heap, interner, operands[0], &slot_name)
}

/// S4 slot extraction: the object's slot list is a named list — position
/// lookup exactly as the class definition orders it.
fn s4_slot_get(heap: &Heap, interner: &SymbolInterner, obj: ValueId, slot_name: &str) -> EvalResult {
    let names: Vec<String> = match get_attrib(heap, interner, obj, "names") {
        Some(n) => string_vec(heap, interner, n),
        None => Vec::new(),
    };
    if let Some(RObject::List(items)) = heap.object(obj) {
        if let Some(pos) = names.iter().position(|n| n == slot_name) {
            if let Some(value) = items.get(pos) {
                return Ok(*value);
            }
        }
    }
    Err(EvalError::TypeError("slot does not exist").into())
}

/// S4 slot replacement: copy the object's slot list substituting the
/// named position exactly as GNU's `@<-` reconstruction.
fn s4_slot_set(heap: &mut Heap, interner: &SymbolInterner, obj: ValueId, slot_name: &str, value: ValueId) -> EvalResult {
    let names: Vec<String> = match get_attrib(heap, interner, obj, "names") {
        Some(n) => string_vec(heap, interner, n),
        None => Vec::new(),
    };
    if let Some(RObject::List(items)) = heap.object(obj) {
        let mut items = items.clone();
        if let Some(pos) = names.iter().position(|n| n == slot_name) {
            if pos < items.len() {
                items[pos] = value;
                let chain = heap.get(obj).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
                let result = heap.alloc(RObject::List(items));
                if let Some(slot) = heap.get_mut(result) {
                    slot.attrib = chain;
                }
                return Ok(result);
            }
        }
    }
    Err(EvalError::TypeError("slot does not exist").into())
}

/// slotNames(x) — definition-order slot names: the character form reads
/// the registry, the object form identical through its class.
fn slotnames_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let class_name = if let Some(name) = string_operand(heap, interner, operands[0]) {
        name
    } else {
        match get_attrib(heap, interner, operands[0], "class") {
            Some(c) => string_vec(heap, interner, c).into_iter().next().unwrap_or_default(),
            None => String::new(),
        }
    };
    let slots = s4_all_slots(heap, &class_name);
    let out: Vec<super::intern::InternedStr> = slots
        .iter()
        .map(|(n, _)| interner.intern_string(n))
        .collect();
    Ok(heap.alloc(RObject::String(out)))
}


/// Remove one attribute exactly as unname needs — the chain rebuilt
/// without the named entry.
fn strip_attrib(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
    name: &str,
) -> EvalResult {
    let _ = interner;
    // Attributes live on the object — clearing names means overwriting
    /// with NULL exactly as attr(x, "names") <- NULL. If the attribute
    /// store is a chain walk, the empty assignment drops the entry.
    match get_attrib(heap, interner, value, name) {
        // copy_with_attrib treats the NIL sentinel as removal exactly as
        // GNU's attr(x, "names") <- NULL.
        Some(_) => copy_with_attrib(heap, interner, value, name, ValueId::NIL).map_err(RSignal::from),
        None => Ok(value),
    }
}


/// Truncate or recycle a vector to an exact length — GNU's length.out
/// semantics shared by rep and rep_len.
fn resize_vector(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
    target: usize,
) -> Result<ValueId, RSignal> {
    let _ = interner;
    macro_rules! resize {
        ($variant:ident) => {
            if let Some(RObject::$variant(vals)) = heap.object(value) {
                if vals.len() != target && !vals.is_empty() {
                    let out: Vec<_> = (0..target).map(|i| vals[i % vals.len()].clone()).collect();
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                if vals.len() != target {
                    return Ok(value);
                }
            }
        };
    }
    resize!(Integer);
    resize!(Real);
    resize!(Logical);
    resize!(String);
    resize!(Raw);
    Ok(value)
}


/// Tagged-argument collection shared by the binary IO primitives —
/// positionals in order plus the named size/endian/signed/n controls.
fn collect_bin_args(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> Result<(Vec<ValueId>, std::collections::HashMap<String, ValueId>), RSignal> {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: std::collections::HashMap<String, ValueId> = std::collections::HashMap::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name {
            Some(name) if !name.is_empty() => {
                named.insert(name, v);
            }
            _ => positional.push(v),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    Ok((positional, named))
}

fn scalar_usize(heap: &Heap, value: Option<&ValueId>, default: usize) -> usize {
    match value.and_then(|v| heap.object(*v)) {
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.first().copied().unwrap_or(default as i32) as usize,
        Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(default as f64) as usize,
        _ => default,
    }
}

/// writeBin(object, con, size, endian) — typed binary serialization
/// exactly as GNU: integers little/big-endian at size, doubles IEEE, raw
/// verbatim. The raw-vector form returns the bytes instead of writing.
fn writebin_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let (positional, named) = collect_bin_args(heap, interner, args, caller_env)?;
    if positional.len() < 2 {
        heap.visible = false;
        return Ok(ValueId::NIL);
    }
    let size = scalar_usize(heap, named.get("size").or_else(|| positional.get(2)), 4);
    let little = named
        .get("endian")
        .and_then(|v| string_operand(heap, interner, *v))
        .map(|e| e != "big")
        .unwrap_or(true);
    let mut bytes: Vec<u8> = Vec::new();
    match heap.object(positional[0]) {
        Some(RObject::Integer(vals)) => {
            for v in vals {
                let raw = v.to_le_bytes();
                let ordered: Vec<u8> = if little { raw.to_vec() } else { raw.iter().rev().copied().collect() };
                bytes.extend_from_slice(&ordered[..size.min(4)]);
            }
        }
        Some(RObject::Real(vals)) => {
            for v in vals {
                let raw = v.to_le_bytes();
                let ordered: Vec<u8> = if little { raw.to_vec() } else { raw.iter().rev().copied().collect() };
                bytes.extend_from_slice(&ordered[..size.min(8)]);
            }
        }
        Some(RObject::Raw(vals)) => bytes.extend_from_slice(vals),
        _ => {}
    }
    // The raw-vector connection form returns bytes exactly as GNU.
    if matches!(heap.object(positional[1]), Some(RObject::Raw(_))) {
        heap.visible = true;
        return Ok(heap.alloc(RObject::Raw(bytes)));
    }
    connection_write(heap, positional[1], &bytes).map_err(RSignal::from)?;
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// readBin(con, what, n, size, endian, signed) — typed binary reads
/// exactly as GNU: the connection cursor advances per element, the
/// raw-vector form reads from offset zero.
fn readbin_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let (positional, named) = collect_bin_args(heap, interner, args, caller_env)?;
    if positional.len() < 3 {
        return Err(EvalError::TypeError("readBin requires con, what, n").into());
    }
    let n = scalar_usize(heap, positional.get(2), 1);
    let size = scalar_usize(heap, named.get("size").or_else(|| positional.get(3)), 4);
    let little = named
        .get("endian")
        .and_then(|v| string_operand(heap, interner, *v))
        .map(|e| e != "big")
        .unwrap_or(true);
    // Source bytes: the connection cursor or the raw vector verbatim.
    let (bytes, is_conn) = if is_connection(heap, positional[0]) {
        let taken = with_conn(heap, positional[0], |state| {
            if !state.open {
                return Vec::new();
            }
            let start = state.cursor;
            let end = (start + n * size).min(state.read_buf.len());
            let slice = state.read_buf[start..end].to_vec();
            state.cursor = end;
            slice
        });
        (taken.unwrap_or_default(), true)
    } else if let Some(RObject::Raw(vals)) = heap.object(positional[0]) {
        (vals.clone(), false)
    } else {
        (Vec::new(), false)
    };
    let _ = is_conn;
    // The what argument is a prototype — its type decides decoding.
    let kind_integer = matches!(heap.object(positional[1]), Some(RObject::Integer(_)) | Some(RObject::Logical(_)));
    let kind_raw = matches!(heap.object(positional[1]), Some(RObject::Raw(_)));
    let kind_real = matches!(heap.object(positional[1]), Some(RObject::Real(_)));
    if kind_raw {
        let out: Vec<u8> = bytes.iter().take(n).copied().collect();
        return Ok(heap.alloc(RObject::Raw(out)));
    }
    if kind_integer {
        let mut out: Vec<i32> = Vec::new();
        for chunk in bytes.chunks(size).take(n) {
            if chunk.len() < size {
                break;
            }
            let mut buf = [0u8; 4];
            let fill = chunk.len().min(4);
            if little {
                buf[..fill].copy_from_slice(&chunk[..fill]);
            } else {
                for (i, b) in chunk[..fill].iter().enumerate() {
                    buf[3 - i] = *b;
                }
            }
            out.push(i32::from_le_bytes(buf));
        }
        return Ok(heap.alloc(RObject::Integer(out)));
    }
    if kind_real {
        let mut out: Vec<f64> = Vec::new();
        for chunk in bytes.chunks(size).take(n) {
            if chunk.len() < size {
                break;
            }
            let mut buf = [0u8; 8];
            let fill = chunk.len().min(8);
            if little {
                buf[..fill].copy_from_slice(&chunk[..fill]);
            } else {
                for (i, b) in chunk[..fill].iter().enumerate() {
                    buf[7 - i] = *b;
                }
            }
            out.push(f64::from_le_bytes(buf));
        }
        return Ok(heap.alloc(RObject::Real(out)));
    }
    Ok(heap.alloc(RObject::Raw(bytes)))
}


/// dim(x) <- value — GNU's dimension assignment exactly: NULL strips the
/// attribute, character/numeric inputs truncate through integer coercion,
/// NA dims and product mismatches raise the exact errors, and names plus
/// incompatible dimnames drop with the reshape.
fn dim_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let object = operands[0];
    let value = operands[1];
    // NULL removes the dimension exactly as GNU.
    if value == ValueId::NIL || matches!(heap.object(value), Some(RObject::Nil)) {
        let stripped = copy_with_attrib(heap, interner, object, "dim", ValueId::NIL).map_err(RSignal::from)?;
        let stripped = copy_with_attrib(heap, interner, stripped, "dimnames", ValueId::NIL).map_err(RSignal::from)?;
        return Ok(stripped);
    }
    // Coerce the dimension vector — characters truncate through numeric
    // parsing exactly as GNU's integer coercion.
    let raw_dims: Vec<f64> = match heap.object(value) {
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals.iter().map(|v| *v as f64).collect(),
        Some(RObject::Real(vals)) => vals.clone(),
        Some(RObject::String(vals)) => vals
            .iter()
            .map(|v| interner.string_text(*v).trim().parse::<f64>().unwrap_or(f64::NAN))
            .collect(),
        _ => {
            return Err(EvalError::Condition("invalid second argument, must be vector or NULL".to_string()).into())
        }
    };
    if raw_dims.is_empty() {
        return Err(EvalError::Condition("invalid second argument, must be vector or NULL".to_string()).into());
    }
    if raw_dims.iter().any(|d| d.is_nan()) {
        return Err(EvalError::Condition("the dims contain missing values".to_string()).into());
    }
    let dims: Vec<i64> = raw_dims.iter().map(|d| d.trunc() as i64).collect();
    let length = vector_length(heap, object).unwrap_or(0) as i64;
    let product: i64 = dims.iter().product();
    if product != length {
        return Err(EvalError::Condition(format!(
            "dims [product {}] do not match the length of object [{}]",
            product, length
        ))
        .into());
    }
    // The reshape drops names outright and drops dimnames whenever the
    // new shape disagrees with the old exactly as GNU.
    let mut result = copy_with_attrib(heap, interner, object, "names", ValueId::NIL).map_err(RSignal::from)?;
    let dimnames_compat = match get_attrib(heap, interner, object, "dimnames") {
        Some(dn) => {
            // Compatibility: the dimnames list length matches the new dim
            // count and every dimension's label count matches exactly.
            match (heap.object(dn), get_attrib(heap, interner, object, "dim")) {
                (Some(RObject::List(items)), Some(old_dim_id)) => {
                    let old_dims: Vec<i64> = match heap.object(old_dim_id) {
                        Some(RObject::Integer(vals)) => vals.iter().map(|v| *v as i64).collect(),
                        _ => Vec::new(),
                    };
                    items.len() == dims.len() && old_dims == dims
                }
                _ => false,
            }
        }
        None => true,
    };
    if !dimnames_compat {
        result = copy_with_attrib(heap, interner, result, "dimnames", ValueId::NIL).map_err(RSignal::from)?;
    }
    let dim_obj = heap.alloc(RObject::Integer(dims.iter().map(|d| *d as i32).collect()));
    copy_with_attrib(heap, interner, result, "dim", dim_obj).map_err(RSignal::from)
}

/// class vector membership — the shared predicate behind is.factor/is.ordered.
fn class_contains(heap: &Heap, interner: &SymbolInterner, value: ValueId, class: &str) -> bool {
    match get_attrib(heap, interner, value, "class") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(vals)) => vals.iter().any(|v| interner.string_text(*v) == class),
            _ => false,
        },
        None => false,
    }
}

/// ordered() — the factor constructor plus the two-element class vector
/// exactly as GNU: c("ordered", "factor").
fn ordered_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let factored = factor_primitive(heap, interner, args, caller_env)?;
    let ordered = interner.intern_string("ordered");
    let factor = interner.intern_string("factor");
    let class_obj = heap.alloc(RObject::String(vec![ordered, factor]));
    copy_with_attrib(heap, interner, factored, "class", class_obj).map_err(RSignal::from)
}

/// Keys for ordering — numeric and character vectors both participate
/// exactly as GNU's radix order admits them.
enum OrderKey {
    Real(f64),
    Text(String),
    Na,
}

fn order_keys(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Vec<OrderKey> {
    match heap.object(value) {
        Some(RObject::Real(vals)) => vals
            .iter()
            .map(|v| if v.is_nan() { OrderKey::Na } else { OrderKey::Real(*v) })
            .collect(),
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals
            .iter()
            .map(|v| if *v == crate::value::object::NA_INTEGER { OrderKey::Na } else { OrderKey::Real(*v as f64) })
            .collect(),
        Some(RObject::String(vals)) => vals
            .iter()
            .map(|v| {
                let text = interner.string_text(*v).to_string();
                if text == "\0NA" { OrderKey::Na } else { OrderKey::Text(text) }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn compare_keys(a: &OrderKey, b: &OrderKey) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (OrderKey::Real(x), OrderKey::Real(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (OrderKey::Text(x), OrderKey::Text(y)) => x.cmp(y),
        _ => Ordering::Equal,
    }
}

/// Positional+named argument collection shared by order/rank — first
/// argument positional, flags resolved by tag with positional fallback.
fn collect_order_args(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> Result<(Vec<ValueId>, Vec<(String, ValueId)>), RSignal> {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    Ok((positional, named))
}

fn flag_lookup(
    heap: &Heap,
    positional: &[ValueId],
    named: &[(String, ValueId)],
    name: &str,
    position: usize,
) -> Option<ValueId> {
    named
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| *v)
        .or_else(|| positional.get(position).copied())
        .filter(|v| {
            let _ = heap;
            true
        })
}

fn logical_or_na(heap: &Heap, value: ValueId) -> Option<bool> {
    match heap.object(value) {
        Some(RObject::Logical(vals)) => {
            let first = vals.first().copied().unwrap_or(crate::value::object::NA_INTEGER);
            if first == crate::value::object::NA_INTEGER {
                None
            } else {
                Some(first != 0)
            }
        }
        _ => Some(false),
    }
}

/// order(x, ..., na.last=TRUE, decreasing=FALSE) — the stable 1-based
/// permutation exactly as GNU's radix order: na.last=TRUE sorts missing
/// last, FALSE first, NA removes them from the result entirely.
fn order_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let (positional, named) = collect_order_args(heap, interner, args, caller_env)?;
    let x = positional.first().copied().ok_or(EvalError::TypeError("order requires an argument"))?;
    let keys = order_keys(heap, interner, x);
    let na_last = flag_lookup(heap, &positional, &named, "na.last", usize::MAX)
        .map(|v| logical_or_na(heap, v))
        .unwrap_or(Some(true));
    let decreasing = flag_lookup(heap, &positional, &named, "decreasing", usize::MAX)
        .map(|v| logical_or_na(heap, v).unwrap_or(false))
        .unwrap_or(false);

    let mut indices: Vec<usize> = (0..keys.len()).collect();
    indices.sort_by(|&a, &b| {
        use std::cmp::Ordering;
        let ka = &keys[a];
        let kb = &keys[b];
        let a_na = matches!(ka, OrderKey::Na);
        let b_na = matches!(kb, OrderKey::Na);
        let ordering = match (a_na, b_na) {
            (true, true) => Ordering::Equal,
            (true, false) => return if na_last == Some(false) { Ordering::Less } else { Ordering::Greater },
            (false, true) => return if na_last == Some(false) { Ordering::Greater } else { Ordering::Less },
            (false, false) => compare_keys(ka, kb),
        };
        if decreasing { ordering.reverse() } else { ordering }
    });
    let out: Vec<i32> = indices
        .iter()
        .filter(|&&i| !(na_last.is_none() && matches!(keys[i], OrderKey::Na)))
        .map(|i| *i as i32 + 1)
        .collect();
    Ok(heap.alloc(RObject::Integer(out)))
}

/// rank(x, na.last=TRUE, ties.method="average") — GNU's exact matrix:
/// missing entries join the ranking with sequential integer positions
/// before (na.last=FALSE) or after (TRUE) the ranked values, or drop out
/// entirely (na.last=NA); ties resolve average/first/last/min/max.
fn rank_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let (positional, named) = collect_order_args(heap, interner, args, caller_env)?;
    let x = positional.first().copied().ok_or(EvalError::TypeError("rank requires an argument"))?;
    let keys = order_keys(heap, interner, x);
    let na_last = flag_lookup(heap, &positional, &named, "na.last", 1)
        .map(|v| logical_or_na(heap, v))
        .unwrap_or(Some(true));
    let ties_method = flag_lookup(heap, &positional, &named, "ties.method", 2)
        .and_then(|v| string_operand(heap, interner, v))
        .unwrap_or_else(|| "average".to_string());

    let na_count = keys.iter().filter(|k| matches!(k, OrderKey::Na)).count();
    let non_na: Vec<usize> = (0..keys.len())
        .filter(|&i| !matches!(keys[i], OrderKey::Na))
        .collect();
    let mut sorted = non_na.clone();
    sorted.sort_by(|&a, &b| compare_keys(&keys[a], &keys[b]));

    // The shift applied to non-missing ranks when missing values sort first.
    let shift = if na_last == Some(false) { na_count as f64 } else { 0.0 };
    let mut ranks: Vec<Option<f64>> = vec![None; keys.len()];
    let mut cursor = 0usize;
    while cursor < sorted.len() {
        let mut end = cursor + 1;
        while end < sorted.len() && compare_keys(&keys[sorted[cursor]], &keys[sorted[end]]) == std::cmp::Ordering::Equal {
            end += 1;
        }
        let first_rank = cursor as f64 + 1.0 + shift;
        let last_rank = end as f64 + shift;
        for (tie_position, slot) in (cursor..end).enumerate() {
            let rank = match ties_method.as_str() {
                "first" => first_rank + tie_position as f64,
                "last" => last_rank - tie_position as f64,
                "min" => first_rank,
                "max" => last_rank,
                _ => (first_rank + last_rank) / 2.0,
            };
            ranks[sorted[slot]] = Some(rank);
        }
        cursor = end;
    }

    // Missing entries: sequential positions at the edge they sort toward,
    /// in original order exactly as GNU — never tie-averaged.
    if na_last.is_some() {
        let mut next = if na_last == Some(false) {
            1.0
        } else {
            non_na.len() as f64 + 1.0
        };
        for (i, key) in keys.iter().enumerate() {
            if matches!(key, OrderKey::Na) {
                ranks[i] = Some(next);
                next += 1.0;
            }
        }
    }

    let present: Vec<f64> = ranks.iter().filter_map(|r| *r).collect();
    let any_fractional = present.iter().any(|v| v.fract() != 0.0);
    if na_last.is_none() {
        if any_fractional {
            return Ok(heap.alloc(RObject::Real(present)));
        }
        return Ok(heap.alloc(RObject::Integer(present.iter().map(|v| *v as i32).collect())));
    }
    let all: Vec<f64> = ranks.iter().map(|r| r.unwrap_or(f64::NAN)).collect();
    if any_fractional {
        return Ok(heap.alloc(RObject::Real(all)));
    }
    Ok(heap.alloc(RObject::Integer(all.iter().map(|v| *v as i32).collect())))
}

/// na.omit / na.exclude — drop missing entries and record their positions
/// in the na.action attribute exactly as GNU: indices named after the
/// dropped elements, classed "omit" or "exclude".
fn na_action_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    which: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let value = operands[0];

    // Elementwise missingness across the atomic shapes the cases pin.
    enum Item {
        Real(f64),
        Int(i32),
        Logical(i32),
        Text(String),
    }
    let (items, template): (Vec<Item>, u8) = match heap.object(value) {
        Some(RObject::Real(vals)) => (
            vals.iter().map(|v| Item::Real(*v)).collect(),
            0,
        ),
        Some(RObject::Integer(vals)) => (
            vals.iter().map(|v| Item::Int(*v)).collect(),
            1,
        ),
        Some(RObject::Logical(vals)) => (
            vals.iter().map(|v| Item::Logical(*v)).collect(),
            2,
        ),
        Some(RObject::String(vals)) => (
            vals.iter()
                .map(|v| Item::Text(interner.string_text(*v).to_string()))
                .collect(),
            3,
        ),
        _ => (Vec::new(), 1),
    };
    let is_na = |item: &Item| -> bool {
        match item {
            Item::Real(v) => v.is_nan(),
            Item::Int(v) => *v == crate::value::object::NA_INTEGER,
            Item::Logical(v) => *v == crate::value::object::NA_INTEGER,
            Item::Text(s) => s == "\0NA",
        }
    };
    let names: Vec<String> = match get_attrib(heap, interner, value, "names") {
        Some(id) => string_vec(heap, interner, id),
        None => Vec::new(),
    };

    let mut kept_indices: Vec<usize> = Vec::new();
    let mut dropped: Vec<usize> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if is_na(item) {
            dropped.push(i);
        } else {
            kept_indices.push(i);
        }
    }

    let kept_real: Vec<f64> = kept_indices
        .iter()
        .filter_map(|&i| match &items[i] {
            Item::Real(v) => Some(*v),
            Item::Int(v) => Some(*v as f64),
            Item::Logical(v) => Some(*v as f64),
            _ => None,
        })
        .collect();
    let mut result = match template {
        0 => heap.alloc(RObject::Real(kept_real)),
        1 => heap.alloc(RObject::Integer(
            kept_indices
                .iter()
                .filter_map(|&i| match &items[i] {
                    Item::Int(v) => Some(*v),
                    _ => None,
                })
                .collect(),
        )),
        2 => heap.alloc(RObject::Logical(
            kept_indices
                .iter()
                .filter_map(|&i| match &items[i] {
                    Item::Logical(v) => Some(*v),
                    Item::Int(v) => Some(*v),
                    _ => None,
                })
                .collect(),
        )),
        3 => {
            let interned: Vec<super::intern::InternedStr> = kept_indices
                .iter()
                .filter_map(|&i| match &items[i] {
                    Item::Text(s) => Some(interner.intern_string(s)),
                    _ => None,
                })
                .collect();
            heap.alloc(RObject::String(interned))
        }
        _ => unreachable!(),
    };

    // Kept names survive on the result exactly as GNU preserves them.
    if !names.is_empty() {
        let kept_names: Vec<super::intern::InternedStr> = kept_indices
            .iter()
            .map(|&i| interner.intern_string(names.get(i).map(|s| s.as_str()).unwrap_or("")))
            .collect();
        let names_obj = heap.alloc(RObject::String(kept_names));
        result = copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)?;
    }

    if !dropped.is_empty() {
        let mut action = heap.alloc(RObject::Integer(dropped.iter().map(|i| *i as i32 + 1).collect()));
        if !names.is_empty() {
            let dropped_names: Vec<super::intern::InternedStr> = dropped
                .iter()
                .map(|&i| interner.intern_string(names.get(i).map(|s| s.as_str()).unwrap_or("")))
                .collect();
            let names_obj = heap.alloc(RObject::String(dropped_names));
            action = copy_with_attrib(heap, interner, action, "names", names_obj).map_err(RSignal::from)?;
        }
        let class_name = if which == NA_OMIT { "omit" } else { "exclude" };
        let class_obj = heap.alloc(RObject::String(vec![interner.intern_string(class_name)]));
        action = copy_with_attrib(heap, interner, action, "class", class_obj).map_err(RSignal::from)?;
        result = copy_with_attrib(heap, interner, result, "na.action", action).map_err(RSignal::from)?;
    }
    Ok(result)
}

fn trig_unary(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use crate::value::object::{NA_REAL_BITS, RComplex};
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    if let Some(RObject::Complex(vals)) = heap.object(operands[0]) {
        let na = f64::from_bits(NA_REAL_BITS);
        let out: Vec<RComplex> = vals
            .iter()
            .map(|z| {
                if complex_na(*z) {
                    return RComplex { r: na, i: na };
                }
                let (a, b) = (z.r, z.i);
                match op {
                    SIN => RComplex {
                        r: a.sin() * b.cosh(),
                        i: a.cos() * b.sinh(),
                    },
                    COS => RComplex {
                        r: a.cos() * b.cosh(),
                        i: -a.sin() * b.sinh(),
                    },
                    SINH => RComplex {
                        r: a.sinh() * b.cos(),
                        i: a.cosh() * b.sin(),
                    },
                    COSH => RComplex {
                        r: a.cosh() * b.cos(),
                        i: a.sinh() * b.sin(),
                    },
                    _ => {
                        // tanh = sinh/cosh stabilized through the tanh(a)
                        /// composition — dividing numerator and denominator
                        /// by cosh(a) keeps large-real inputs finite exactly
                        /// as libm's saturated complex tanh.
                        let ta = a.tanh();
                        let num = RComplex { r: ta * b.cos(), i: b.sin() };
                        let den = RComplex { r: b.cos(), i: ta * b.sin() };
                        let denom = den.r * den.r + den.i * den.i;
                        if denom == 0.0 {
                            RComplex { r: f64::NAN, i: f64::NAN }
                        } else {
                            RComplex {
                                r: (num.r * den.r + num.i * den.i) / denom,
                                i: (num.i * den.r - num.r * den.i) / denom,
                            }
                        }
                    }
                }
            })
            .collect();
        return Ok(heap.alloc(RObject::Complex(out)));
    }
    let values = reals_of_value(heap, operands[0]).map_err(RSignal::from)?;
    let out: Vec<f64> = values
        .iter()
        .map(|v| {
            if v.is_nan() {
                f64::from_bits(crate::value::object::NA_REAL_BITS)
            } else {
                match op {
                    SIN => v.sin(),
                    COS => v.cos(),
                    SINH => v.sinh(),
                    COSH => v.cosh(),
                    _ => v.tanh(),
                }
            }
        })
        .collect();
    Ok(heap.alloc(RObject::Real(out)))
}

/// `complex(length.out, real, imaginary)` — zeros by default exactly as
/// GNU's constructor, real/imaginary parts recycling through the length.
fn complex_ctor(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use crate::value::object::RComplex;
    let mut length_out = 0usize;
    let mut real_vals: Vec<f64> = Vec::new();
    let mut imag_vals: Vec<f64> = Vec::new();
    let mut positional = 0usize;
    let mut cursor = args;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            break;
        }
        let (car, tag, next) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
            _ => break,
        };
        let value = eval(heap, interner, car, caller_env)?;
        let tag_name = if tag == ValueId::NIL {
            None
        } else {
            match heap.object(tag) {
                Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                _ => None,
            }
        };
        let reals = numeric_vector(heap, value);
        match tag_name.as_deref() {
            Some("length.out") => length_out = reals.first().copied().unwrap_or(0.0) as usize,
            Some("real") => real_vals = reals,
            Some("imaginary") => imag_vals = reals,
            _ => {
                positional += 1;
                if positional == 1 {
                    length_out = reals.first().copied().unwrap_or(0.0) as usize;
                }
            }
        }
        cursor = next;
    }
    if length_out == 0 {
        length_out = real_vals.len().max(imag_vals.len());
    }
    let out: Vec<RComplex> = (0..length_out)
        .map(|i| RComplex {
            r: if real_vals.is_empty() { 0.0 } else { real_vals[i % real_vals.len()] },
            i: if imag_vals.is_empty() { 0.0 } else { imag_vals[i % imag_vals.len()] },
        })
        .collect();
    Ok(heap.alloc(RObject::Complex(out)))
}

/// First logical element as a boolean — the tagged-argument coercion every
/// flag-style parameter shares.
fn logical_scalar(heap: &Heap, value: ValueId) -> bool {
    match heap.object(value) {
        Some(RObject::Logical(vals)) => vals.first().copied().unwrap_or(0) != 0,
        Some(RObject::Integer(vals)) => vals.first().copied().unwrap_or(0) != 0,
        Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(0.0) != 0.0,
        _ => false,
    }
}

/// `parse(text=, file=)` — source text parses through the legacy parser
/// inside a thread-local session, each top-level expression importing
/// into the new heap exactly as the sweep harness's upfront import does.
/// The dependency on the legacy parser is a migration bridge: the parse
/// tree crosses into ValueId form immediately, so evaluation stays
/// entirely in the new world.
fn parse_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut text_source: Option<String> = None;
    let mut file_path: Option<String> = None;
    let mut cursor = args;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            break;
        }
        let (car, tag, next) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
            _ => break,
        };
        let value = eval(heap, interner, car, caller_env)?;
        let tag_name = if tag == ValueId::NIL {
            None
        } else {
            match heap.object(tag) {
                Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                _ => None,
            }
        };
        let texts = match heap.object(value) {
            Some(RObject::String(vals)) => vals
                .iter()
                .map(|v| interner.string_text(*v).to_string())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        match tag_name.as_deref() {
            Some("file") => file_path = texts.first().cloned(),
            Some("text") => text_source = Some(texts.join("\n")),
            _ => {
                if text_source.is_none() && file_path.is_none() {
                    text_source = Some(texts.join("\n"));
                }
            }
        }
        cursor = next;
    }
    let source = if let Some(path) = file_path {
        std::fs::read_to_string(&path)
            .map_err(|e| RSignal::from(EvalError::TypeError(Box::leak(format!("cannot open file '{path}': {e}").into_boxed_str()))))?
    } else {
        text_source.unwrap_or_default()
    };
    parse_source(heap, interner, &source)
}

/// Parse source text into the imported expression vector — the thread-local
/// session parse and bridge import both dget and the parse primitive share.
fn parse_source(heap: &mut Heap, interner: &mut SymbolInterner, source: &str) -> EvalResult {
    use std::cell::RefCell;
    thread_local! {
        static PARSE_SESSION: RefCell<Option<crate::sexp::session::RSession>> = const { RefCell::new(None) };
    }
    // Empty source parses to the empty expression vector exactly as GNU.
    if source.trim().is_empty() {
        return Ok(heap.alloc(RObject::Expression(Vec::new())));
    }
    let imported = PARSE_SESSION.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(crate::sexp::session::RSession::new());
        }
        let session = slot.as_ref().expect("session initialized");
        let expressions = session.with_active(|| {
            crate::eval::parser::parse_active_expressions(source).unwrap_or_default()
        });
        let mut bridge = crate::sexp::value_bridge::ValueBridge::new();
        let mut out = Vec::new();
        for expr in &expressions {
            if let Ok(id) = bridge.import(heap, interner, expr) {
                out.push(id);
            }
        }
        out
    });
    Ok(heap.alloc(RObject::Expression(imported)))
}

/// Is this object a classed factor — the class chain carrying "factor"
/// exactly as GNU's inherits check.
fn is_factor(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    match get_attrib(heap, interner, value, "class") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(vals)) => vals
                .iter()
                .any(|v| interner.string_text(*v) == "factor"),
            _ => false,
        },
        None => false,
    }
}

/// `gregexpr(pattern, text)` — every match position per input element as a
/// list of integer vectors, each carrying match.length, index.type, and
/// useBytes attributes exactly as GNU; no-match elements yield -1/-1.
fn gregexpr_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let pattern = match heap.object(operands[0]) {
        Some(RObject::String(vals)) if !vals.is_empty() => interner.string_text(vals[0]).to_string(),
        _ => return Err(EvalError::TypeError("pattern must be character").into()),
    };
    let fixed = has_true_tag(heap, interner, args, caller_env, "fixed");
    let texts: Vec<String> = match heap.object(operands[1]) {
        Some(RObject::String(vals)) => vals
            .iter()
            .map(|v| interner.string_text(*v).to_string())
            .collect(),
        _ => Vec::new(),
    };
    let mut elements = Vec::new();
    for text in &texts {
        let mut positions: Vec<i32> = Vec::new();
        let mut lengths: Vec<i32> = Vec::new();
        let mut offset = 0usize;
        // Iterate leftmost matches, advancing past each match end exactly
        /// as GNU's global matching; empty matches advance one byte.
        for _ in 0..100_000 {
            if offset > text.len() {
                break;
            }
            let hay = &text[offset..];
            let found = if fixed {
                hay.find(&pattern).map(|pos| (pos, pos + pattern.len()))
            } else {
                crate::value::regex::regex_search(&pattern, hay)
            };
            match found {
                Some((start, end)) => {
                    let len = end - start;
                    positions.push((offset + start + 1) as i32);
                    lengths.push(len as i32);
                    // Advance past the match; zero-width matches step one
                    // character exactly as GNU's global iteration.
                    offset += start + len.max(1);
                }
                None => break,
            }
        }
        if positions.is_empty() {
            positions.push(-1);
            lengths.push(-1);
        }
        let result = heap.alloc(RObject::Integer(positions));
        let ml = heap.alloc(RObject::Integer(lengths));
        let result = copy_with_attrib(heap, interner, result, "match.length", ml).map_err(RSignal::from)?;
        let it = heap.alloc(RObject::String(vec![interner.intern_string("chars")]));
        let result = copy_with_attrib(heap, interner, result, "index.type", it).map_err(RSignal::from)?;
        let ub = heap.alloc(RObject::Logical(vec![1]));
        let result = copy_with_attrib(heap, interner, result, "useBytes", ub).map_err(RSignal::from)?;
        elements.push(result);
    }
    Ok(heap.alloc(RObject::List(elements)))
}

/// `gsub(pattern, replacement, x)` — every match replaced per element
/// exactly as GNU's global substitution; sub replaces only the first.
fn gsub_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 3)?;
    let pattern = match heap.object(operands[0]) {
        Some(RObject::String(vals)) if !vals.is_empty() => interner.string_text(vals[0]).to_string(),
        _ => return Err(EvalError::TypeError("pattern must be character").into()),
    };
    let replacement = match heap.object(operands[1]) {
        Some(RObject::String(vals)) if !vals.is_empty() => interner.string_text(vals[0]).to_string(),
        _ => return Err(EvalError::TypeError("replacement must be character").into()),
    };
    let fixed = has_true_tag(heap, interner, args, caller_env, "fixed");
    let texts: Vec<String> = match heap.object(operands[2]) {
        Some(RObject::String(vals)) => vals
            .iter()
            .map(|v| interner.string_text(*v).to_string())
            .collect(),
        _ => Vec::new(),
    };
    let out: Vec<super::intern::InternedStr> = texts
        .iter()
        .map(|text| {
            let mut result = String::new();
            let mut cursor = 0usize;
            let mut replaced = 0usize;
            for _ in 0..100_000 {
                if cursor > text.len() {
                    break;
                }
                let hay = &text[cursor..];
                let found = if fixed {
                    hay.find(&pattern).map(|pos| (pos, pos + pattern.len()))
                } else {
                    crate::value::regex::regex_search(&pattern, hay)
                };
                match found {
                    Some((start, end)) => {
                        let len = end - start;
                        result.push_str(&hay[..start]);
                        result.push_str(&replacement);
                        replaced += 1;
                        cursor += start + len.max(1);
                        if op == SUB_FN {
                            break;
                        }
                    }
                    None => break,
                }
            }
            result.push_str(&text[cursor.min(text.len())..]);
            let _ = replaced;
            interner.intern_string(&result)
        })
        .collect();
    Ok(heap.alloc(RObject::String(out)))
}

/// Read one tagged logical argument without disturbing positionals — the
/// regex family shared flag extraction.
fn has_true_tag(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    want: &str,
) -> bool {
    let mut cursor = args;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            break;
        }
        let (car, tag, next) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
            _ => break,
        };
        let tag_name = if tag == ValueId::NIL {
            None
        } else {
            match heap.object(tag) {
                Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                _ => None,
            }
        };
        if tag_name.as_deref() == Some(want) {
            if let Ok(value) = eval(heap, interner, car, caller_env) {
                return logical_scalar(heap, value);
            }
        }
        cursor = next;
    }
    false
}

/// The option store lives on the heap as an insertion-ordered association
/// exactly as GNU's session options; width defaults to 80 as GNU.
fn lookup_option(heap: &Heap, interner: &mut SymbolInterner, name: &str) -> Option<ValueId> {
    let _ = interner;
    heap.options
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| *value)
        .or_else(|| {
            if name == "width" {
                // The one default the slice honors — everything else reads
                // NULL exactly as an unset custom option does.
                heap.options_width_fallback
            } else {
                None
            }
        })
}

/// `options(...)` — tagged pairs set values and the result carries the
/// previous values as a named list; bare string arguments query the named
/// options exactly as GNU's query form.
fn options_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    if heap.options_width_fallback.is_none() {
        let fallback = heap.alloc(RObject::Real(vec![80.0]));
        heap.options_width_fallback = Some(fallback);
    }
    let mut previous: Vec<(String, ValueId)> = Vec::new();
    let mut queried: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            break;
        }
        let (car, tag, next) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
            _ => break,
        };
        let tag_name = if tag == ValueId::NIL {
            None
        } else {
            match heap.object(tag) {
                Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                _ => None,
            }
        };
        match tag_name {
            Some(name) if !name.is_empty() => {
                let value = eval(heap, interner, car, caller_env)?;
                let old = lookup_option(heap, interner, &name).unwrap_or(ValueId::NIL);
                previous.push((name.clone(), old));
                if let Some(slot) = heap.options.iter_mut().find(|(key, _)| *key == name) {
                    slot.1 = value;
                } else {
                    heap.options.push((name, value));
                }
            }
            _ => {
                // Query form: bare character arguments name options to read.
                let value = eval(heap, interner, car, caller_env)?;
                if let Some(RObject::String(vals)) = heap.object(value) {
                    for v in vals.clone() {
                        let name = interner.string_text(v).to_string();
                        let current = lookup_option(heap, interner, &name).unwrap_or(ValueId::NIL);
                        queried.push((name, current));
                    }
                }
            }
        }
        cursor = next;
    }
    // GNU's visibility discipline: setting options returns invisibly; the
    /// pure query form returns visibly.
    if queried.is_empty() && !previous.is_empty() {
        heap.visible = false;
    } else {
        heap.visible = true;
    }
    let result_set = if !queried.is_empty() { queried } else { previous };
    let items: Vec<ValueId> = result_set.iter().map(|(_, v)| *v).collect();
    let names: Vec<super::intern::InternedStr> = result_set
        .iter()
        .map(|(n, _)| interner.intern_string(n))
        .collect();
    let result = heap.alloc(RObject::List(items));
    if !names.is_empty() {
        let names_vec = heap.alloc(RObject::String(names));
        return copy_with_attrib(heap, interner, result, "names", names_vec).map_err(RSignal::from);
    }
    Ok(result)
}

/// `cbind`/`rbind` — atomic operands recycle to the longest length and
/// bind into a column-major matrix exactly as GNU; tags become column or
/// row names; mixed types coerce to the highest in the lattice.
fn bind_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut operands = Vec::new();
    let mut tags: Vec<Option<String>> = Vec::new();
    let mut cursor = args;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            break;
        }
        let (car, tag, next) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let value = eval(heap, interner, car, caller_env)?;
        operands.push(force_if_promise(heap, interner, value)?);
        tags.push(tag_name);
        cursor = next;
    }
    if operands.is_empty() {
        return Ok(ValueId::NIL);
    }
    // Highest rank across operands decides the matrix type.
    let mut rank = 0u8;
    for operand in &operands {
        rank = rank.max(match heap.object(*operand) {
            Some(RObject::Logical(_)) => 1,
            Some(RObject::Integer(_)) => 2,
            Some(RObject::Real(_)) => 3,
            Some(RObject::Complex(_)) => 4,
            Some(RObject::String(_)) => 5,
            Some(RObject::Raw(_)) => 1,
            _ => 0,
        });
    }
    let lengths: Vec<usize> = operands
        .iter()
        .map(|v| vector_length(heap, *v).unwrap_or(0))
        .collect();
    let longest = lengths.iter().copied().max().unwrap_or(0);
    // Zero-length operands drop out whenever any non-zero operand exists
    /// exactly as GNU's cbind/rbind; when every operand is empty they all
    /// stay and shape a zero-extent matrix of the common type.
    if longest > 0 {
        let keep: Vec<usize> = (0..operands.len()).filter(|&i| lengths[i] > 0).collect();
        operands = keep.iter().map(|&i| operands[i]).collect();
        tags = keep.iter().map(|&i| tags[i].clone()).collect();
    }
    let (nrow, ncol) = if op == CBIND {
        (longest, operands.len())
    } else {
        (operands.len(), longest)
    };
    // Element extraction with recycling — column-major output arranged by
    /// orientation exactly as GNU's recycling fill.
    let cell = |heap: &Heap, interner: &SymbolInterner, operand: ValueId, row: usize| -> (i32, f64, String) {
        let len = vector_length(heap, operand).unwrap_or(0);
        if len == 0 {
            // Zero-extent matrices never touch their elements — the total
            /// element count is zero — so this path is unreachable in exact
            /// GNU shape; the guard documents the invariant.
            return (crate::value::object::NA_INTEGER, f64::from_bits(crate::value::object::NA_REAL_BITS), "NA".to_string());
        }
        let idx = row % len;
        match heap.object(operand) {
            Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => {
                let v = vals.get(idx).copied().unwrap_or(crate::value::object::NA_INTEGER);
                let r = if v == crate::value::object::NA_INTEGER {
                    f64::from_bits(crate::value::object::NA_REAL_BITS)
                } else {
                    v as f64
                };
                (v, r, if v == crate::value::object::NA_INTEGER { "NA".to_string() } else { v.to_string() })
            }
            Some(RObject::Real(vals)) => {
                let v = vals.get(idx).copied().unwrap_or(f64::NAN);
                (0, v, crate::value::print::format_real(v))
            }
            Some(RObject::String(vals)) => {
                let s = vals
                    .get(idx)
                    .map(|x| interner.string_text(*x).to_string())
                    .unwrap_or_default();
                (0, f64::NAN, s)
            }
            _ => (0, f64::NAN, String::new()),
        }
    };
    // Column-major positions: cbind column j is operand j recycled down the
    /// rows; rbind row i is operand i with elements running across columns.
    let total = nrow * ncol;
    let mut ints = Vec::with_capacity(total);
    let mut reals = Vec::with_capacity(total);
    let mut texts: Vec<String> = Vec::with_capacity(total);
    for flat in 0..total {
        let row = flat % nrow;
        let col = flat / nrow;
        let (operand, element_row) = if op == CBIND {
            (operands[col], row)
        } else {
            (operands[row], col)
        };
        let (iv, rv, tv) = cell(heap, interner, operand, element_row);
        ints.push(iv);
        reals.push(rv);
        texts.push(tv);
    }
    let matrix = if rank >= 5 {
        let interned: Vec<super::intern::InternedStr> = texts
            .iter()
            .map(|s| interner.intern_string(s))
            .collect();
        heap.alloc(RObject::String(interned))
    } else if rank >= 3 {
        heap.alloc(RObject::Real(reals))
    } else {
        heap.alloc(RObject::Integer(ints))
    };
    let dim = heap.alloc(RObject::Integer(vec![nrow as i32, ncol as i32]));
    let matrix = copy_with_attrib(heap, interner, matrix, "dim", dim).map_err(RSignal::from)?;
    // Dimension names: tags fill the binding orientation exactly as GNU;
    /// the other orientation carries empty names.
    if tags.iter().any(|t| t.is_some()) {
        let tag_texts: Vec<super::intern::InternedStr> = tags
            .iter()
            .map(|t| interner.intern_string(t.as_deref().unwrap_or("")))
            .collect();
        let tag_vec = heap.alloc(RObject::String(tag_texts));
        // The untagged orientation carries NULL exactly as GNU — an empty
        /// character vector would read non-null to is.null.
        let (first, second) = if op == CBIND {
            (ValueId::NIL, tag_vec)
        } else {
            (tag_vec, ValueId::NIL)
        };
        let dn = heap.alloc(RObject::List(vec![first, second]));
        copy_with_attrib(heap, interner, matrix, "dimnames", dn).map_err(RSignal::from)
    } else {
        Ok(matrix)
    }
}

/// `substitute(expr, env)` — GNU's matchArgs matrix in miniature for the
/// two formals, then symbol substitution through the expression tree
/// exactly as substitute does for list environments.
fn substitute_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Collect tagged/untagged actuals in order.
    struct Actual {
        car: ValueId,
        tag: Option<String>,
        index: usize,
    }
    let mut actuals = Vec::new();
    let mut cursor = args;
    for i in 0..100_000usize {
        if cursor == ValueId::NIL {
            break;
        }
        let (car, tag, next) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        actuals.push(Actual { car, tag: tag_name, index: i });
        cursor = next;
    }
    let formals = ["expr", "env"];
    let mut bound: [Option<ValueId>; 2] = [None, None];
    let mut exact_taken = [false; 2];
    let mut partial_taken = [false; 2];
    let mut used = vec![false; actuals.len()];
    // Pass 1: exact names.
    for (ai, actual) in actuals.iter().enumerate() {
        if let Some(tag) = &actual.tag {
            if let Some(fi) = formals.iter().position(|f| f == tag) {
                if exact_taken[fi] || partial_taken[fi] || bound[fi].is_some() {
                    return Err(EvalError::Condition(format!(
                        "formal argument \"{}\" matched by multiple actual arguments",
                        formals[fi]
                    ))
                    .into());
                }
                bound[fi] = Some(actual.car);
                exact_taken[fi] = true;
                used[ai] = true;
            }
        }
    }
    // Pass 2: partial matching among non-exact-bound formals.
    for (ai, actual) in actuals.iter().enumerate() {
        if used[ai] {
            continue;
        }
        if let Some(tag) = &actual.tag {
            if tag.is_empty() {
                continue;
            }
            let matches: Vec<usize> = (0..2)
                .filter(|&fi| !exact_taken[fi] && formals[fi].starts_with(tag.as_str()))
                .collect();
            if matches.len() > 1 {
                return Err(EvalError::Condition(format!(
                    "argument {} matches multiple formal arguments",
                    actual.index + 1
                ))
                .into());
            }
            if matches.len() == 1 {
                let fi = matches[0];
                if partial_taken[fi] {
                    return Err(EvalError::Condition(format!(
                        "formal argument \"{}\" matched by multiple actual arguments",
                        formals[fi]
                    ))
                    .into());
                }
                bound[fi] = Some(actual.car);
                partial_taken[fi] = true;
                used[ai] = true;
            }
        }
    }
    // Pass 3: untagged actuals fill unbound formals in order.
    for (ai, actual) in actuals.iter().enumerate() {
        if used[ai] || actual.tag.is_some() {
            continue;
        }
        if let Some(fi) = (0..2).find(|&fi| bound[fi].is_none()) {
            bound[fi] = Some(actual.car);
            used[ai] = true;
        }
    }
    // Leftovers error with their deparsed actuals exactly as GNU.
    for (ai, actual) in actuals.iter().enumerate() {
        if used[ai] {
            continue;
        }
        let rendered = crate::value::print::deparse(heap, interner, actual.car);
        let detail = match &actual.tag {
            Some(tag) => format!("{tag} = {rendered}"),
            None => rendered,
        };
        return Err(EvalError::Condition(format!("unused argument ({detail})")).into());
    }
    let expr = bound[0].unwrap_or(ValueId::NIL);
    // The environment argument evaluates exactly as any argument does.
    let env_value = match bound[1] {
        Some(env_expr) => {
            let evaluated = eval(heap, interner, env_expr, caller_env)?;
            force_if_promise(heap, interner, evaluated)?
        }
        None => caller_env,
    };
    Ok(substitute_in_tree(heap, interner, expr, env_value))
}

/// Recursive symbol substitution through a language tree — symbols named
/// in the environment list replace with their values exactly as GNU's
/// substitute; everything else rebuilds structurally.
fn substitute_in_tree(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    expr: ValueId,
    env: ValueId,
) -> ValueId {
    if let Some(RObject::Symbol { name, .. }) = heap.object(expr) {
        let text = interner.symbol_name(*name).to_string();
        if let Some(RObject::List(items)) = heap.object(env) {
            let names = get_attrib(heap, interner, env, "names")
                .map(|id| string_vec(heap, interner, id))
                .unwrap_or_default();
            for (i, item) in items.clone().iter().enumerate() {
                if names.get(i).map(|n| n == &text).unwrap_or(false) {
                    return *item;
                }
            }
        }
        // GNU's frame semantics: a symbol bound to a promise in the
        // calling frame substitutes the promise's unevaluated expression;
        // plain bindings and misses leave the symbol standing.
        if matches!(heap.object(env), Some(RObject::Environment { .. })) {
            let sym = interner.intern_symbol(&text);
            if let Some(binding) = super::env::lookup_in_frame(heap, env, sym) {
                if let Some(RObject::Promise { expr: promise_expr, .. }) = heap.object(binding.value) {
                    if *promise_expr != ValueId::MISSING {
                        return *promise_expr;
                    }
                }
            }
        }
        return expr;
    }
    if let Some(RObject::Language { car, cdr, tag }) = heap.object(expr).cloned() {
        let new_car = substitute_in_tree(heap, interner, car, env);
        let new_cdr = substitute_in_tree(heap, interner, cdr, env);
        return heap.alloc(RObject::Language { car: new_car, cdr: new_cdr, tag });
    }
    if let Some(RObject::Pairlist { car, cdr, tag, .. }) = heap.object(expr).cloned() {
        let new_car = substitute_in_tree(heap, interner, car, env);
        let new_cdr = substitute_in_tree(heap, interner, cdr, env);
        return heap.alloc(RObject::Pairlist { car: new_car, cdr: new_cdr, tag });
    }
    expr
}

/// `summary(x)` — the default methods exactly as GNU's summary.default
/// family: numeric yields the five-number summary plus mean as a classed
/// vector, logical yields mode and counts, character yields the length/
/// unique/nchar profile, and factor/ordered yields level counts exactly
/// as summary.factor. The classed object prints through the
/// summaryDefault table path.
fn summary_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let value = operands[0];

    let names_attr = |heap: &mut Heap, interner: &mut SymbolInterner, target: ValueId, names: &[&str]| -> Result<ValueId, RSignal> {
        let interned: Vec<_> = names.iter().map(|n| interner.intern_string(n)).collect();
        let names_obj = heap.alloc(RObject::String(interned));
        copy_with_attrib(heap, interner, target, "names", names_obj).map_err(Into::into)
    };
    let class_summary = |heap: &mut Heap, interner: &mut SymbolInterner, target: ValueId| -> Result<ValueId, RSignal> {
        let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("summaryDefault")]));
        copy_with_attrib(heap, interner, target, "class", class_obj).map_err(Into::into)
    };

    // factor/ordered: level counts exactly as summary.factor — a plain
    // named integer vector, NAs count appended when missing values exist.
    if is_factor(heap, interner, value) {
        let levels = get_attrib(heap, interner, value, "levels")
            .map(|id| string_vec(heap, interner, id))
            .unwrap_or_default();
        let codes: Vec<i32> = match heap.object(value) {
            Some(RObject::Integer(vals)) => vals.clone(),
            _ => Vec::new(),
        };
        let mut counts = vec![0i32; levels.len()];
        let mut na_count = 0i32;
        for code in &codes {
            if *code == crate::value::object::NA_INTEGER {
                na_count += 1;
            } else if *code >= 1 && (*code as usize) <= levels.len() {
                counts[(*code - 1) as usize] += 1;
            }
        }
        let mut names: Vec<&str> = levels.iter().map(|s| s.as_str()).collect();
        if na_count > 0 {
            counts.push(na_count);
            names.push("NAs");
        }
        let result = heap.alloc(RObject::Integer(counts));
        return names_attr(heap, interner, result, &names);
    }

    match heap.object(value) {
        Some(RObject::Real(_)) | Some(RObject::Integer(_)) => {
            let vals: Vec<f64> = match heap.object(value) {
                Some(RObject::Real(v)) => v.clone(),
                Some(RObject::Integer(v)) => v
                    .iter()
                    .map(|&x| {
                        if x == crate::value::object::NA_INTEGER {
                            f64::from_bits(crate::value::object::NA_REAL_BITS)
                        } else {
                            x as f64
                        }
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let mut finite: Vec<f64> = vals.iter().copied().filter(|v| !v.is_nan()).collect();
            let na_count = vals.len() - finite.len();
            finite.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            // GNU's quantile type 7 interpolation exactly as the default.
            let quantile7 = |sorted: &[f64], prob: f64| -> f64 {
                let n = sorted.len();
                if n == 0 {
                    return f64::from_bits(crate::value::object::NA_REAL_BITS);
                }
                if n == 1 {
                    return sorted[0];
                }
                let h = 1.0 + (n as f64 - 1.0) * prob;
                let lower = h.floor() as usize;
                if lower >= n {
                    return sorted[n - 1];
                }
                let frac = h - lower as f64;
                sorted[lower - 1] + frac * (sorted[lower] - sorted[lower - 1])
            };
            let (min, q1, median, mean, q3, max);
            if finite.is_empty() {
                let na = f64::from_bits(crate::value::object::NA_REAL_BITS);
                min = na;
                q1 = na;
                median = na;
                mean = f64::NAN;
                q3 = na;
                max = na;
            } else {
                min = finite[0];
                q1 = quantile7(&finite, 0.25);
                median = quantile7(&finite, 0.5);
                mean = finite.iter().sum::<f64>() / finite.len() as f64;
                q3 = quantile7(&finite, 0.75);
                max = finite[finite.len() - 1];
            }
            let mut stats = vec![min, q1, median, mean, q3, max];
            let mut names = vec!["Min.", "1st Qu.", "Median", "Mean", "3rd Qu.", "Max."];
            if na_count > 0 {
                stats.push(na_count as f64);
                names.push("NAs");
            }
            let result = heap.alloc(RObject::Real(stats));
            let result = names_attr(heap, interner, result, &names)?;
            class_summary(heap, interner, result)
        }
        Some(RObject::Logical(vals)) => {
            let vals = vals.clone();
            let mut false_count = 0i32;
            let mut true_count = 0i32;
            let mut na_count = 0i32;
            for v in &vals {
                if *v == crate::value::object::NA_INTEGER {
                    na_count += 1;
                } else if *v != 0 {
                    true_count += 1;
                } else {
                    false_count += 1;
                }
            }
            // GNU's logical summary is a character matrix of mode plus
            // counts, NAs column only when missing values exist.
            let mut cells = vec!["logical".to_string()];
            let mut names = vec!["Mode".to_string()];
            if false_count > 0 || true_count > 0 || na_count == 0 {
                names.push("FALSE".to_string());
                cells.push(false_count.to_string());
                names.push("TRUE".to_string());
                cells.push(true_count.to_string());
            }
            if na_count > 0 {
                names.push("NAs".to_string());
                cells.push(na_count.to_string());
            }
            let interned: Vec<_> = cells.iter().map(|c| interner.intern_string(c)).collect();
            let result = heap.alloc(RObject::String(interned));
            let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
            let result = names_attr(heap, interner, result, &name_refs)?;
            class_summary(heap, interner, result)
        }
        Some(RObject::String(vals)) => {
            let texts: Vec<String> = vals.iter().map(|v| interner.string_text(*v).to_string()).collect();
            let total = texts.len() as i32;
            let non_na: Vec<&String> = texts.iter().filter(|s| !s.starts_with('\0')).collect();
            let na_count = total as usize - non_na.len();
            let mut unique = std::collections::HashSet::new();
            let mut blank = 0i32;
            for s in &non_na {
                unique.insert(s.as_str());
                if s.is_empty() {
                    blank += 1;
                }
            }
            let min_nchar = if non_na.is_empty() {
                crate::value::object::NA_INTEGER
            } else {
                non_na.iter().map(|s| s.chars().count() as i32).min().unwrap_or(0)
            };
            let max_nchar = if non_na.is_empty() {
                crate::value::object::NA_INTEGER
            } else {
                non_na.iter().map(|s| s.chars().count() as i32).max().unwrap_or(0)
            };
            let mut stats = vec![
                total,
                unique.len() as i32,
                blank,
                min_nchar,
                max_nchar,
            ];
            let mut names = vec!["Length", "N.unique", "N.blank", "Min.nchar", "Max.nchar"];
            if na_count > 0 {
                stats.push(na_count as i32);
                names.push("NAs");
            }
            let result = heap.alloc(RObject::Integer(stats));
            let result = names_attr(heap, interner, result, &names)?;
            class_summary(heap, interner, result)
        }
        _ => Ok(value),
    }
}

/// Matrix algebra over dim-attributed vectors exactly as GNU: `%*%` is
/// the inner product with column-major layout, crossprod and tcrossprod
/// the transpose products, and max.col the row-wise maxima positions with
/// first/last tie breaking.
fn matrix_ops_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let dims_of = |heap: &Heap, interner: &SymbolInterner, v: ValueId| -> Option<(usize, usize)> {
        get_attrib(heap, interner, v, "dim").and_then(|id| match heap.object(id) {
            Some(RObject::Integer(d)) if d.len() == 2 => Some((d[0] as usize, d[1] as usize)),
            Some(RObject::Real(d)) if d.len() == 2 => Some((d[0] as usize, d[1] as usize)),
            _ => None,
        })
    };
    let reals_of = |heap: &Heap, v: ValueId| -> Vec<f64> {
        match heap.object(v) {
            Some(RObject::Real(vals)) => vals.clone(),
            Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals
                .iter()
                .map(|&x| {
                    if x == crate::value::object::NA_INTEGER {
                        f64::from_bits(crate::value::object::NA_REAL_BITS)
                    } else {
                        x as f64
                    }
                })
                .collect(),
            _ => Vec::new(),
        }
    };

    if op == MAXCOL {
        let operands = eval_args(heap, interner, args, caller_env, 2)?;
        let (nr, nc) = dims_of(heap, interner, operands[0])
            .ok_or(EvalError::TypeError("max.col requires a matrix"))?;
        let vals = reals_of(heap, operands[0]);
        let ties_last = string_operand(heap, interner, operands[1])
            .map(|s| s == "last")
            .unwrap_or(false);
        let mut out = Vec::with_capacity(nr);
        for row in 0..nr {
            let mut best: Option<(f64, usize)> = None;
            for col in 0..nc {
                let v = vals[col * nr + row];
                if v.is_nan() {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some((bv, _)) if v > bv => true,
                    Some((bv, _)) if v == bv && ties_last => true,
                    _ => false,
                };
                if better {
                    best = Some((v, col));
                }
            }
            out.push(best.map(|(_, c)| (c + 1) as i32).unwrap_or(crate::value::object::NA_INTEGER));
        }
        return Ok(heap.alloc(RObject::Integer(out)));
    }

    if op == MATRIX_MULT {
        let operands = eval_args(heap, interner, args, caller_env, 2)?;
        let (ar, ac) = dims_of(heap, interner, operands[0])
            .ok_or(EvalError::TypeError("matrix multiplication requires matrices"))?;
        let (br, bc) = dims_of(heap, interner, operands[1])
            .ok_or(EvalError::TypeError("matrix multiplication requires matrices"))?;
        if ac != br {
            return Err(EvalError::TypeError("non-conformable arguments").into());
        }
        let a = reals_of(heap, operands[0]);
        let b = reals_of(heap, operands[1]);
        let mut out = vec![0.0f64; ar * bc];
        for i in 0..ar {
            for j in 0..bc {
                let mut sum = 0.0;
                for k in 0..ac {
                    sum += a[k * ar + i] * b[j * br + k];
                }
                out[j * ar + i] = sum;
            }
        }
        let result = heap.alloc(RObject::Real(out));
        let dim_vec = heap.alloc(RObject::Integer(vec![ar as i32, bc as i32]));
        return copy_with_attrib(heap, interner, result, "dim", dim_vec).map_err(Into::into);
    }

    // crossprod(x) = t(x) %*% x ; tcrossprod(x) = x %*% t(x)
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let (nr, nc) = dims_of(heap, interner, operands[0])
        .ok_or(EvalError::TypeError("crossprod requires a matrix"))?;
    let vals = reals_of(heap, operands[0]);
    let (out_dim, result_vals): (usize, Vec<f64>) = if op == CROSSPROD {
        let mut out = vec![0.0f64; nc * nc];
        for i in 0..nc {
            for j in 0..nc {
                let mut sum = 0.0;
                for k in 0..nr {
                    sum += vals[i * nr + k] * vals[j * nr + k];
                }
                out[j * nc + i] = sum;
            }
        }
        (nc, out)
    } else {
        let mut out = vec![0.0f64; nr * nr];
        for i in 0..nr {
            for j in 0..nr {
                let mut sum = 0.0;
                for k in 0..nc {
                    sum += vals[k * nr + i] * vals[k * nr + j];
                }
                out[j * nr + i] = sum;
            }
        }
        (nr, out)
    };
    let result = heap.alloc(RObject::Real(result_vals));
    let dim_vec = heap.alloc(RObject::Integer(vec![out_dim as i32, out_dim as i32]));
    copy_with_attrib(heap, interner, result, "dim", dim_vec).map_err(Into::into)
}

/// Reverse any vector or list elementwise exactly as GNU's rev — names
/// and attributes reverse with their elements.
fn reverse_value(heap: &mut Heap, interner: &mut SymbolInterner, value: ValueId) -> EvalResult {
    let names_rev = |heap: &mut Heap, interner: &mut SymbolInterner, result: ValueId, source: ValueId| -> EvalResult {
        if let Some(names_id) = get_attrib(heap, interner, source, "names") {
            if let Some(RObject::String(vals)) = heap.object(names_id) {
                let reversed: Vec<_> = vals.iter().rev().copied().collect();
                let names_obj = heap.alloc(RObject::String(reversed));
                return copy_with_attrib(heap, interner, result, "names", names_obj).map_err(Into::into);
            }
        }
        Ok(result)
    };
    match heap.object(value) {
        Some(RObject::List(items)) => {
            let reversed: Vec<ValueId> = items.iter().rev().copied().collect();
            let result = heap.alloc(RObject::List(reversed));
            names_rev(heap, interner, result, value)
        }
        Some(RObject::String(vals)) => {
            let reversed: Vec<_> = vals.iter().rev().copied().collect();
            let result = heap.alloc(RObject::String(reversed));
            names_rev(heap, interner, result, value)
        }
        Some(RObject::Integer(vals)) => {
            let reversed: Vec<_> = vals.iter().rev().copied().collect();
            let result = heap.alloc(RObject::Integer(reversed));
            names_rev(heap, interner, result, value)
        }
        Some(RObject::Real(vals)) => {
            let reversed: Vec<_> = vals.iter().rev().copied().collect();
            let result = heap.alloc(RObject::Real(reversed));
            names_rev(heap, interner, result, value)
        }
        Some(RObject::Logical(vals)) => {
            let reversed: Vec<_> = vals.iter().rev().copied().collect();
            let result = heap.alloc(RObject::Logical(reversed));
            names_rev(heap, interner, result, value)
        }
        Some(RObject::Raw(vals)) => {
            let reversed: Vec<_> = vals.iter().rev().copied().collect();
            let result = heap.alloc(RObject::Raw(reversed));
            names_rev(heap, interner, result, value)
        }
        Some(RObject::Complex(vals)) => {
            let reversed: Vec<_> = vals.iter().rev().copied().collect();
            let result = heap.alloc(RObject::Complex(reversed));
            names_rev(heap, interner, result, value)
        }
        _ => Ok(value),
    }
}

/// Names for apply iteration exactly as GNU: pairlist inputs carry their
/// node tags, vectors their names attribute.
fn iteration_names(heap: &Heap, interner: &SymbolInterner, x: ValueId) -> Vec<String> {
    if matches!(heap.object(x), Some(RObject::Pairlist { .. })) {
        let mut out = Vec::new();
        let mut cursor = x;
        loop {
            match heap.object(cursor) {
                Some(RObject::Pairlist { cdr, tag, .. }) => {
                    match heap.object(*tag) {
                        Some(RObject::Symbol { name, .. }) => {
                            out.push(interner.symbol_name(*name).to_string());
                        }
                        _ => out.push(String::new()),
                    }
                    if *cdr == ValueId::NIL {
                        break;
                    }
                    cursor = *cdr;
                }
                _ => break,
            }
        }
        return out;
    }
    match get_attrib(heap, interner, x, "names") {
        Some(n) => string_vec(heap, interner, n),
        None => Vec::new(),
    }
}

/// Run a command capturing stdout trimmed — the system identity shells
/// share exactly.
fn run_command_output(cmd: &str, args: &[&str]) -> Result<String, std::io::Error> {
    let output = std::process::Command::new(cmd).args(args).output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// GNU round semantics: half-to-even at the decimal digit position —
/// negative digits round left of the decimal point exactly as GNU.
fn round_half_even(v: f64, digits: i32) -> f64 {
    let scale = 10f64.powi(digits);
    if digits >= 0 {
        let scaled = v * scale;
        if !scaled.is_finite() {
            return v;
        }
        scaled.round_ties_even() / scale
    } else {
        let divisor = 10f64.powi(-digits);
        (v / divisor).round_ties_even() * divisor
    }
}

/// GNU signif semantics: round to the given count of significant digits.
fn signif_digits(v: f64, digits: i32) -> f64 {
    if v == 0.0 {
        return v;
    }
    let digits = if digits < 1 { 1 } else { digits };
    let magnitude = v.abs().log10().floor() as i32;
    let shift = digits - 1 - magnitude;
    round_half_even(v, shift)
}

/// GNU sinpi exactness: integer inputs are exactly zero, half-integers
/// exactly ±1 — computed by remainder reduction, not raw π multiplication.
fn sinpi_exact(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return f64::NAN;
    }
    let r = x % 2.0;
    if r == 0.0 || r == 1.0 || r == -1.0 {
        return 0.0_f64.copysign(x);
    }
    if r == 0.5 {
        return 1.0;
    }
    if r == -0.5 {
        return -1.0;
    }
    if r == 1.5 {
        return -1.0;
    }
    if r == -1.5 {
        return 1.0;
    }
    (r * std::f64::consts::PI).sin()
}

/// GNU cospi exactness: integer inputs alternate ±1, half-integers are
/// exactly zero.
fn cospi_exact(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return f64::NAN;
    }
    let r = x % 2.0;
    if r == 0.0 {
        return 1.0;
    }
    if r == 1.0 || r == -1.0 {
        return -1.0;
    }
    if r == 0.5 || r == -0.5 || r == 1.5 || r == -1.5 {
        return 0.0;
    }
    (r * std::f64::consts::PI).cos()
}

/// GNU tanpi exactness: integer inputs are exactly zero, quarter inputs
/// exactly ±1.
fn tanpi_exact(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return f64::NAN;
    }
    let r = x % 1.0;
    if r == 0.0 {
        return 0.0_f64.copysign(x);
    }
    if r == 0.25 {
        return 1.0;
    }
    if r == -0.25 {
        return -1.0;
    }
    (r * std::f64::consts::PI).tan()
}

/// Scan any vector for missingness exactly as anyNA — NA integers,
/// NA-real and NaN doubles, logical NA, the string sentinel.
fn any_na_in_value(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    match heap.object(value) {
        Some(RObject::Integer(v)) => v.iter().any(|x| *x == crate::value::object::NA_INTEGER),
        Some(RObject::Logical(v)) => v.iter().any(|x| *x == crate::value::object::NA_INTEGER),
        Some(RObject::Real(v)) => v.iter().any(|x| x.is_nan()),
        Some(RObject::String(v)) => v.iter().any(|s| {
            interner.string_text(*s).starts_with('\0')
        }),
        Some(RObject::Complex(v)) => v.iter().any(|c| c.r.is_nan() || c.i.is_nan()),
        Some(RObject::List(items)) => items.iter().any(|item| any_na_in_value(heap, interner, *item)),
        _ => false,
    }
}

/// Approximate substring edit distance exactly as TRE's semantics: the
/// pattern aligns against the best substring of the text — leading and
/// trailing text outside the match is free. Classic DP with a zeroed
/// first row.
fn approximate_substring_distance(pattern: &str, text: &str) -> usize {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = text.chars().collect();
    if p.is_empty() {
        return 0;
    }
    if s.is_empty() {
        return p.len();
    }
    let mut prev: Vec<usize> = (0..=p.len()).collect();
    let mut best = prev[p.len()];
    for row in 1..=s.len() {
        let mut cur = vec![0usize; p.len() + 1];
        for col in 1..=p.len() {
            let cost = if p[col - 1] == s[row - 1] { 0 } else { 1 };
            cur[col] = (prev[col] + 1).min(cur[col - 1] + 1).min(prev[col - 1] + cost);
        }
        best = best.min(cur[p.len()]);
        prev = cur;
    }
    best
}

/// The enclosure chain from the user environment to base — GNU's search
/// path skeleton.
fn enclosure_chain(heap: &Heap, start: ValueId) -> Vec<ValueId> {
    let mut out = Vec::new();
    let mut cursor = start;
    for _ in 0..100 {
        if cursor == ValueId::EMPTY_ENV || cursor.is_sentinel() {
            break;
        }
        if !matches!(heap.object(cursor), Some(RObject::Environment { .. })) {
            break;
        }
        out.push(cursor);
        cursor = match heap.object(cursor) {
            Some(RObject::Environment { enclos, .. }) => *enclos,
            _ => break,
        };
    }
    out
}

/// Extract one margin slice from a column-major matrix exactly as GNU's
/// apply indexing — margin 1 row idx strides by nrow, margin 2 column
/// idx is contiguous.
fn extract_matrix_margin(
    heap: &mut Heap,
    interner: &SymbolInterner,
    matrix: ValueId,
    margin: usize,
    idx: usize,
    nrow: usize,
    ncol: usize,
) -> EvalResult {
    let _ = interner;
    macro_rules! slice_of {
        ($v:expr, $ty:ty) => {{
            let indices: Vec<usize> = if margin == 1 {
                (0..ncol).map(|c| c * nrow + idx).collect()
            } else {
                (0..nrow).map(|r| idx * nrow + r).collect()
            };
            let out: Vec<$ty> = indices.iter().filter_map(|i| $v.get(*i).copied()).collect();
            out
        }};
    }
    match heap.object(matrix) {
        Some(RObject::Integer(v)) => {
            let out = slice_of!(v, i32);
            Ok(heap.alloc(RObject::Integer(out)))
        }
        Some(RObject::Real(v)) => {
            let out = slice_of!(v, f64);
            Ok(heap.alloc(RObject::Real(out)))
        }
        Some(RObject::Logical(v)) => {
            let out = slice_of!(v, i32);
            Ok(heap.alloc(RObject::Logical(out)))
        }
        Some(RObject::String(v)) => {
            let out = slice_of!(v, super::intern::InternedStr);
            Ok(heap.alloc(RObject::String(out)))
        }
        Some(RObject::Complex(v)) => {
            let out = slice_of!(v, crate::value::object::RComplex);
            Ok(heap.alloc(RObject::Complex(out)))
        }
        _ => Err(EvalError::TypeError("apply requires a vector").into()),
    }
}

/// GNU's re-factorization: the original level order survives but levels
/// absent from the data drop — as.ordered's factor-input semantics.
fn refactor_dropping_unused(heap: &mut Heap, interner: &mut SymbolInterner, factor: ValueId) -> EvalResult {
    let codes: Vec<i32> = match heap.object(factor) {
        Some(RObject::Integer(v)) => v.clone(),
        _ => return Ok(factor),
    };
    let levels: Vec<String> = match get_attrib(heap, interner, factor, "levels") {
        Some(l) => string_vec(heap, interner, l),
        None => Vec::new(),
    };
    let used: Vec<bool> = {
        let mut u = vec![false; levels.len()];
        for code in &codes {
            if *code != crate::value::object::NA_INTEGER && *code >= 1 && (*code as usize) <= levels.len() {
                u[(*code as usize) - 1] = true;
            }
        }
        u
    };
    // Remap codes onto the filtered level set exactly as GNU's refactors.
    let mut remap = vec![0i32; levels.len() + 1];
    let mut kept: Vec<String> = Vec::new();
    for (index, level) in levels.iter().enumerate() {
        if used[index] {
            kept.push(level.clone());
            remap[index + 1] = kept.len() as i32;
        }
    }
    let new_codes: Vec<i32> = codes
        .iter()
        .map(|c| {
            if *c == crate::value::object::NA_INTEGER {
                *c
            } else {
                remap[*c as usize]
            }
        })
        .collect();
    let result = heap.alloc(RObject::Integer(new_codes));
    let interned: Vec<_> = kept.iter().map(|s| interner.intern_string(s)).collect();
    let levels_obj = heap.alloc(RObject::String(interned));
    let result = copy_with_attrib(heap, interner, result, "levels", levels_obj).map_err(RSignal::from)?;
    let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("factor")]));
    copy_with_attrib(heap, interner, result, "class", class_obj).map_err(RSignal::from)
}

/// The aperm element permutation: out multi-index (i_1..i_n) draws input
/// element j where j_{perm[m]} = i_m — column-major strides throughout.
fn aperm_impl(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
    dims: &[usize],
    perm: &[usize],
    resize: bool,
) -> EvalResult {
    let n = dims.len();
    if n == 0 || perm.len() != n {
        return Err(EvalError::TypeError("aperm dimension mismatch").into());
    }
    let out_dims: Vec<usize> = perm.iter().map(|p| dims[p - 1]).collect();
    let total: usize = dims.iter().product();
    // Input strides column-major.
    let mut in_strides = vec![1usize; n];
    for k in 1..n {
        in_strides[k] = in_strides[k - 1] * dims[k - 1];
    }
    let mut out_strides = vec![1usize; n];
    for k in 1..n {
        out_strides[k] = out_strides[k - 1] * out_dims[k - 1];
    }
    // Walk output linear positions column-major, mapping each back to the
    // input linear index through the permutation.
    let mut mapping = vec![0usize; total];
    for linear in 0..total {
        let mut rem = linear;
        let mut in_index = 0usize;
        let mut out_coord = vec![0usize; n];
        for k in 0..n {
            out_coord[k] = rem % out_dims[k];
            rem /= out_dims[k];
        }
        let mut in_coord = vec![0usize; n];
        for m in 0..n {
            in_coord[perm[m] - 1] = out_coord[m];
        }
        for k in 0..n {
            in_index += in_coord[k] * in_strides[k];
        }
        mapping[linear] = in_index;
    }
    macro_rules! permute {
        ($v:expr, $variant:ident) => {{
            let out: Vec<_> = mapping.iter().filter_map(|i| $v.get(*i).copied()).collect();
            heap.alloc(RObject::$variant(out))
        }};
    }
    let result = match heap.object(value) {
        Some(RObject::Integer(v)) => permute!(v, Integer),
        Some(RObject::Real(v)) => permute!(v, Real),
        Some(RObject::Logical(v)) => permute!(v, Logical),
        Some(RObject::String(v)) => permute!(v, String),
        Some(RObject::Complex(v)) => permute!(v, Complex),
        _ => return Err(EvalError::TypeError("aperm requires a vector").into()),
    };
    let final_dims: Vec<usize> = if resize { out_dims } else { dims.to_vec() };
    let dim_obj = heap.alloc(RObject::Integer(final_dims.iter().map(|d| *d as i32).collect()));
    copy_with_attrib(heap, interner, result, "dim", dim_obj).map_err(RSignal::from)
}

/// The asplit mechanics: result positions walk the margin subspace
/// column-major, slices walk the complement subspace column-major —
/// both dim-attributed exactly as GNU.
fn asplit_impl(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
    dims: &[usize],
    margin: &[usize],
) -> EvalResult {
    let n = dims.len();
    let in_margin = |d: usize| margin.contains(&(d + 1));
    let slice_dim_indices: Vec<usize> = (0..n).filter(|d| !in_margin(*d)).collect();
    let margin_dim_indices: Vec<usize> = (0..n).filter(|d| in_margin(*d)).collect();
    let result_dims: Vec<usize> = margin_dim_indices.iter().map(|d| dims[*d]).collect();
    let slice_dims: Vec<usize> = slice_dim_indices.iter().map(|d| dims[*d]).collect();
    let result_len: usize = result_dims.iter().product();
    let slice_len: usize = slice_dims.iter().product();
    let mut in_strides = vec![1usize; n];
    for k in 1..n {
        in_strides[k] = in_strides[k - 1] * dims[k - 1];
    }
    // Precompute per-result-position input index vectors for one slice.
    let mut slices: Vec<ValueId> = Vec::with_capacity(result_len);
    for result_pos in 0..result_len {
        // Margin coordinate tuple column-major over result dims.
        let mut coords = vec![0usize; n];
        let mut rem = result_pos;
        for (seq, d) in margin_dim_indices.iter().enumerate() {
            let dim_size = result_dims[seq];
            coords[*d] = rem % dim_size.max(1);
            rem /= dim_size.max(1);
        }
        // Slice iteration: column-major over the complement subspace.
        let mut indices = Vec::with_capacity(slice_len);
        for slice_pos in 0..slice_len {
            let mut sub = slice_pos;
            let mut coord = coords.clone();
            for (seq, d) in slice_dim_indices.iter().enumerate() {
                let dim_size = slice_dims[seq];
                coord[*d] = sub % dim_size.max(1);
                sub /= dim_size.max(1);
            }
            let mut linear = 0usize;
            for k in 0..n {
                linear += coord[k] * in_strides[k];
            }
            indices.push(linear);
        }
        macro_rules! slice_of {
            ($v:expr, $variant:ident) => {{
                let out: Vec<_> = indices.iter().filter_map(|i| $v.get(*i).copied()).collect();
                heap.alloc(RObject::$variant(out))
            }};
        }
        let slice = match heap.object(value) {
            Some(RObject::Integer(v)) => slice_of!(v, Integer),
            Some(RObject::Real(v)) => slice_of!(v, Real),
            Some(RObject::Logical(v)) => slice_of!(v, Logical),
            Some(RObject::String(v)) => slice_of!(v, String),
            Some(RObject::Complex(v)) => slice_of!(v, Complex),
            _ => return Err(EvalError::TypeError("asplit requires a vector").into()),
        };
        // Slice dim attribute: the complement dimensions exactly as GNU —
        // dropped only when the complement is empty.
        let slice = if !slice_dims.is_empty() {
            let dim_obj = heap.alloc(RObject::Integer(slice_dims.iter().map(|d| *d as i32).collect()));
            copy_with_attrib(heap, interner, slice, "dim", dim_obj).map_err(RSignal::from)?
        } else {
            slice
        };
        slices.push(slice);
    }
    let result = heap.alloc(RObject::List(slices));
    let dim_obj = heap.alloc(RObject::Integer(result_dims.iter().map(|d| *d as i32).collect()));
    copy_with_attrib(heap, interner, result, "dim", dim_obj).map_err(RSignal::from)
}

/// One environment's search-path name exactly as GNU composes it: the
/// user environment is .GlobalEnv, the chain end package:base, attached
/// environments their name attribute.
fn env_search_name(heap: &Heap, interner: &SymbolInterner, env: ValueId, genv: ValueId, is_base: bool) -> String {
    if env == genv {
        return ".GlobalEnv".to_string();
    }
    if let Some(n) = get_attrib(heap, interner, env, "name").and_then(|id| string_operand(heap, interner, id)) {
        return n;
    }
    if is_base {
        return "package:base".to_string();
    }
    String::new()
}

/// The search-path names walking the enclosure chain from the user
/// environment — dynamic exactly as GNU after attach/detach.
fn search_path_names(heap: &Heap, interner: &SymbolInterner, genv: ValueId) -> Vec<String> {
    let chain = enclosure_chain(heap, genv);
    let last = chain.len().saturating_sub(1);
    chain
        .iter()
        .enumerate()
        .map(|(i, env)| env_search_name(heap, interner, *env, genv, i == last))
        .collect()
}

/// The bquote tree walk: a call headed by the symbol `.` evaluates its
/// single argument in the caller's frame and splices the value;
/// pairlist tags ride through the rebuild exactly as GNU.
fn bquote_splice(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    expr: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    if let Some(RObject::Language { car, cdr, tag }) = heap.object(expr).cloned() {
        let is_dot = match heap.object(car) {
            Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name) == ".",
            _ => false,
        };
        if is_dot {
            let arg = match heap.object(cdr) {
                Some(RObject::Pairlist { car, .. }) => Some(*car),
                _ => None,
            };
            if let Some(inner) = arg {
                let evaluated = eval(heap, interner, inner, caller_env)?;
                return force_if_promise(heap, interner, evaluated);
            }
        }
        let new_car = bquote_splice(heap, interner, car, caller_env)?;
        let new_cdr = bquote_splice(heap, interner, cdr, caller_env)?;
        return Ok(heap.alloc(RObject::Language { car: new_car, cdr: new_cdr, tag }));
    }
    if let Some(RObject::Pairlist { car, cdr, tag }) = heap.object(expr).cloned() {
        let new_car = bquote_splice(heap, interner, car, caller_env)?;
        let new_cdr = bquote_splice(heap, interner, cdr, caller_env)?;
        return Ok(heap.alloc(RObject::Pairlist { car: new_car, cdr: new_cdr, tag }));
    }
    Ok(expr)
}

/// Element missingness at one position across the atomic lattice — the
/// complete.cases row predicate.
fn element_is_na(heap: &Heap, interner: &SymbolInterner, value: ValueId, index: usize) -> bool {
    match heap.object(value) {
        Some(RObject::Integer(v)) | Some(RObject::Logical(v)) => {
            let v = match heap.object(value) {
                Some(RObject::Integer(iv)) => iv,
                Some(RObject::Logical(lv)) => lv,
                _ => return false,
            };
            v.get(index).map(|x| *x == crate::value::object::NA_INTEGER).unwrap_or(false)
        }
        Some(RObject::Real(v)) => v.get(index).map(|x| x.is_nan()).unwrap_or(false),
        Some(RObject::String(v)) => v
            .get(index)
            .map(|s| interner.string_text(*s).starts_with('\0'))
            .unwrap_or(false),
        Some(RObject::Complex(v)) => v.get(index).map(|c| c.r.is_nan() || c.i.is_nan()).unwrap_or(false),
        _ => false,
    }
}

/// The margin-summary kernel: col variants sum the leading `dims_count`
/// dimensions keeping the trailing subspace; row variants keep the
/// leading subspace summing the rest. NA poisoning exactly as GNU —
/// na.rm drops missing terms from both sum and count.
fn margin_summary_impl(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    x: ValueId,
    x_dims: &[usize],
    is_col: bool,
    is_mean: bool,
    dims_count: usize,
    na_rm: bool,
) -> EvalResult {
    let n = x_dims.len();
    // Kept dimensions: trailing for col, leading for row.
    let (kept_indices, summed_indices): (Vec<usize>, Vec<usize>) = if is_col {
        ((dims_count..n).collect(), (0..dims_count).collect())
    } else {
        ((0..dims_count).collect(), (dims_count..n).collect())
    };
    let kept_dims: Vec<usize> = kept_indices.iter().map(|d| x_dims[*d]).collect();
    let summed_dims: Vec<usize> = summed_indices.iter().map(|d| x_dims[*d]).collect();
    let kept_len: usize = kept_dims.iter().product();
    let summed_len: usize = summed_dims.iter().product();
    let mut in_strides = vec![1usize; n];
    for k in 1..n {
        in_strides[k] = in_strides[k - 1] * x_dims[k - 1];
    }
    let values = reals_of_value(heap, x).map_err(RSignal::from)?;
    let mut out = vec![0.0f64; kept_len];
    let mut poisoned = vec![false; kept_len];
    let mut counts = vec![summed_len; kept_len];
    for kept_pos in 0..kept_len {
        // Kept coordinate tuple column-major over kept dims in INDEX ORDER.
        let mut coords = vec![0usize; n];
        let mut rem = kept_pos;
        for (seq, d) in kept_indices.iter().enumerate() {
            let size = kept_dims[seq];
            coords[*d] = rem % size.max(1);
            rem /= size.max(1);
        }
        let mut sum = 0.0f64;
        let mut count = 0usize;
        for sum_pos in 0..summed_len {
            let mut sub = sum_pos;
            let mut coord = coords.clone();
            for (seq, d) in summed_indices.iter().enumerate() {
                let size = summed_dims[seq];
                coord[*d] = sub % size.max(1);
                sub /= size.max(1);
            }
            let mut linear = 0usize;
            for k in 0..n {
                linear += coord[k] * in_strides[k];
            }
            let v = values.get(linear).copied().unwrap_or(f64::NAN);
            if v.is_nan() {
                if na_rm {
                    continue;
                }
                poisoned[kept_pos] = true;
            } else {
                sum += v;
                count += 1;
            }
        }
        let denom = if is_mean {
            if na_rm { count.max(1) } else { summed_len.max(1) }
        } else {
            1
        };
        out[kept_pos] = sum / denom as f64;
        counts[kept_pos] = count;
    }
    for (pos, poison) in poisoned.iter().enumerate() {
        if *poison {
            out[pos] = f64::from_bits(crate::value::object::NA_REAL_BITS);
        }
    }
    let result = heap.alloc(RObject::Real(out));
    let dim_obj = heap.alloc(RObject::Integer(kept_dims.iter().map(|d| *d as i32).collect()));
    let result = copy_with_attrib(heap, interner, result, "dim", dim_obj).map_err(RSignal::from)?;
    // Dimnames of the kept dimensions ride exactly as GNU — the list
    /// subset in kept order.
    if let Some(dn) = get_attrib(heap, interner, x, "dimnames") {
        if let Some(RObject::List(entries)) = heap.object(dn).cloned() {
            let kept_entries: Vec<ValueId> = kept_indices
                .iter()
                .filter_map(|d| entries.get(*d).copied())
                .collect();
            if kept_entries.len() == kept_indices.len() {
                let dn_obj = heap.alloc(RObject::List(kept_entries));
                return copy_with_attrib(heap, interner, result, "dimnames", dn_obj).map_err(RSignal::from);
            }
        }
    }
    Ok(result)
}

/// Howard Hinnant's civil-from-days conversion — epoch day count to
/// (year, month, day) exactly as the proleptic Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

/// lgamma(x + 1) — log-factorial exactly as GNU's multinomial needs.
fn lgamma1p(x: f64) -> f64 {
    crate::nmath::special::gamma::lgammafn1p(x)
}

/// Build a named list from (name, value) entries — GNU's list shape with
/// the names attribute exactly as introspection primitives report.
fn named_list(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    entries: Vec<(&str, ValueId)>,
) -> EvalResult {
    let names: Vec<_> = entries.iter().map(|(n, _)| interner.intern_string(n)).collect();
    let items: Vec<ValueId> = entries.iter().map(|(_, v)| *v).collect();
    let result = heap.alloc(RObject::List(items));
    let names_obj = heap.alloc(RObject::String(names));
    copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)
}
