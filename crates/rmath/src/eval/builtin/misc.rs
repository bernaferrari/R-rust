//! Miscellaneous builtin entries.

use super::EvaluatedBuiltin;

pub(super) const MISC: &[EvaluatedBuiltin] = &[
    EvaluatedBuiltin {
        name: "eigen",
        handler: crate::mainutils::eigen::do_eigen,
    },
    EvaluatedBuiltin {
        name: "bcVersion",
        handler: crate::eval::compiler::do_bcversion,
    },
    EvaluatedBuiltin {
        name: "enableJIT",
        handler: crate::eval::compiler::do_enable_jit,
    },
    EvaluatedBuiltin {
        name: "growconst",
        handler: crate::eval::compiler::do_growconst,
    },
    EvaluatedBuiltin {
        name: "getconst",
        handler: crate::eval::compiler::do_getconst,
    },
    EvaluatedBuiltin {
        name: "cmpfun",
        handler: crate::eval::compiler::do_cmpfun,
    },
    EvaluatedBuiltin {
        name: "compile",
        handler: crate::eval::compiler::do_compile,
    },
    EvaluatedBuiltin {
        name: ".rport_pretty",
        handler: crate::mainutils::pretty::pretty_values,
    },
    EvaluatedBuiltin {
        name: ".pretty",
        handler: crate::mainutils::pretty::dot_pretty,
    },
    EvaluatedBuiltin {
        name: "...elt",
        handler: crate::eval::missing::do_dots_elt,
    },
    EvaluatedBuiltin {
        name: "...length",
        handler: crate::eval::missing::do_dots_length,
    },
    EvaluatedBuiltin {
        name: "...names",
        handler: crate::eval::missing::do_dots_names,
    },
    EvaluatedBuiltin {
        name: ".primTrace",
        handler: crate::mainutils::debug::do_trace,
    },
    EvaluatedBuiltin {
        name: ".primUntrace",
        handler: crate::mainutils::debug::do_trace,
    },
    EvaluatedBuiltin {
        name: "tracingState",
        handler: crate::mainutils::debug::do_tracing_state,
    },
    EvaluatedBuiltin {
        name: "tracemem",
        handler: crate::mainutils::debug::do_tracemem,
    },
    EvaluatedBuiltin {
        name: "untracemem",
        handler: crate::mainutils::debug::do_untracemem,
    },
    EvaluatedBuiltin {
        name: ".isMethodsDispatchOn",
        handler: crate::mainutils::objects::do_S4on,
    },
    EvaluatedBuiltin {
        name: ".cache_class",
        handler: crate::mainutils::essentials::do_cache_class,
    },
    EvaluatedBuiltin {
        name: ".Primitive",
        handler: crate::mainutils::names::do_primitive,
    },
    EvaluatedBuiltin {
        name: ".External.graphics",
        handler: crate::mainutils::dotcode::do_Externalgr,
    },
    EvaluatedBuiltin {
        name: "browser",
        handler: crate::eval::context::do_browser,
    },
    EvaluatedBuiltin {
        name: "environment<-",
        handler: crate::mainutils::builtin::do_envirgets,
    },
    EvaluatedBuiltin {
        name: "xtfrm",
        handler: crate::mainutils::essentials::do_xtfrm,
    },
    EvaluatedBuiltin {
        name: "aperm",
        handler: crate::mainutils::array::do_aperm,
    },
    EvaluatedBuiltin {
        name: "backsolve",
        handler: crate::mainutils::array::do_backsolve,
    },
    EvaluatedBuiltin {
        name: "forwardsolve",
        handler: crate::mainutils::essentials::do_forwardsolve,
    },
    EvaluatedBuiltin {
        name: ".C",
        handler: crate::mainutils::dotcode::do_dotCode,
    },
    EvaluatedBuiltin {
        name: ".Fortran",
        handler: crate::mainutils::dotcode::do_dotCode,
    },
    EvaluatedBuiltin {
        name: ".Call",
        handler: crate::mainutils::dotcode::do_dotcall,
    },
    EvaluatedBuiltin {
        name: ".Call.graphics",
        handler: crate::mainutils::dotcode::do_dotcall,
    },
    EvaluatedBuiltin {
        name: ".External",
        handler: crate::mainutils::dotcode::do_External,
    },
    EvaluatedBuiltin {
        name: ".External2",
        handler: crate::mainutils::dotcode::do_External,
    },
    EvaluatedBuiltin {
        name: "&",
        handler: crate::mainutils::logic::do_logic,
    },
    EvaluatedBuiltin {
        name: "|",
        handler: crate::mainutils::logic::do_logic,
    },
    EvaluatedBuiltin {
        name: "!",
        handler: crate::mainutils::logic::do_logic,
    },
    EvaluatedBuiltin {
        name: "xor",
        handler: crate::mainutils::logic::do_xor,
    },
    EvaluatedBuiltin {
        name: "bitwAnd",
        handler: crate::mainutils::relop::do_bitwAnd,
    },
    EvaluatedBuiltin {
        name: "bitwOr",
        handler: crate::mainutils::relop::do_bitwOr,
    },
    EvaluatedBuiltin {
        name: "bitwXor",
        handler: crate::mainutils::relop::do_bitwXor,
    },
    EvaluatedBuiltin {
        name: "bitwNot",
        handler: crate::mainutils::relop::do_bitwNot,
    },
    EvaluatedBuiltin {
        name: "bitwShiftL",
        handler: crate::mainutils::relop::do_bitwShiftL,
    },
    EvaluatedBuiltin {
        name: "bitwShiftR",
        handler: crate::mainutils::relop::do_bitwShiftR,
    },
    EvaluatedBuiltin {
        name: "integer",
        handler: crate::mainutils::essentials::do_integer_constructor,
    },
    EvaluatedBuiltin {
        name: "numeric",
        handler: crate::mainutils::essentials::do_numeric_constructor,
    },
    EvaluatedBuiltin {
        name: "complex",
        handler: crate::mainutils::essentials::do_complex_constructor,
    },
    EvaluatedBuiltin {
        name: "vector",
        handler: crate::mainutils::essentials::do_vector_constructor,
    },
    EvaluatedBuiltin {
        name: "mode",
        handler: crate::mainutils::essentials::do_mode,
    },
    EvaluatedBuiltin {
        name: "storage.mode",
        handler: crate::mainutils::essentials::do_storage_mode_get,
    },
    EvaluatedBuiltin {
        name: "storage.mode<-",
        handler: crate::mainutils::essentials::do_storage_mode_set,
    },
    EvaluatedBuiltin {
        name: "mode<-",
        handler: crate::mainutils::essentials::do_storage_mode_set,
    },
    EvaluatedBuiltin {
        name: "identity",
        handler: crate::mainutils::essentials::do_identity,
    },
    EvaluatedBuiltin {
        name: "globalCallingHandlers",
        handler: crate::mainutils::essentials::do_globalCallingHandlers,
    },
    EvaluatedBuiltin {
        name: "names",
        handler: crate::mainutils::essentials::do_names,
    },
    EvaluatedBuiltin {
        name: "unname",
        handler: crate::mainutils::essentials::do_unname,
    },
    EvaluatedBuiltin {
        name: "unclass",
        handler: crate::mainutils::essentials::do_unclass,
    },
    EvaluatedBuiltin {
        name: "oldClass",
        handler: crate::mainutils::essentials::do_oldClass,
    },
    EvaluatedBuiltin {
        name: "ifelse",
        handler: crate::mainutils::essentials::do_ifelse,
    },
    EvaluatedBuiltin {
        name: "sessionInfo",
        handler: crate::mainutils::essentials::do_sessionInfo,
    },
    EvaluatedBuiltin {
        name: "Encoding",
        handler: crate::mainutils::essentials::do_encoding,
    },
    EvaluatedBuiltin {
        name: "Encoding<-",
        handler: crate::mainutils::essentials::do_setencoding,
    },
    EvaluatedBuiltin {
        name: "make.unique",
        handler: crate::mainutils::essentials::do_make_unique,
    },
    EvaluatedBuiltin {
        name: "glob2rx",
        handler: crate::mainutils::essentials::do_glob2rx,
    },
    EvaluatedBuiltin {
        name: "aregexec",
        handler: crate::mainutils::essentials::do_aregexec,
    },
    EvaluatedBuiltin {
        name: "intToBits",
        handler: crate::mainutils::essentials::do_intToBits,
    },
    EvaluatedBuiltin {
        name: "numToInts",
        handler: crate::mainutils::essentials::do_numToInts,
    },
    EvaluatedBuiltin {
        name: "numToBits",
        handler: crate::mainutils::essentials::do_numToBits,
    },
    EvaluatedBuiltin {
        name: "gettext",
        handler: crate::mainutils::essentials::do_gettext,
    },
    EvaluatedBuiltin {
        name: "ngettext",
        handler: crate::mainutils::essentials::do_ngettext,
    },
    EvaluatedBuiltin {
        name: "bindtextdomain",
        handler: crate::mainutils::essentials::do_bindtextdomain,
    },
    EvaluatedBuiltin {
        name: "nclass.Sturges",
        handler: crate::mainutils::essentials::do_nclass_sturges,
    },
    EvaluatedBuiltin {
        name: "nclass.scott",
        handler: crate::mainutils::essentials::do_nclass_scott,
    },
    EvaluatedBuiltin {
        name: "nclass.FD",
        handler: crate::mainutils::essentials::do_nclass_fd,
    },
    EvaluatedBuiltin {
        name: "bw.nrd0",
        handler: crate::mainutils::essentials::do_bw_nrd0,
    },
    EvaluatedBuiltin {
        name: "bw.nrd",
        handler: crate::mainutils::essentials::do_bw_nrd,
    },
    EvaluatedBuiltin {
        name: "bw.SJ",
        handler: crate::mainutils::essentials::do_bw_sj,
    },
    EvaluatedBuiltin {
        name: "bw.ucv",
        handler: crate::mainutils::essentials::do_bw_ucv,
    },
    EvaluatedBuiltin {
        name: "bw.bcv",
        handler: crate::mainutils::essentials::do_bw_bcv,
    },
    EvaluatedBuiltin {
        name: "localeToCharset",
        handler: crate::mainutils::essentials::do_localeToCharset,
    },
    EvaluatedBuiltin {
        name: "units",
        handler: crate::mainutils::essentials::do_units,
    },
    EvaluatedBuiltin {
        name: "enc2native",
        handler: crate::mainutils::essentials::do_enc2native,
    },
    EvaluatedBuiltin {
        name: "set.seed",
        handler: crate::mainutils::random::do_setseed,
    },
    EvaluatedBuiltin {
        name: "RNGkind",
        handler: crate::mainutils::random::do_RNGkind,
    },
    EvaluatedBuiltin {
        name: "RNGversion",
        handler: crate::mainutils::random::do_RNGversion,
    },
    EvaluatedBuiltin {
        name: "q",
        handler: crate::mainutils::essentials::do_quit,
    },
    EvaluatedBuiltin {
        name: "sQuote",
        handler: crate::mainutils::essentials::do_sQuote,
    },
    EvaluatedBuiltin {
        name: ".rport_loess_fit",
        handler: crate::mainutils::loess::do_fit,
    },
    EvaluatedBuiltin {
        name: "box",
        handler: crate::mainutils::essentials::do_box,
    },
    EvaluatedBuiltin {
        name: "scatter.smooth",
        handler: crate::mainutils::essentials::do_scatter_smooth,
    },
    EvaluatedBuiltin {
        name: "mapply",
        handler: crate::mainutils::mapply::do_mapply,
    },
    EvaluatedBuiltin {
        name: "outer",
        handler: crate::mainutils::essentials::do_outer,
    },
    EvaluatedBuiltin {
        name: "merge",
        handler: crate::mainutils::essentials::do_merge,
    },
    EvaluatedBuiltin {
        name: "jitter",
        handler: crate::mainutils::essentials::do_jitter,
    },
    EvaluatedBuiltin {
        name: "t.test",
        handler: crate::mainutils::essentials::do_t_test,
    },
    EvaluatedBuiltin {
        name: "binom.test",
        handler: crate::mainutils::essentials::do_binom_test,
    },
    EvaluatedBuiltin {
        name: "bartlett.test",
        handler: crate::mainutils::essentials::do_bartlett_test,
    },
    EvaluatedBuiltin {
        name: "fligner.test",
        handler: crate::mainutils::essentials::do_fligner_test,
    },
    EvaluatedBuiltin {
        name: "mood.test",
        handler: crate::mainutils::essentials::do_mood_test,
    },
    EvaluatedBuiltin {
        name: "ansari.test",
        handler: crate::mainutils::essentials::do_ansari_test,
    },
    EvaluatedBuiltin {
        name: "mcnemar.test",
        handler: crate::mainutils::essentials::do_mcnemar_test,
    },
    EvaluatedBuiltin {
        name: "friedman.test",
        handler: crate::mainutils::essentials::do_friedman_test,
    },
    EvaluatedBuiltin {
        name: "mantelhaen.test",
        handler: crate::mainutils::essentials::do_mantelhaen_test,
    },
    EvaluatedBuiltin {
        name: "factanal",
        handler: crate::mainutils::essentials::do_factanal,
    },
    EvaluatedBuiltin {
        name: "oneway.test",
        handler: crate::mainutils::essentials::do_oneway_test,
    },
    EvaluatedBuiltin {
        name: "AIC.default",
        handler: crate::mainutils::essentials::do_aic,
    },
    EvaluatedBuiltin {
        name: "BIC",
        handler: crate::mainutils::essentials::do_bic,
    },
    EvaluatedBuiltin {
        name: "sigma",
        handler: crate::mainutils::essentials::do_sigma,
    },
    EvaluatedBuiltin {
        name: "extractAIC",
        handler: crate::mainutils::essentials::do_extract_aic,
    },
    EvaluatedBuiltin {
        name: "case.names",
        handler: crate::mainutils::essentials::do_case_names,
    },
    EvaluatedBuiltin {
        name: "confint",
        handler: crate::mainutils::essentials::do_confint,
    },
    EvaluatedBuiltin {
        name: "confint.default",
        handler: crate::mainutils::essentials::do_confint_default,
    },
    EvaluatedBuiltin {
        name: "hat",
        handler: crate::mainutils::essentials::do_hat,
    },
    EvaluatedBuiltin {
        name: "effects",
        handler: crate::mainutils::essentials::do_effects,
    },
    EvaluatedBuiltin {
        name: "numericDeriv",
        handler: crate::mainutils::essentials::do_numeric_deriv,
    },
    EvaluatedBuiltin {
        name: "manova",
        handler: crate::mainutils::essentials::do_manova,
    },
    EvaluatedBuiltin {
        name: "nls",
        handler: crate::mainutils::essentials::do_nls,
    },
    EvaluatedBuiltin {
        name: "nls.control",
        handler: crate::mainutils::essentials::do_nls_control,
    },
    EvaluatedBuiltin {
        name: "SSlogis",
        handler: crate::mainutils::essentials::do_sslogis,
    },
    EvaluatedBuiltin {
        name: "SSasymp",
        handler: crate::mainutils::essentials::do_ssasymp,
    },
    EvaluatedBuiltin {
        name: "SSmicmen",
        handler: crate::mainutils::essentials::do_ssmicmen,
    },
    EvaluatedBuiltin {
        name: "SSgompertz",
        handler: crate::mainutils::essentials::do_ssgompertz,
    },
    EvaluatedBuiltin {
        name: "SSbiexp",
        handler: crate::mainutils::essentials::do_ssbiexp,
    },
    EvaluatedBuiltin {
        name: "SSasympOff",
        handler: crate::mainutils::essentials::do_ssasymp_off,
    },
    EvaluatedBuiltin {
        name: "SSasympOrig",
        handler: crate::mainutils::essentials::do_ssasymp_orig,
    },
    EvaluatedBuiltin {
        name: "SSfol",
        handler: crate::mainutils::essentials::do_ssfol,
    },
    EvaluatedBuiltin {
        name: "SSweibull",
        handler: crate::mainutils::essentials::do_ssweibull,
    },
    EvaluatedBuiltin {
        name: "SSfpl",
        handler: crate::mainutils::essentials::do_ssfpl,
    },
    EvaluatedBuiltin {
        name: "NLSstLfAsymptote",
        handler: crate::mainutils::essentials::do_nls_lf_asymp,
    },
    EvaluatedBuiltin {
        name: "NLSstRtAsymptote",
        handler: crate::mainutils::essentials::do_nls_rt_asymp,
    },
    EvaluatedBuiltin {
        name: "NLSstAsymptotic",
        handler: crate::mainutils::essentials::do_nls_asymptotic,
    },
    EvaluatedBuiltin {
        name: "NLSstClosestX",
        handler: crate::mainutils::essentials::do_nls_closest_x,
    },
    EvaluatedBuiltin {
        name: "lsfit",
        handler: crate::mainutils::essentials::do_lsfit,
    },
    EvaluatedBuiltin {
        name: "ls.diag",
        handler: crate::mainutils::essentials::do_ls_diag,
    },
    EvaluatedBuiltin {
        name: "symnum",
        handler: crate::mainutils::essentials::do_symnum,
    },
    EvaluatedBuiltin {
        name: "simulate",
        handler: crate::mainutils::essentials::do_simulate,
    },
    EvaluatedBuiltin {
        name: "mauchly.test",
        handler: crate::mainutils::essentials::do_mauchly_test,
    },
    EvaluatedBuiltin {
        name: "selfStart",
        handler: crate::mainutils::essentials::do_self_start,
    },
    EvaluatedBuiltin {
        name: "getInitial",
        handler: crate::mainutils::essentials::do_get_initial,
    },
    EvaluatedBuiltin {
        name: "stat.anova",
        handler: crate::mainutils::essentials::do_stat_anova,
    },
    EvaluatedBuiltin {
        name: "binomial",
        handler: crate::mainutils::essentials::do_binomial,
    },
    EvaluatedBuiltin {
        name: "gaussian",
        handler: crate::mainutils::essentials::do_gaussian,
    },
    EvaluatedBuiltin {
        name: "Gamma",
        handler: crate::mainutils::essentials::do_gamma_family,
    },
    EvaluatedBuiltin {
        name: "inverse.gaussian",
        handler: crate::mainutils::essentials::do_inverse_gaussian,
    },
    EvaluatedBuiltin {
        name: "make.link",
        handler: crate::mainutils::essentials::do_make_link,
    },
    EvaluatedBuiltin {
        name: "Pair",
        handler: crate::mainutils::essentials::do_pair,
    },
    EvaluatedBuiltin {
        name: "family",
        handler: crate::mainutils::essentials::do_family,
    },
    EvaluatedBuiltin {
        name: "influence",
        handler: crate::mainutils::essentials::do_influence,
    },
    EvaluatedBuiltin {
        name: "PP.test",
        handler: crate::mainutils::essentials::do_pp_test,
    },
    EvaluatedBuiltin {
        name: "hclust",
        handler: crate::mainutils::essentials::do_hclust,
    },
    EvaluatedBuiltin {
        name: ".rport_as_dendrogram",
        handler: crate::mainutils::essentials::do_as_dendrogram,
    },
    EvaluatedBuiltin {
        name: "heatmap",
        handler: crate::mainutils::essentials::do_heatmap,
    },
    EvaluatedBuiltin {
        name: "cophenetic",
        handler: crate::mainutils::essentials::do_cophenetic,
    },
    EvaluatedBuiltin {
        name: "knots",
        handler: crate::mainutils::essentials::do_knots,
    },
    EvaluatedBuiltin {
        name: "cpgram",
        handler: crate::mainutils::essentials::do_cpgram,
    },
    EvaluatedBuiltin {
        name: ".ecdf_apply",
        handler: crate::mainutils::essentials::do_ecdf_apply,
    },
    EvaluatedBuiltin {
        name: "medpolish",
        handler: crate::mainutils::essentials::do_medpolish,
    },
    EvaluatedBuiltin {
        name: "stepfun",
        handler: crate::mainutils::essentials::do_stepfun,
    },
    EvaluatedBuiltin {
        name: ".stepfun_apply",
        handler: crate::mainutils::essentials::do_stepfun_apply,
    },
    EvaluatedBuiltin {
        name: ".splinefunH_apply",
        handler: crate::mainutils::essentials::do_splinefun_h_apply,
    },
    EvaluatedBuiltin {
        name: "add1",
        handler: crate::mainutils::essentials::do_add1,
    },
    EvaluatedBuiltin {
        name: "step",
        handler: crate::mainutils::essentials::do_step,
    },
    EvaluatedBuiltin {
        name: "ks.test",
        handler: crate::mainutils::essentials::do_ks_test,
    },
    EvaluatedBuiltin {
        name: "Box.test",
        handler: crate::mainutils::essentials::do_box_test,
    },
    EvaluatedBuiltin {
        name: "embed",
        handler: crate::mainutils::essentials::do_embed,
    },
    EvaluatedBuiltin {
        name: ".rport_toeplitz",
        handler: crate::mainutils::essentials::do_toeplitz,
    },
    EvaluatedBuiltin {
        name: "toeplitz2",
        handler: crate::mainutils::essentials::do_toeplitz2,
    },
    EvaluatedBuiltin {
        name: "D",
        handler: crate::mainutils::essentials::do_D,
    },
    EvaluatedBuiltin {
        name: "kronecker",
        handler: crate::mainutils::essentials::do_kronecker,
    },
    EvaluatedBuiltin {
        name: ".kronecker",
        handler: crate::mainutils::essentials::do_kronecker,
    },
    EvaluatedBuiltin {
        name: "pcre_config",
        handler: crate::mainutils::essentials::do_pcre_config,
    },
    EvaluatedBuiltin {
        name: "shortRowNames",
        handler: crate::mainutils::attrib::do_shortRowNames,
    },
    EvaluatedBuiltin {
        name: "append",
        handler: crate::mainutils::essentials::do_append,
    },
    EvaluatedBuiltin {
        name: ".subset",
        handler: crate::mainutils::subset::do_subset_dflt,
    },
    EvaluatedBuiltin {
        name: ".subset2",
        handler: crate::mainutils::subset::do_subset2_dflt,
    },
    EvaluatedBuiltin {
        name: "union",
        handler: crate::mainutils::essentials::do_union,
    },
    EvaluatedBuiltin {
        name: "intersect",
        handler: crate::mainutils::essentials::do_intersect,
    },
    EvaluatedBuiltin {
        name: "setequal",
        handler: crate::mainutils::essentials::do_setequal,
    },
    EvaluatedBuiltin {
        name: "NROW",
        handler: crate::mainutils::essentials::do_NROW,
    },
    EvaluatedBuiltin {
        name: "NCOL",
        handler: crate::mainutils::essentials::do_NCOL,
    },
    EvaluatedBuiltin {
        name: "class",
        handler: crate::mainutils::essentials::do_class_get,
    },
    EvaluatedBuiltin {
        name: ".class2",
        handler: crate::mainutils::essentials::do_class2,
    },
    EvaluatedBuiltin {
        name: "list",
        handler: crate::mainutils::essentials::do_list,
    },
    EvaluatedBuiltin {
        name: "attr",
        handler: crate::mainutils::essentials::do_attr,
    },
    EvaluatedBuiltin {
        name: "attributes",
        handler: crate::mainutils::essentials::do_attributes,
    },
    EvaluatedBuiltin {
        name: "comment",
        handler: crate::mainutils::essentials::do_comment,
    },
    EvaluatedBuiltin {
        name: "structure",
        handler: crate::mainutils::essentials::do_structure,
    },
    EvaluatedBuiltin {
        name: "nargs",
        handler: crate::mainutils::essentials::do_nargs,
    },
    EvaluatedBuiltin {
        name: "UseMethod",
        handler: crate::mainutils::objects::do_usemethod,
    },
    EvaluatedBuiltin {
        name: "gettextf",
        handler: crate::mainutils::essentials::do_gettextf,
    },
    EvaluatedBuiltin {
        name: "topenv",
        handler: crate::mainutils::objects::do_topenv,
    },
    EvaluatedBuiltin {
        name: "sys.call",
        handler: crate::eval::context::do_sys,
    },
    EvaluatedBuiltin {
        name: "sys.frame",
        handler: crate::eval::context::do_sys,
    },
    EvaluatedBuiltin {
        name: "sys.parent",
        handler: crate::eval::context::do_sys,
    },
    EvaluatedBuiltin {
        name: "sys.calls",
        handler: crate::eval::context::do_sys,
    },
    EvaluatedBuiltin {
        name: "sys.frames",
        handler: crate::eval::context::do_sys,
    },
    EvaluatedBuiltin {
        name: "sys.parents",
        handler: crate::eval::context::do_sys,
    },
    EvaluatedBuiltin {
        name: "sys.on.exit",
        handler: crate::eval::context::do_sys,
    },
    EvaluatedBuiltin {
        name: "getwd",
        handler: crate::mainutils::essentials::do_getwd,
    },
    EvaluatedBuiltin {
        name: "setwd",
        handler: crate::mainutils::essentials::do_setwd,
    },
    EvaluatedBuiltin {
        name: "basename",
        handler: crate::mainutils::essentials::do_basename,
    },
    EvaluatedBuiltin {
        name: "unlink",
        handler: crate::mainutils::essentials::do_unlink,
    },
    EvaluatedBuiltin {
        name: "nzchar",
        handler: crate::mainutils::essentials::do_nzchar,
    },
    EvaluatedBuiltin {
        name: "lapply",
        handler: crate::mainutils::essentials::do_lapply,
    },
    EvaluatedBuiltin {
        name: "sapply",
        handler: crate::mainutils::essentials::do_sapply,
    },
    EvaluatedBuiltin {
        name: "vapply",
        handler: crate::mainutils::essentials::do_vapply,
    },
    EvaluatedBuiltin {
        name: "Map",
        handler: crate::mainutils::essentials::do_map,
    },
    EvaluatedBuiltin {
        name: "Filter",
        handler: crate::mainutils::essentials::do_filter,
    },
    EvaluatedBuiltin {
        name: "close",
        handler: crate::mainutils::essentials::do_close,
    },
    EvaluatedBuiltin {
        name: "fifo",
        handler: crate::mainutils::essentials::do_fifo,
    },
    EvaluatedBuiltin {
        name: "isOpen",
        handler: crate::mainutils::essentials::do_isOpen,
    },
    EvaluatedBuiltin {
        name: "isIncomplete",
        handler: crate::mainutils::essentials::do_isIncomplete,
    },
    EvaluatedBuiltin {
        name: "isSeekable",
        handler: crate::mainutils::essentials::do_isSeekable,
    },
    EvaluatedBuiltin {
        name: "str",
        handler: crate::mainutils::essentials::do_str,
    },
    EvaluatedBuiltin {
        name: "ls.str",
        handler: crate::mainutils::essentials::do_ls_str,
    },
    EvaluatedBuiltin {
        name: "chol",
        handler: crate::mainutils::essentials::do_chol,
    },
    EvaluatedBuiltin {
        name: "chol2inv",
        handler: crate::mainutils::essentials::do_chol2inv,
    },
    EvaluatedBuiltin {
        name: "norm",
        handler: crate::mainutils::essentials::do_norm,
    },
    EvaluatedBuiltin {
        name: "kappa",
        handler: crate::mainutils::essentials::do_kappa,
    },
    EvaluatedBuiltin {
        name: "svd",
        handler: crate::mainutils::essentials::do_svd,
    },
    EvaluatedBuiltin {
        name: "det_ge_real",
        handler: crate::mainutils::essentials::do_det_ge_real,
    },
    EvaluatedBuiltin {
        name: "La_svd",
        handler: crate::mainutils::essentials::do_la_svd,
    },
    EvaluatedBuiltin {
        name: "La_svd_cmplx",
        handler: crate::mainutils::essentials::do_la_svd_cmplx,
    },
    EvaluatedBuiltin {
        name: "La_dlange",
        handler: crate::mainutils::essentials::do_la_dlange,
    },
    EvaluatedBuiltin {
        name: "La_dgecon",
        handler: crate::mainutils::essentials::do_la_dgecon,
    },
    EvaluatedBuiltin {
        name: "La_zlange",
        handler: crate::mainutils::essentials::do_la_zlange,
    },
    EvaluatedBuiltin {
        name: "La_zgecon",
        handler: crate::mainutils::essentials::do_la_zgecon,
    },
    EvaluatedBuiltin {
        name: "La_dtrcon",
        handler: crate::mainutils::essentials::do_la_dtrcon,
    },
    EvaluatedBuiltin {
        name: "La_dtrcon3",
        handler: crate::mainutils::essentials::do_la_dtrcon3,
    },
    EvaluatedBuiltin {
        name: "La_ztrcon",
        handler: crate::mainutils::essentials::do_la_ztrcon,
    },
    EvaluatedBuiltin {
        name: "La_ztrcon3",
        handler: crate::mainutils::essentials::do_la_ztrcon3,
    },
    EvaluatedBuiltin {
        name: "solve",
        handler: crate::mainutils::essentials::do_solve,
    },
    EvaluatedBuiltin {
        name: "emptyenv",
        handler: crate::mainutils::essentials::do_emptyenv,
    },
    EvaluatedBuiltin {
        name: "baseenv",
        handler: crate::mainutils::essentials::do_baseenv,
    },
    EvaluatedBuiltin {
        name: "globalenv",
        handler: crate::mainutils::essentials::do_globalenv,
    },
    EvaluatedBuiltin {
        name: "new.env",
        handler: crate::mainutils::essentials::do_new_env,
    },
    EvaluatedBuiltin {
        name: "list2env",
        handler: crate::mainutils::essentials::do_list2env,
    },
    EvaluatedBuiltin {
        name: "get0",
        handler: crate::mainutils::essentials::do_get0,
    },
    EvaluatedBuiltin {
        name: "mget",
        handler: crate::mainutils::essentials::do_mget,
    },
    EvaluatedBuiltin {
        name: "environment",
        handler: crate::mainutils::essentials::do_environment,
    },
    EvaluatedBuiltin {
        name: "lockBinding",
        handler: crate::mainutils::essentials::do_lockBinding,
    },
    EvaluatedBuiltin {
        name: "unlockBinding",
        handler: crate::mainutils::essentials::do_unlockBinding,
    },
    EvaluatedBuiltin {
        name: "bindingIsLocked",
        handler: crate::mainutils::essentials::do_bindingIsLocked,
    },
    EvaluatedBuiltin {
        name: "bindingIsActive",
        handler: crate::mainutils::essentials::do_bindingIsActive,
    },
    EvaluatedBuiltin {
        name: "makeActiveBinding",
        handler: crate::mainutils::essentials::do_makeActiveBinding,
    },
    EvaluatedBuiltin {
        name: "lockEnvironment",
        handler: crate::mainutils::essentials::do_lockEnvironment,
    },
    EvaluatedBuiltin {
        name: "environmentIsLocked",
        handler: crate::mainutils::essentials::do_environmentIsLocked,
    },
    EvaluatedBuiltin {
        name: "formals",
        handler: crate::mainutils::essentials::do_formals,
    },
    EvaluatedBuiltin {
        name: "body",
        handler: crate::mainutils::essentials::do_body,
    },
    EvaluatedBuiltin {
        name: "bodyCode",
        handler: crate::mainutils::builtin::do_bodyCode,
    },
    EvaluatedBuiltin {
        name: "charmatch",
        handler: crate::mainutils::essentials::do_charmatch,
    },
    EvaluatedBuiltin {
        name: "strtoi",
        handler: crate::mainutils::essentials::do_strtoi,
    },
    EvaluatedBuiltin {
        name: "strtrim",
        handler: crate::mainutils::essentials::do_strtrim,
    },
    EvaluatedBuiltin {
        name: "zapsmall",
        handler: crate::mainutils::essentials::do_zapsmall,
    },
    EvaluatedBuiltin {
        name: "eval",
        handler: crate::mainutils::essentials::do_eval,
    },
    EvaluatedBuiltin {
        name: "simpleError",
        handler: crate::mainutils::essentials::do_simpleError,
    },
    EvaluatedBuiltin {
        name: "simpleWarning",
        handler: crate::mainutils::essentials::do_simpleWarning,
    },
    EvaluatedBuiltin {
        name: "simpleCondition",
        handler: crate::mainutils::essentials::do_simpleCondition,
    },
    EvaluatedBuiltin {
        name: "objsxp",
        handler: crate::mainutils::objects::do_objsxp,
    },
    EvaluatedBuiltin {
        name: ".OBJSXP",
        handler: crate::mainutils::objects::do_objsxp,
    },
    EvaluatedBuiltin {
        name: "setClass",
        handler: crate::mainutils::essentials::do_setClass,
    },
    EvaluatedBuiltin {
        name: "setValidity",
        handler: crate::mainutils::essentials::do_setValidity,
    },
    EvaluatedBuiltin {
        name: "isVirtualClass",
        handler: crate::mainutils::essentials::do_isVirtualClass,
    },
    EvaluatedBuiltin {
        name: "new",
        handler: crate::mainutils::essentials::do_new,
    },
    EvaluatedBuiltin {
        name: "validObject",
        handler: crate::mainutils::essentials::do_validObject,
    },
    EvaluatedBuiltin {
        name: "show",
        handler: crate::mainutils::essentials::do_show,
    },
    EvaluatedBuiltin {
        name: "slotNames",
        handler: crate::mainutils::essentials::do_slotNames,
    },
    EvaluatedBuiltin {
        name: "slot",
        handler: crate::mainutils::essentials::do_slot,
    },
    EvaluatedBuiltin {
        name: "extends",
        handler: crate::mainutils::essentials::do_extends,
    },
    EvaluatedBuiltin {
        name: "isSealedClass",
        handler: crate::mainutils::essentials::do_isSealedClass,
    },
    EvaluatedBuiltin {
        name: "sealClass",
        handler: crate::mainutils::essentials::do_sealClass,
    },
    EvaluatedBuiltin {
        name: "setReplaceMethod",
        handler: crate::mainutils::essentials::do_setReplaceMethod,
    },
    EvaluatedBuiltin {
        name: "getMethod",
        handler: crate::mainutils::essentials::do_getMethod,
    },
    EvaluatedBuiltin {
        name: "isGeneric",
        handler: crate::mainutils::essentials::do_isGeneric,
    },
    EvaluatedBuiltin {
        name: "findMethod",
        handler: crate::mainutils::essentials::do_findMethod,
    },
    EvaluatedBuiltin {
        name: "findMethods",
        handler: crate::mainutils::essentials::do_findMethods,
    },
    EvaluatedBuiltin {
        name: "showMethods",
        handler: crate::mainutils::essentials::do_showMethods,
    },
    EvaluatedBuiltin {
        name: "getGenerics",
        handler: crate::mainutils::essentials::do_getGenerics,
    },
    EvaluatedBuiltin {
        name: "getMethods",
        handler: crate::mainutils::essentials::do_getMethods,
    },
    EvaluatedBuiltin {
        name: "existsMethod",
        handler: crate::mainutils::essentials::do_existsMethod,
    },
    EvaluatedBuiltin {
        name: "hasMethod",
        handler: crate::mainutils::essentials::do_hasMethod,
    },
    EvaluatedBuiltin {
        name: "selectMethod",
        handler: crate::mainutils::essentials::do_selectMethod,
    },
    EvaluatedBuiltin {
        name: "findInterval",
        handler: crate::mainutils::essentials::do_findInterval,
    },
    EvaluatedBuiltin {
        name: "cut",
        handler: crate::mainutils::essentials::do_cut,
    },
    EvaluatedBuiltin {
        name: "setOldClass",
        handler: crate::mainutils::essentials::do_setOldClass,
    },
    EvaluatedBuiltin {
        name: "lower.tri",
        handler: crate::mainutils::essentials::do_lower_tri,
    },
    EvaluatedBuiltin {
        name: "upper.tri",
        handler: crate::mainutils::essentials::do_upper_tri,
    },
    EvaluatedBuiltin {
        name: "col",
        handler: crate::mainutils::essentials::do_col,
    },
    EvaluatedBuiltin {
        name: "mahalanobis",
        handler: crate::mainutils::essentials::do_mahalanobis,
    },
    EvaluatedBuiltin {
        name: "commandArgs",
        handler: crate::mainutils::essentials::do_commandArgs,
    },
    EvaluatedBuiltin {
        name: "getOption",
        handler: crate::mainutils::options::do_getOption,
    },
    EvaluatedBuiltin {
        name: "options",
        handler: crate::mainutils::options::do_options,
    },
    EvaluatedBuiltin {
        name: "interactive",
        handler: crate::mainutils::essentials::do_interactive,
    },
    EvaluatedBuiltin {
        name: "getRversion",
        handler: crate::mainutils::essentials::do_getRversion,
    },
    EvaluatedBuiltin {
        name: "getNamespaceVersion",
        handler: crate::mainutils::essentials::do_getNamespaceVersion,
    },
    EvaluatedBuiltin {
        name: "R.Version",
        handler: crate::mainutils::essentials::do_R_Version,
    },
    EvaluatedBuiltin {
        name: "complete.cases",
        handler: crate::mainutils::essentials::do_complete_cases,
    },
    EvaluatedBuiltin {
        name: "na.omit",
        handler: crate::mainutils::essentials::do_na_omit,
    },
    EvaluatedBuiltin {
        name: "na.exclude",
        handler: crate::mainutils::essentials::do_na_exclude,
    },
    EvaluatedBuiltin {
        name: "strwrap",
        handler: crate::mainutils::essentials::do_strwrap,
    },
    EvaluatedBuiltin {
        name: "environmentName",
        handler: crate::mainutils::essentials::do_environment_name,
    },
    EvaluatedBuiltin {
        name: "as",
        handler: crate::mainutils::essentials::do_as,
    },
    EvaluatedBuiltin {
        name: "withVisible",
        handler: crate::mainutils::essentials::do_with_visible,
    },
    EvaluatedBuiltin {
        name: "invisible",
        handler: crate::mainutils::essentials::do_invisible,
    },
    EvaluatedBuiltin {
        name: "isNamespace",
        handler: crate::mainutils::essentials::do_isNamespace,
    },
    EvaluatedBuiltin {
        name: "suppressMessages",
        handler: crate::mainutils::essentials::do_suppress_messages,
    },
    EvaluatedBuiltin {
        name: "force",
        handler: crate::mainutils::essentials::do_force,
    },
    EvaluatedBuiltin {
        name: "isTRUE",
        handler: crate::mainutils::essentials::do_is_true,
    },
    EvaluatedBuiltin {
        name: "isFALSE",
        handler: crate::mainutils::essentials::do_is_false,
    },
    EvaluatedBuiltin {
        name: "modifyList",
        handler: crate::mainutils::essentials::do_modify_list,
    },
    EvaluatedBuiltin {
        name: "with",
        handler: crate::mainutils::essentials::do_with,
    },
    EvaluatedBuiltin {
        name: "within",
        handler: crate::mainutils::essentials::do_within,
    },
    EvaluatedBuiltin {
        name: "transform",
        handler: crate::mainutils::essentials::do_transform,
    },
    EvaluatedBuiltin {
        name: "factor",
        handler: crate::mainutils::essentials::do_factor,
    },
    EvaluatedBuiltin {
        name: "addNA",
        handler: crate::mainutils::essentials::do_addNA,
    },
    EvaluatedBuiltin {
        name: "levels",
        handler: crate::mainutils::essentials::do_levels,
    },
    EvaluatedBuiltin {
        name: "levels<-",
        handler: crate::mainutils::essentials::do_levels_set,
    },
    EvaluatedBuiltin {
        name: "nlevels",
        handler: crate::mainutils::essentials::do_nlevels,
    },
    EvaluatedBuiltin {
        name: "R.home",
        handler: crate::mainutils::essentials::do_R_home,
    },
    EvaluatedBuiltin {
        name: "Sys.getenv",
        handler: crate::mainutils::essentials::do_Sys_getenv,
    },
    EvaluatedBuiltin {
        name: "Sys.setenv",
        handler: crate::mainutils::essentials::do_Sys_setenv,
    },
    EvaluatedBuiltin {
        name: "Sys.unsetenv",
        handler: crate::mainutils::essentials::do_Sys_unsetenv,
    },
    EvaluatedBuiltin {
        name: "Sys.info",
        handler: crate::mainutils::essentials::do_Sys_info,
    },
    EvaluatedBuiltin {
        name: "Sys.sleep",
        handler: crate::mainutils::essentials::do_Sys_sleep,
    },
    EvaluatedBuiltin {
        name: "OlsonNames",
        handler: crate::mainutils::essentials::do_OlsonNames,
    },
    EvaluatedBuiltin {
        name: "Sys.localeconv",
        handler: crate::mainutils::essentials::do_Sys_localeconv,
    },
    EvaluatedBuiltin {
        name: "Sys.getlocale",
        handler: crate::mainutils::essentials::do_Sys_getlocale,
    },
    EvaluatedBuiltin {
        name: "Sys.setlocale",
        handler: crate::mainutils::essentials::do_Sys_setlocale,
    },
    EvaluatedBuiltin {
        name: "Sys.chmod",
        handler: crate::mainutils::platform::do_syschmod,
    },
    EvaluatedBuiltin {
        name: "Sys.umask",
        handler: crate::mainutils::platform::do_sysumask,
    },
    EvaluatedBuiltin {
        name: "l10n_info",
        handler: crate::mainutils::platform::do_l10n_info,
    },
    EvaluatedBuiltin {
        name: "extSoftVersion",
        handler: crate::mainutils::platform::do_eSoftVersion,
    },
    EvaluatedBuiltin {
        name: "La_version",
        handler: crate::mainutils::lapack::do_La_version,
    },
    EvaluatedBuiltin {
        name: "Sys.getpid",
        handler: crate::mainutils::platform::do_sysgetpid,
    },
    EvaluatedBuiltin {
        name: "capabilities",
        handler: crate::mainutils::platform::do_capabilities,
    },
    EvaluatedBuiltin {
        name: "sys.nframe",
        handler: crate::eval::context::do_sys,
    },
    EvaluatedBuiltin {
        name: "sys.function",
        handler: crate::eval::context::do_sys,
    },
    EvaluatedBuiltin {
        name: "isSymmetric",
        handler: crate::mainutils::essentials::do_is_symmetric,
    },
    EvaluatedBuiltin {
        name: "Rprof",
        handler: crate::mainutils::essentials::do_Rprof,
    },
    EvaluatedBuiltin {
        name: "Rprofmem",
        handler: crate::mainutils::essentials::do_Rprofmem,
    },
    EvaluatedBuiltin {
        name: "gc",
        handler: crate::mainutils::essentials::do_gc,
    },
    EvaluatedBuiltin {
        name: "gc.time",
        handler: crate::mainutils::essentials::do_gc_time,
    },
    EvaluatedBuiltin {
        name: "gcinfo",
        handler: crate::mainutils::essentials::do_gcinfo,
    },
    EvaluatedBuiltin {
        name: "gctorture",
        handler: crate::mainutils::essentials::do_gctorture,
    },
    EvaluatedBuiltin {
        name: "gctorture2",
        handler: crate::mainutils::essentials::do_gctorture2,
    },
    EvaluatedBuiltin {
        name: "memory.size",
        handler: crate::mainutils::essentials::do_memory_size,
    },
    EvaluatedBuiltin {
        name: "object.size",
        handler: crate::mainutils::essentials::do_object_size,
    },
    EvaluatedBuiltin {
        name: "getS3method",
        handler: crate::mainutils::essentials::do_getS3method,
    },
    EvaluatedBuiltin {
        name: "setGeneric",
        handler: crate::mainutils::essentials::do_setGeneric,
    },
    EvaluatedBuiltin {
        name: "setMethod",
        handler: crate::mainutils::essentials::do_setMethod,
    },
    EvaluatedBuiltin {
        name: "computeRestarts",
        handler: crate::mainutils::essentials::do_computeRestarts,
    },
    EvaluatedBuiltin {
        name: "findRestart",
        handler: crate::mainutils::essentials::do_findRestart,
    },
    EvaluatedBuiltin {
        name: "invokeRestart",
        handler: crate::mainutils::essentials::do_invokeRestart,
    },
    EvaluatedBuiltin {
        name: "tryInvokeRestart",
        handler: crate::mainutils::essentials::do_tryInvokeRestart,
    },
    EvaluatedBuiltin {
        name: "isRestart",
        handler: crate::mainutils::essentials::do_isRestart,
    },
    EvaluatedBuiltin {
        name: ".libPaths",
        handler: crate::mainutils::essentials::do_lib_paths,
    },
    EvaluatedBuiltin {
        name: "installed.packages",
        handler: crate::mainutils::essentials::do_installed_packages,
    },
    EvaluatedBuiltin {
        name: "find.package",
        handler: crate::mainutils::essentials::do_find_package,
    },
    EvaluatedBuiltin {
        name: "attach",
        handler: crate::mainutils::envir::do_attach,
    },
    EvaluatedBuiltin {
        name: "find",
        handler: crate::mainutils::essentials::do_find,
    },
    EvaluatedBuiltin {
        name: "cbind",
        handler: crate::mainutils::essentials::do_cbind,
    },
    EvaluatedBuiltin {
        name: "t",
        handler: crate::mainutils::essentials::do_transpose,
    },
    EvaluatedBuiltin {
        name: "asin",
        handler: crate::mainutils::essentials::do_asin,
    },
    EvaluatedBuiltin {
        name: "acos",
        handler: crate::mainutils::essentials::do_acos,
    },
    EvaluatedBuiltin {
        name: "acosh",
        handler: crate::mainutils::essentials::do_acosh,
    },
    EvaluatedBuiltin {
        name: "asinh",
        handler: crate::mainutils::essentials::do_asinh,
    },
    EvaluatedBuiltin {
        name: "beta",
        handler: crate::mainutils::essentials::do_beta,
    },
    EvaluatedBuiltin {
        name: "lbeta",
        handler: crate::mainutils::essentials::do_lbeta,
    },
    EvaluatedBuiltin {
        name: "choose",
        handler: crate::mainutils::essentials::do_choose,
    },
    EvaluatedBuiltin {
        name: "lchoose",
        handler: crate::mainutils::essentials::do_lchoose,
    },
    EvaluatedBuiltin {
        name: "factorial",
        handler: crate::mainutils::essentials::do_factorial,
    },
    EvaluatedBuiltin {
        name: "lfactorial",
        handler: crate::mainutils::essentials::do_lfactorial,
    },
    EvaluatedBuiltin {
        name: "besselI",
        handler: crate::mainutils::essentials::do_besselI,
    },
    EvaluatedBuiltin {
        name: "besselJ",
        handler: crate::mainutils::essentials::do_besselJ,
    },
    EvaluatedBuiltin {
        name: "besselK",
        handler: crate::mainutils::essentials::do_besselK,
    },
    EvaluatedBuiltin {
        name: "besselY",
        handler: crate::mainutils::essentials::do_besselY,
    },
    EvaluatedBuiltin {
        name: "simplify2array",
        handler: crate::mainutils::essentials::do_simplify2array,
    },
    EvaluatedBuiltin {
        name: "char.expand",
        handler: crate::mainutils::essentials::do_char_expand,
    },
    EvaluatedBuiltin {
        name: "matrix",
        handler: crate::mainutils::essentials::do_matrix,
    },
    EvaluatedBuiltin {
        name: "matrix_impl",
        handler: crate::mainutils::essentials::do_matrix,
    },
    EvaluatedBuiltin {
        name: "array",
        handler: crate::mainutils::essentials::do_array,
    },
    EvaluatedBuiltin {
        name: "tsp",
        handler: crate::mainutils::essentials::do_tsp,
    },
    EvaluatedBuiltin {
        name: "tsp<-",
        handler: crate::mainutils::essentials::do_tsp_set,
    },
    EvaluatedBuiltin {
        name: "ts",
        handler: crate::mainutils::essentials::do_ts,
    },
    EvaluatedBuiltin {
        name: "ts.union",
        handler: crate::mainutils::essentials::do_ts_union,
    },
    EvaluatedBuiltin {
        name: "ts.intersect",
        handler: crate::mainutils::essentials::do_ts_intersect,
    },
    EvaluatedBuiltin {
        name: "nrow",
        handler: crate::mainutils::essentials::do_nrow,
    },
    EvaluatedBuiltin {
        name: "ncol",
        handler: crate::mainutils::essentials::do_ncol,
    },
    EvaluatedBuiltin {
        name: "names<-",
        handler: crate::mainutils::essentials::do_names_set,
    },
    EvaluatedBuiltin {
        name: "dim<-",
        handler: crate::mainutils::essentials::do_dim_set,
    },
    EvaluatedBuiltin {
        name: "dimnames<-",
        handler: crate::mainutils::essentials::do_dimnames_set,
    },
    EvaluatedBuiltin {
        name: "row.names<-",
        handler: crate::mainutils::essentials::do_rownames_set,
    },
    EvaluatedBuiltin {
        name: "exists",
        handler: crate::mainutils::essentials::do_exists,
    },
    EvaluatedBuiltin {
        name: "get",
        handler: crate::mainutils::essentials::do_get,
    },
    EvaluatedBuiltin {
        name: "assign",
        handler: crate::mainutils::essentials::do_assign,
    },
    EvaluatedBuiltin {
        name: "ls",
        handler: crate::mainutils::essentials::do_ls,
    },
    EvaluatedBuiltin {
        name: "objects",
        handler: crate::mainutils::essentials::do_ls,
    },
    EvaluatedBuiltin {
        name: "inherits",
        handler: crate::mainutils::objects::do_inherits,
    },
    EvaluatedBuiltin {
        name: "nameOfClass",
        handler: crate::mainutils::objects::do_nameOfClass,
    },
    EvaluatedBuiltin {
        name: "nameOfClass.default",
        handler: crate::mainutils::objects::do_nameOfClass_default,
    },
    EvaluatedBuiltin {
        name: "stop",
        handler: crate::mainutils::essentials::do_stop,
    },
    EvaluatedBuiltin {
        name: "message",
        handler: crate::mainutils::essentials::do_message,
    },
    EvaluatedBuiltin {
        name: "system",
        handler: crate::mainutils::essentials::do_system,
    },
    EvaluatedBuiltin {
        name: "system2",
        handler: crate::mainutils::essentials::do_system2,
    },
    EvaluatedBuiltin {
        name: "tempdir",
        handler: crate::mainutils::essentials::do_tempdir,
    },
    EvaluatedBuiltin {
        name: "list.dirs",
        handler: crate::mainutils::platform::do_listdirs,
    },
    EvaluatedBuiltin {
        name: "normalizePath",
        handler: crate::mainutils::essentials::do_normalizePath,
    },
    EvaluatedBuiltin {
        name: "gregexpr",
        handler: crate::mainutils::essentials::do_gregexpr,
    },
    EvaluatedBuiltin {
        name: "gregexec",
        handler: crate::mainutils::essentials::do_regexec,
    },
    EvaluatedBuiltin {
        name: "class<-",
        handler: crate::mainutils::essentials::do_class_set,
    },
    EvaluatedBuiltin {
        name: "comment<-",
        handler: crate::mainutils::essentials::do_comment_set,
    },
    EvaluatedBuiltin {
        name: "oldClass<-",
        handler: crate::mainutils::essentials::do_oldClass_set,
    },
    EvaluatedBuiltin {
        name: "attr<-",
        handler: crate::mainutils::essentials::do_attr_set,
    },
    EvaluatedBuiltin {
        name: "attributes<-",
        handler: crate::mainutils::essentials::do_attributes_set,
    },
    EvaluatedBuiltin {
        name: "qf",
        handler: crate::mainutils::essentials::do_qf,
    },
    EvaluatedBuiltin {
        name: "dunif",
        handler: crate::mainutils::essentials::do_dunif,
    },
    EvaluatedBuiltin {
        name: "punif",
        handler: crate::mainutils::essentials::do_punif,
    },
    EvaluatedBuiltin {
        name: "qunif",
        handler: crate::mainutils::essentials::do_qunif,
    },
    EvaluatedBuiltin {
        name: "dnbinom",
        handler: crate::mainutils::essentials::do_dnbinom,
    },
    EvaluatedBuiltin {
        name: "pnbinom",
        handler: crate::mainutils::essentials::do_pnbinom,
    },
    EvaluatedBuiltin {
        name: "qnbinom",
        handler: crate::mainutils::essentials::do_qnbinom,
    },
    EvaluatedBuiltin {
        name: "dgeom",
        handler: crate::mainutils::essentials::do_dgeom,
    },
    EvaluatedBuiltin {
        name: "pgeom",
        handler: crate::mainutils::essentials::do_pgeom,
    },
    EvaluatedBuiltin {
        name: "qgeom",
        handler: crate::mainutils::essentials::do_qgeom,
    },
];
