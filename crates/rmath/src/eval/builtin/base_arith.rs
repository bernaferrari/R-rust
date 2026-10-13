//! Arithmetic, mathematical, and type-coercion builtin entries.

use super::EvaluatedBuiltin;

pub(super) const BASE_ARITH: &[EvaluatedBuiltin] = &[
    EvaluatedBuiltin {
        name: "+",
        handler: crate::eval::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: "-",
        handler: crate::eval::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: "*",
        handler: crate::eval::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: "/",
        handler: crate::eval::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: "^",
        handler: crate::eval::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: "%%",
        handler: crate::eval::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: "%/%",
        handler: crate::eval::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: ":",
        handler: crate::mainutils::seq::do_colon,
    },
    EvaluatedBuiltin {
        name: "<",
        handler: crate::eval::arithmetic::do_relop,
    },
    EvaluatedBuiltin {
        name: ">",
        handler: crate::eval::arithmetic::do_relop,
    },
    EvaluatedBuiltin {
        name: "<=",
        handler: crate::eval::arithmetic::do_relop,
    },
    EvaluatedBuiltin {
        name: ">=",
        handler: crate::eval::arithmetic::do_relop,
    },
    EvaluatedBuiltin {
        name: "==",
        handler: crate::eval::arithmetic::do_relop,
    },
    EvaluatedBuiltin {
        name: "!=",
        handler: crate::eval::arithmetic::do_relop,
    },
    EvaluatedBuiltin {
        name: "abs",
        handler: crate::mainutils::essentials::do_abs,
    },
    EvaluatedBuiltin {
        name: "sign",
        handler: crate::mainutils::essentials::do_sign,
    },
    EvaluatedBuiltin {
        name: "sqrt",
        handler: crate::eval::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "log",
        handler: crate::eval::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "log10",
        handler: crate::eval::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "exp",
        handler: crate::eval::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "sinh",
        handler: crate::eval::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "cosh",
        handler: crate::eval::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "tanh",
        handler: crate::eval::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "ceiling",
        handler: crate::eval::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "floor",
        handler: crate::eval::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "length",
        handler: crate::eval::arithmetic::do_length,
    },
    EvaluatedBuiltin {
        name: "is.numeric",
        handler: crate::eval::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "is.integer",
        handler: crate::eval::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "is.double",
        handler: crate::eval::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "is.complex",
        handler: crate::eval::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "is.logical",
        handler: crate::eval::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "is.character",
        handler: crate::eval::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "is.null",
        handler: crate::eval::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "is.raw",
        handler: crate::eval::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "identical",
        handler: crate::mainutils::identical::do_identical,
    },
    EvaluatedBuiltin {
        name: "all.equal",
        handler: crate::mainutils::all_equal::do_all_equal,
    },
    EvaluatedBuiltin {
        name: "c",
        handler: crate::mainutils::essentials::do_c,
    },
    EvaluatedBuiltin {
        name: "logical",
        handler: crate::mainutils::essentials::do_logical_constructor,
    },
    EvaluatedBuiltin {
        name: "single",
        handler: crate::mainutils::essentials::do_single_constructor,
    },
    EvaluatedBuiltin {
        name: "rep_len",
        handler: crate::mainutils::seq::do_rep_len,
    },
    EvaluatedBuiltin {
        name: "typeof",
        handler: crate::mainutils::essentials::do_typeof,
    },
    EvaluatedBuiltin {
        name: "is.na",
        handler: crate::mainutils::essentials::do_is_na,
    },
    EvaluatedBuiltin {
        name: "as.integer",
        handler: crate::mainutils::essentials::do_as_integer,
    },
    EvaluatedBuiltin {
        name: "as.double",
        handler: crate::mainutils::essentials::do_as_double,
    },
    EvaluatedBuiltin {
        name: "as.logical",
        handler: crate::mainutils::essentials::do_as_logical,
    },
    EvaluatedBuiltin {
        name: "as.pairlist",
        handler: crate::mainutils::essentials::do_as_pairlist,
    },
    EvaluatedBuiltin {
        name: "as.vector",
        handler: crate::mainutils::essentials::do_as_vector,
    },
    EvaluatedBuiltin {
        name: "as.symbol",
        handler: crate::mainutils::essentials::do_as_symbol,
    },
    EvaluatedBuiltin {
        name: "as.name",
        handler: crate::mainutils::essentials::do_as_symbol,
    },
    EvaluatedBuiltin {
        name: "as.call",
        handler: crate::mainutils::coerce::do_ascall,
    },
    EvaluatedBuiltin {
        name: "as.list",
        handler: crate::mainutils::essentials::do_as_list,
    },
    EvaluatedBuiltin {
        name: "as.list.default",
        handler: crate::mainutils::essentials::do_as_list,
    },
    EvaluatedBuiltin {
        name: "as.list.function",
        handler: crate::mainutils::essentials::do_as_list_function,
    },
    EvaluatedBuiltin {
        name: "as.hexmode",
        handler: crate::mainutils::essentials::do_as_hexmode,
    },
    EvaluatedBuiltin {
        name: "as.octmode",
        handler: crate::mainutils::essentials::do_as_octmode,
    },
    EvaluatedBuiltin {
        name: "expand.grid",
        handler: crate::mainutils::essentials::do_expand_grid,
    },
    EvaluatedBuiltin {
        name: "logLik",
        handler: crate::mainutils::essentials::do_loglik,
    },
    EvaluatedBuiltin {
        name: "loglin",
        handler: crate::mainutils::essentials::do_loglin,
    },
    EvaluatedBuiltin {
        name: "as.hclust",
        handler: crate::mainutils::essentials::do_as_hclust,
    },
    EvaluatedBuiltin {
        name: "as.dendrogram",
        handler: crate::mainutils::essentials::do_as_dendrogram,
    },
    EvaluatedBuiltin {
        name: "is.finite",
        handler: crate::mainutils::essentials::do_is_finite,
    },
    EvaluatedBuiltin {
        name: "is.infinite",
        handler: crate::mainutils::essentials::do_is_infinite,
    },
    EvaluatedBuiltin {
        name: "is.nan",
        handler: crate::mainutils::essentials::do_is_nan,
    },
    EvaluatedBuiltin {
        name: "is.matrix",
        handler: crate::mainutils::essentials::do_is_matrix,
    },
    EvaluatedBuiltin {
        name: "is.array",
        handler: crate::mainutils::essentials::do_is_array,
    },
    EvaluatedBuiltin {
        name: "is.list",
        handler: crate::mainutils::essentials::do_is_list,
    },
    EvaluatedBuiltin {
        name: "lengths",
        handler: crate::mainutils::essentials::do_lengths,
    },
    EvaluatedBuiltin {
        name: "length<-",
        handler: crate::mainutils::essentials::do_length_set,
    },
    EvaluatedBuiltin {
        name: "is.atomic",
        handler: crate::mainutils::essentials::do_is_atomic,
    },
    EvaluatedBuiltin {
        name: "is.recursive",
        handler: crate::mainutils::essentials::do_is_recursive,
    },
    EvaluatedBuiltin {
        name: "is.object",
        handler: crate::mainutils::essentials::do_is_object,
    },
    EvaluatedBuiltin {
        name: "as.data.frame",
        handler: crate::mainutils::essentials::do_as_data_frame,
    },
    EvaluatedBuiltin {
        name: "as.list.environment",
        handler: crate::mainutils::essentials_basic::do_as_list_environment,
    },
    EvaluatedBuiltin {
        name: "args",
        handler: crate::mainutils::essentials::do_args,
    },
    EvaluatedBuiltin {
        name: "signif",
        handler: crate::mainutils::essentials::do_signif,
    },
    EvaluatedBuiltin {
        name: "trunc",
        handler: crate::mainutils::essentials::do_trunc,
    },
    EvaluatedBuiltin {
        name: "log2",
        handler: crate::mainutils::essentials::do_log2,
    },
    EvaluatedBuiltin {
        name: "as.function",
        handler: crate::mainutils::coerce::do_asfunction,
    },
    EvaluatedBuiltin {
        name: "as.function.default",
        handler: crate::mainutils::coerce::do_asfunction,
    },
    EvaluatedBuiltin {
        name: "anyDuplicated",
        handler: crate::mainutils::essentials::do_anyDuplicated,
    },
    EvaluatedBuiltin {
        name: "anyDuplicated.array",
        handler: crate::mainutils::essentials::do_anyDuplicated_array,
    },
    EvaluatedBuiltin {
        name: "match",
        handler: crate::mainutils::essentials::do_match,
    },
    EvaluatedBuiltin {
        name: "is.language",
        handler: crate::mainutils::essentials::do_is_language,
    },
    EvaluatedBuiltin {
        name: "is.call",
        handler: crate::mainutils::essentials::do_is_call,
    },
    EvaluatedBuiltin {
        name: "is.symbol",
        handler: crate::mainutils::essentials::do_is_symbol,
    },
    EvaluatedBuiltin {
        name: "is.name",
        handler: crate::mainutils::essentials::do_is_name,
    },
    EvaluatedBuiltin {
        name: "is.pairlist",
        handler: crate::mainutils::essentials::do_is_pairlist,
    },
    EvaluatedBuiltin {
        name: "is.function",
        handler: crate::mainutils::essentials::do_is_function,
    },
    EvaluatedBuiltin {
        name: "is.expression",
        handler: crate::mainutils::essentials::do_is_expression,
    },
    EvaluatedBuiltin {
        name: "is.environment",
        handler: crate::mainutils::essentials::do_is_environment,
    },
    EvaluatedBuiltin {
        name: "is.single",
        handler: crate::mainutils::essentials::do_is_single,
    },
    EvaluatedBuiltin {
        name: "is.vector",
        handler: crate::mainutils::essentials::do_is_vector,
    },
    EvaluatedBuiltin {
        name: "is.loaded",
        handler: crate::mainutils::essentials::do_is_loaded,
    },
    EvaluatedBuiltin {
        name: "is.primitive",
        handler: crate::mainutils::essentials::do_is_primitive,
    },
    EvaluatedBuiltin {
        name: "is.data.frame",
        handler: crate::mainutils::essentials::do_is_data_frame,
    },
    EvaluatedBuiltin {
        name: "as.complex",
        handler: crate::mainutils::essentials::do_as_complex,
    },
    EvaluatedBuiltin {
        name: "as.raw",
        handler: crate::mainutils::essentials::do_as_raw,
    },
    EvaluatedBuiltin {
        name: "anyNA",
        handler: crate::mainutils::essentials::do_any_na,
    },
    EvaluatedBuiltin {
        name: "as.factor",
        handler: crate::mainutils::essentials::do_as_factor,
    },
    EvaluatedBuiltin {
        name: "is.factor",
        handler: crate::mainutils::essentials::do_is_factor,
    },
    EvaluatedBuiltin {
        name: "match.call",
        handler: crate::mainutils::essentials::do_match_call,
    },
    EvaluatedBuiltin {
        name: "as.matrix",
        handler: crate::mainutils::essentials::do_as_matrix,
    },
    EvaluatedBuiltin {
        name: "as.numeric",
        handler: crate::mainutils::essentials::do_as_numeric,
    },
    EvaluatedBuiltin {
        name: "Re",
        handler: crate::mainutils::complex_cmath::do_cmathfuns,
    },
    EvaluatedBuiltin {
        name: "Im",
        handler: crate::mainutils::complex_cmath::do_cmathfuns,
    },
    EvaluatedBuiltin {
        name: "Mod",
        handler: crate::mainutils::complex_cmath::do_cmathfuns,
    },
    EvaluatedBuiltin {
        name: "Arg",
        handler: crate::mainutils::complex_cmath::do_cmathfuns,
    },
    EvaluatedBuiltin {
        name: "Conj",
        handler: crate::mainutils::complex_cmath::do_cmathfuns,
    },
    EvaluatedBuiltin {
        name: "sin",
        handler: crate::mainutils::essentials::do_sin,
    },
    EvaluatedBuiltin {
        name: "cos",
        handler: crate::mainutils::essentials::do_cos,
    },
    EvaluatedBuiltin {
        name: "tan",
        handler: crate::mainutils::essentials::do_tan,
    },
    EvaluatedBuiltin {
        name: "atan",
        handler: crate::mainutils::essentials::do_atan,
    },
    EvaluatedBuiltin {
        name: "atan2",
        handler: crate::mainutils::essentials::do_atan2,
    },
    EvaluatedBuiltin {
        name: "expm1",
        handler: crate::mainutils::essentials::do_expm1,
    },
    EvaluatedBuiltin {
        name: "log1p",
        handler: crate::mainutils::essentials::do_log1p,
    },
    EvaluatedBuiltin {
        name: "atanh",
        handler: crate::mainutils::essentials::do_atanh,
    },
    EvaluatedBuiltin {
        name: "cospi",
        handler: crate::mainutils::essentials::do_cospi,
    },
    EvaluatedBuiltin {
        name: "sinpi",
        handler: crate::mainutils::essentials::do_sinpi,
    },
    EvaluatedBuiltin {
        name: "tanpi",
        handler: crate::mainutils::essentials::do_tanpi,
    },
    EvaluatedBuiltin {
        name: "lgamma",
        handler: crate::mainutils::essentials::do_lgamma,
    },
    EvaluatedBuiltin {
        name: "gamma",
        handler: crate::mainutils::essentials::do_gamma,
    },
    EvaluatedBuiltin {
        name: "trigamma",
        handler: crate::mainutils::essentials::do_trigamma,
    },
    EvaluatedBuiltin {
        name: "as.environment",
        handler: crate::mainutils::essentials::do_as_environment,
    },
    EvaluatedBuiltin {
        name: "match.fun",
        handler: crate::mainutils::essentials::do_match_fun,
    },
    EvaluatedBuiltin {
        name: "any",
        handler: crate::mainutils::essentials::do_any,
    },
    EvaluatedBuiltin {
        name: "all",
        handler: crate::mainutils::essentials::do_all,
    },
    EvaluatedBuiltin {
        name: "unique",
        handler: crate::mainutils::essentials::do_unique,
    },
    EvaluatedBuiltin {
        name: "%in%",
        handler: crate::mainutils::essentials::do_in_operator,
    },
];
