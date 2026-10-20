//! Statistical and data-manipulation builtin entries.

use super::EvaluatedBuiltin;

pub(super) const BASE_STATS: &[EvaluatedBuiltin] = &[
    EvaluatedBuiltin {
        name: "returnValue",
        handler: crate::mainutils::builtin::do_returnValue,
    },
    EvaluatedBuiltin {
        name: "qr.qy",
        handler: crate::mainutils::qr_apply::do_qr_qy,
    },
    EvaluatedBuiltin {
        name: "qr.qty",
        handler: crate::mainutils::qr_apply::do_qr_qty,
    },
    EvaluatedBuiltin {
        name: "qr.Q",
        handler: crate::mainutils::qr_q::do_qr_Q,
    },
    EvaluatedBuiltin {
        name: "qr.R",
        handler: crate::mainutils::qr_extract::do_qr_R,
    },
    EvaluatedBuiltin {
        name: "qr.coef",
        handler: crate::mainutils::qr_coef::do_qr_coef,
    },
    EvaluatedBuiltin {
        name: "qr.fitted",
        handler: crate::mainutils::qr_fitted::do_qr_fitted,
    },
    EvaluatedBuiltin {
        name: "qr.resid",
        handler: crate::mainutils::qr_fitted::do_qr_resid,
    },
    EvaluatedBuiltin {
        name: "qr.X",
        handler: crate::mainutils::qr_x::do_qr_X,
    },
    EvaluatedBuiltin {
        name: "qr",
        handler: crate::mainutils::qr::do_qr,
    },
    EvaluatedBuiltin {
        name: "qr.influence",
        handler: crate::mainutils::essentials::do_qr_influence,
    },
    EvaluatedBuiltin {
        name: "qr.default",
        handler: crate::mainutils::qr::do_qr_default,
    },
    EvaluatedBuiltin {
        name: "disassemble",
        handler: crate::eval::compiler_disassemble::do_disassemble,
    },
    EvaluatedBuiltin {
        name: "putconst",
        handler: crate::eval::compiler::do_putconst,
    },
    #[cfg(feature = "renderplot-device")]
    EvaluatedBuiltin {
        name: ".rport_grid",
        handler: crate::mainutils::portable_grid::dispatch,
    },
    EvaluatedBuiltin {
        name: "retracemem",
        handler: crate::mainutils::debug::do_retracemem,
    },
    EvaluatedBuiltin {
        name: "%*%",
        handler: crate::mainutils::array::do_matprod,
    },
    EvaluatedBuiltin {
        name: "asplit",
        handler: crate::mainutils::array::do_asplit,
    },
    EvaluatedBuiltin {
        name: "sum",
        handler: crate::eval::arithmetic::do_summary,
    },
    EvaluatedBuiltin {
        name: "min",
        handler: crate::eval::arithmetic::do_summary,
    },
    EvaluatedBuiltin {
        name: "max",
        handler: crate::eval::arithmetic::do_summary,
    },
    EvaluatedBuiltin {
        name: "prod",
        handler: crate::eval::arithmetic::do_summary,
    },
    EvaluatedBuiltin {
        name: "range",
        handler: crate::eval::arithmetic::do_summary,
    },
    EvaluatedBuiltin {
        name: "double",
        handler: crate::mainutils::essentials::do_numeric_constructor,
    },
    EvaluatedBuiltin {
        name: "raw",
        handler: crate::mainutils::essentials::do_raw_constructor,
    },
    EvaluatedBuiltin {
        name: "seq",
        handler: crate::mainutils::seq::do_seq,
    },
    EvaluatedBuiltin {
        name: "seq.int",
        handler: crate::mainutils::seq::do_seq,
    },
    EvaluatedBuiltin {
        name: "sequence",
        handler: crate::mainutils::seq::do_sequence,
    },
    EvaluatedBuiltin {
        name: "rep.int",
        handler: crate::mainutils::seq::do_rep_int,
    },
    EvaluatedBuiltin {
        name: "which",
        handler: crate::mainutils::essentials::do_which,
    },
    EvaluatedBuiltin {
        name: "table",
        handler: crate::mainutils::essentials::do_table,
    },
    EvaluatedBuiltin {
        name: "tabulate",
        handler: crate::mainutils::essentials::do_tabulate,
    },
    EvaluatedBuiltin {
        name: "pairlist",
        handler: crate::mainutils::essentials::do_pairlist,
    },
    EvaluatedBuiltin {
        name: "adist",
        handler: crate::mainutils::essentials::do_adist,
    },
    EvaluatedBuiltin {
        name: "abbreviate",
        handler: crate::mainutils::essentials::do_abbreviate,
    },
    EvaluatedBuiltin {
        name: "rawToBits",
        handler: crate::mainutils::essentials::do_rawToBits,
    },
    EvaluatedBuiltin {
        name: "packBits",
        handler: crate::mainutils::essentials::do_packBits,
    },
    EvaluatedBuiltin {
        name: "rawShift",
        handler: crate::mainutils::essentials::do_rawShift,
    },
    EvaluatedBuiltin {
        name: "quit",
        handler: crate::mainutils::essentials::do_quit,
    },
    EvaluatedBuiltin {
        name: "runif",
        handler: crate::library::stats::random::do_runif_r,
    },
    EvaluatedBuiltin {
        name: "rnorm",
        handler: crate::library::stats::random::do_rnorm_r,
    },
    EvaluatedBuiltin {
        name: "fft",
        handler: crate::library::stats::fourier::do_fft,
    },
    EvaluatedBuiltin {
        name: "mvfft",
        handler: crate::library::stats::fourier::do_mvfft,
    },
    EvaluatedBuiltin {
        name: "rpois",
        handler: crate::library::stats::random::do_rpois_r,
    },
    EvaluatedBuiltin {
        name: "rexp",
        handler: crate::library::stats::random::do_rexp_r,
    },
    EvaluatedBuiltin {
        name: "rchisq",
        handler: crate::library::stats::random::do_rchisq_r,
    },
    EvaluatedBuiltin {
        name: "rgeom",
        handler: crate::library::stats::random::do_rgeom_r,
    },
    EvaluatedBuiltin {
        name: "rt",
        handler: crate::library::stats::random::do_rt_r,
    },
    EvaluatedBuiltin {
        name: "rsignrank",
        handler: crate::library::stats::random::do_rsignrank_r,
    },
    EvaluatedBuiltin {
        name: "rbeta",
        handler: crate::library::stats::random::do_rbeta_r,
    },
    EvaluatedBuiltin {
        name: "rbinom",
        handler: crate::library::stats::random::do_rbinom_r,
    },
    EvaluatedBuiltin {
        name: "rmultinom",
        handler: crate::library::stats::random::do_rmultinom_r,
    },
    EvaluatedBuiltin {
        name: "r2dtable",
        handler: crate::library::stats::random::do_r2dtable_r,
    },
    EvaluatedBuiltin {
        name: "rWishart",
        handler: crate::library::stats::rwishart::do_rWishart_r,
    },
    EvaluatedBuiltin {
        name: "rcauchy",
        handler: crate::library::stats::random::do_rcauchy_r,
    },
    EvaluatedBuiltin {
        name: "rf",
        handler: crate::library::stats::random::do_rf_r,
    },
    EvaluatedBuiltin {
        name: "rgamma",
        handler: crate::library::stats::random::do_rgamma_r,
    },
    EvaluatedBuiltin {
        name: "rlnorm",
        handler: crate::library::stats::random::do_rlnorm_r,
    },
    EvaluatedBuiltin {
        name: "rlogis",
        handler: crate::library::stats::random::do_rlogis_r,
    },
    EvaluatedBuiltin {
        name: "rnbinom",
        handler: crate::library::stats::random::do_rnbinom_r,
    },
    EvaluatedBuiltin {
        name: "rweibull",
        handler: crate::library::stats::random::do_rweibull_r,
    },
    EvaluatedBuiltin {
        name: "rwilcox",
        handler: crate::library::stats::random::do_rwilcox_r,
    },
    EvaluatedBuiltin {
        name: "rhyper",
        handler: crate::library::stats::random::do_rhyper_r,
    },
    EvaluatedBuiltin {
        name: "sample",
        handler: crate::mainutils::rng_dispatch::do_sample,
    },
    EvaluatedBuiltin {
        name: "apply",
        handler: crate::mainutils::essentials::do_apply,
    },
    EvaluatedBuiltin {
        name: "tapply",
        handler: crate::mainutils::essentials::do_tapply,
    },
    EvaluatedBuiltin {
        name: ".rport_loess_predict",
        handler: crate::mainutils::loess::do_predict_core,
    },
    EvaluatedBuiltin {
        name: "eff.aovlist",
        handler: crate::mainutils::essentials::do_eff_aovlist,
    },
    EvaluatedBuiltin {
        name: "stack",
        handler: crate::mainutils::essentials::do_stack,
    },
    EvaluatedBuiltin {
        name: "unstack",
        handler: crate::mainutils::essentials::do_unstack,
    },
    EvaluatedBuiltin {
        name: "rowsum",
        handler: crate::mainutils::essentials::do_rowsum,
    },
    EvaluatedBuiltin {
        name: "margin.table",
        handler: crate::mainutils::essentials::do_margin_table,
    },
    EvaluatedBuiltin {
        name: "mad",
        handler: crate::mainutils::essentials::do_mad,
    },
    EvaluatedBuiltin {
        name: "fivenum",
        handler: crate::mainutils::essentials::do_fivenum,
    },
    EvaluatedBuiltin {
        name: "p.adjust",
        handler: crate::mainutils::essentials::do_p_adjust,
    },
    EvaluatedBuiltin {
        name: "pairwise.table",
        handler: crate::mainutils::essentials::do_pairwise_table,
    },
    EvaluatedBuiltin {
        name: "chisq.test",
        handler: crate::mainutils::essentials::do_chisq_test,
    },
    EvaluatedBuiltin {
        name: "prop.test",
        handler: crate::mainutils::essentials::do_prop_test,
    },
    EvaluatedBuiltin {
        name: "cor.test",
        handler: crate::mainutils::essentials::do_cor_test,
    },
    EvaluatedBuiltin {
        name: "var.test",
        handler: crate::mainutils::essentials::do_var_test,
    },
    EvaluatedBuiltin {
        name: "kruskal.test",
        handler: crate::mainutils::essentials::do_kruskal_test,
    },
    EvaluatedBuiltin {
        name: "poisson.test",
        handler: crate::mainutils::essentials::do_poisson_test,
    },
    EvaluatedBuiltin {
        name: "prop.trend.test",
        handler: crate::mainutils::essentials::do_prop_trend_test,
    },
    EvaluatedBuiltin {
        name: "fisher.test",
        handler: crate::mainutils::essentials::do_fisher_test,
    },
    EvaluatedBuiltin {
        name: "quade.test",
        handler: crate::mainutils::essentials::do_quade_test,
    },
    EvaluatedBuiltin {
        name: "pairwise.t.test",
        handler: crate::mainutils::essentials::do_pairwise_t_test,
    },
    EvaluatedBuiltin {
        name: "pairwise.prop.test",
        handler: crate::mainutils::essentials::do_pairwise_prop_test,
    },
    EvaluatedBuiltin {
        name: "pairwise.wilcox.test",
        handler: crate::mainutils::essentials::do_pairwise_wilcox_test,
    },
    EvaluatedBuiltin {
        name: "reorder",
        handler: crate::mainutils::essentials::do_reorder,
    },
    EvaluatedBuiltin {
        name: "variable.names",
        handler: crate::mainutils::essentials::do_variable_names,
    },
    EvaluatedBuiltin {
        name: "confint.lm",
        handler: crate::mainutils::essentials::do_confint_lm,
    },
    EvaluatedBuiltin {
        name: "vcov",
        handler: crate::mainutils::essentials::do_vcov,
    },
    EvaluatedBuiltin {
        name: "vcov.lm",
        handler: crate::mainutils::essentials::do_vcov_lm,
    },
    EvaluatedBuiltin {
        name: "dummy.coef",
        handler: crate::mainutils::essentials::do_dummy_coef,
    },
    EvaluatedBuiltin {
        name: "dummy.coef.lm",
        handler: crate::mainutils::essentials::do_dummy_coef,
    },
    EvaluatedBuiltin {
        name: "dfbeta",
        handler: crate::mainutils::essentials::do_dfbeta,
    },
    EvaluatedBuiltin {
        name: "dfbetas",
        handler: crate::mainutils::essentials::do_dfbetas,
    },
    EvaluatedBuiltin {
        name: "glm.control",
        handler: crate::mainutils::essentials::do_glm_control,
    },
    EvaluatedBuiltin {
        name: "sortedXyData",
        handler: crate::mainutils::essentials::do_sorted_xy_data,
    },
    EvaluatedBuiltin {
        name: "lm.fit",
        handler: crate::mainutils::essentials::do_lm_fit,
    },
    EvaluatedBuiltin {
        name: "lm.wfit",
        handler: crate::mainutils::essentials::do_lm_wfit,
    },
    EvaluatedBuiltin {
        name: "glm.fit",
        handler: crate::mainutils::essentials::do_glm_fit,
    },
    EvaluatedBuiltin {
        name: "lm",
        handler: crate::mainutils::essentials::do_lm,
    },
    EvaluatedBuiltin {
        name: ".lm.fit",
        handler: crate::mainutils::essentials::do_lm_fit,
    },
    EvaluatedBuiltin {
        name: "smoothEnds",
        handler: crate::mainutils::essentials::do_smooth_ends,
    },
    EvaluatedBuiltin {
        name: "aov",
        handler: crate::mainutils::essentials::do_aov,
    },
    EvaluatedBuiltin {
        name: "TukeyHSD",
        handler: crate::mainutils::essentials::do_tukey_hsd,
    },
    EvaluatedBuiltin {
        name: "glm",
        handler: crate::mainutils::essentials::do_glm,
    },
    EvaluatedBuiltin {
        name: "predict.glm",
        handler: crate::mainutils::essentials::do_predict_glm,
    },
    EvaluatedBuiltin {
        name: "estVar",
        handler: crate::mainutils::essentials::do_est_var,
    },
    EvaluatedBuiltin {
        name: "se.contrast",
        handler: crate::mainutils::essentials::do_se_contrast,
    },
    EvaluatedBuiltin {
        name: "SSD",
        handler: crate::mainutils::essentials::do_ssd,
    },
    EvaluatedBuiltin {
        name: "proj",
        handler: crate::mainutils::essentials::do_proj,
    },
    EvaluatedBuiltin {
        name: "poisson",
        handler: crate::mainutils::essentials::do_poisson,
    },
    EvaluatedBuiltin {
        name: "quasi",
        handler: crate::mainutils::essentials::do_quasi,
    },
    EvaluatedBuiltin {
        name: "quasibinomial",
        handler: crate::mainutils::essentials::do_quasibinomial,
    },
    EvaluatedBuiltin {
        name: "quasipoisson",
        handler: crate::mainutils::essentials::do_quasipoisson,
    },
    EvaluatedBuiltin {
        name: "power",
        handler: crate::mainutils::essentials::do_power,
    },
    EvaluatedBuiltin {
        name: "covratio",
        handler: crate::mainutils::essentials::do_covratio,
    },
    EvaluatedBuiltin {
        name: "lm.influence",
        handler: crate::mainutils::essentials::do_lm_influence,
    },
    EvaluatedBuiltin {
        name: "model.offset",
        handler: crate::mainutils::essentials::do_model_offset,
    },
    EvaluatedBuiltin {
        name: "varimax",
        handler: crate::mainutils::essentials::do_varimax,
    },
    EvaluatedBuiltin {
        name: "promax",
        handler: crate::mainutils::essentials::do_promax,
    },
    EvaluatedBuiltin {
        name: "power.t.test",
        handler: crate::mainutils::essentials::do_power_t_test,
    },
    EvaluatedBuiltin {
        name: "power.prop.test",
        handler: crate::mainutils::essentials::do_power_prop_test,
    },
    EvaluatedBuiltin {
        name: "dendrapply",
        handler: crate::mainutils::essentials::do_dendrapply,
    },
    EvaluatedBuiltin {
        name: "power.anova.test",
        handler: crate::mainutils::essentials::do_power_anova_test,
    },
    EvaluatedBuiltin {
        name: ".rport_reorder_dendrogram",
        handler: crate::mainutils::essentials::do_reorder_dendrogram,
    },
    EvaluatedBuiltin {
        name: ".rport_reorder_default",
        handler: crate::mainutils::essentials::do_reorder,
    },
    EvaluatedBuiltin {
        name: "order.dendrogram",
        handler: crate::mainutils::essentials::do_order_dendrogram,
    },
    EvaluatedBuiltin {
        name: "cutree",
        handler: crate::library::stats::hclust_utils::do_cutree,
    },
    EvaluatedBuiltin {
        name: "ecdf",
        handler: crate::mainutils::essentials::do_ecdf,
    },
    EvaluatedBuiltin {
        name: "ppoints",
        handler: crate::mainutils::essentials::do_ppoints,
    },
    EvaluatedBuiltin {
        name: "pbirthday",
        handler: crate::mainutils::essentials::do_pbirthday,
    },
    EvaluatedBuiltin {
        name: "qbirthday",
        handler: crate::mainutils::essentials::do_qbirthday,
    },
    EvaluatedBuiltin {
        name: "qqnorm",
        handler: crate::mainutils::essentials::do_qqnorm,
    },
    EvaluatedBuiltin {
        name: "qqline",
        handler: crate::mainutils::essentials::do_qqline,
    },
    EvaluatedBuiltin {
        name: "density",
        handler: crate::mainutils::essentials::do_density,
    },
    EvaluatedBuiltin {
        name: "density.default",
        handler: crate::mainutils::essentials::do_density,
    },
    EvaluatedBuiltin {
        name: "smooth",
        handler: crate::library::stats::smooth::do_smooth,
    },
    EvaluatedBuiltin {
        name: "ppr",
        handler: crate::mainutils::essentials::do_ppr,
    },
    EvaluatedBuiltin {
        name: "loess.smooth",
        handler: crate::mainutils::loess::do_loess_smooth,
    },
    EvaluatedBuiltin {
        name: "lowess",
        handler: crate::library::stats::lowess::do_lowess,
    },
    EvaluatedBuiltin {
        name: "supsmu",
        handler: crate::library::stats::lowess::do_supsmu,
    },
    EvaluatedBuiltin {
        name: "cancor",
        handler: crate::mainutils::essentials::do_cancor,
    },
    EvaluatedBuiltin {
        name: "isoreg",
        handler: crate::library::stats::isoreg::do_isoreg,
    },
    EvaluatedBuiltin {
        name: "ksmooth",
        handler: crate::library::stats::ksmooth::do_ksmooth,
    },
    EvaluatedBuiltin {
        name: "line",
        handler: crate::library::stats::line::do_line,
    },
    EvaluatedBuiltin {
        name: "as.dist",
        handler: crate::mainutils::essentials::do_as_dist,
    },
    EvaluatedBuiltin {
        name: "dist",
        handler: crate::mainutils::essentials::do_dist,
    },
    EvaluatedBuiltin {
        name: "kmeans",
        handler: crate::library::stats::kmeans::do_kmeans,
    },
    EvaluatedBuiltin {
        name: "cmdscale",
        handler: crate::library::stats::dblcen::do_cmdscale,
    },
    EvaluatedBuiltin {
        name: "prcomp",
        handler: crate::mainutils::essentials::do_prcomp,
    },
    EvaluatedBuiltin {
        name: "princomp",
        handler: crate::mainutils::essentials::do_princomp,
    },
    EvaluatedBuiltin {
        name: "smooth.spline",
        handler: crate::library::stats::splines::do_smooth_spline,
    },
    EvaluatedBuiltin {
        name: "runmed",
        handler: crate::library::stats::srunmed::do_runmed,
    },
    EvaluatedBuiltin {
        name: "stl",
        handler: crate::library::stats::stl::do_stl,
    },
    EvaluatedBuiltin {
        name: "filter",
        handler: crate::library::stats::filter::do_filter,
    },
    EvaluatedBuiltin {
        name: "decompose",
        handler: crate::library::stats::filter::do_decompose,
    },
    EvaluatedBuiltin {
        name: "acf",
        handler: crate::library::stats::filter::do_acf,
    },
    EvaluatedBuiltin {
        name: "spec.ar",
        handler: crate::library::stats::filter::do_spec_ar,
    },
    EvaluatedBuiltin {
        name: "ARMAacf",
        handler: crate::library::stats::filter::do_ARMAacf,
    },
    EvaluatedBuiltin {
        name: "ARMAtoMA",
        handler: crate::library::stats::filter::do_ARMAtoMA,
    },
    EvaluatedBuiltin {
        name: "acf2AR",
        handler: crate::library::stats::filter::do_acf2AR,
    },
    EvaluatedBuiltin {
        name: "kernel",
        handler: crate::library::stats::filter::do_kernel,
    },
    EvaluatedBuiltin {
        name: ".getXlevels",
        handler: crate::library::stats::filter::do_get_xlevels,
    },
    EvaluatedBuiltin {
        name: ".MFclass",
        handler: crate::library::stats::filter::do_mfclass,
    },
    EvaluatedBuiltin {
        name: ".checkMFClasses",
        handler: crate::library::stats::filter::do_check_mf_classes,
    },
    EvaluatedBuiltin {
        name: ".vcov.aliased",
        handler: crate::library::stats::filter::do_vcov_aliased,
    },
    EvaluatedBuiltin {
        name: ".nknots.smspl",
        handler: crate::library::stats::filter::do_nknots_smspl,
    },
    EvaluatedBuiltin {
        name: "df.kernel",
        handler: crate::library::stats::filter::do_df_kernel,
    },
    EvaluatedBuiltin {
        name: "bandwidth.kernel",
        handler: crate::library::stats::filter::do_bandwidth_kernel,
    },
    EvaluatedBuiltin {
        name: "as.stepfun",
        handler: crate::library::stats::filter::do_as_stepfun,
    },
    EvaluatedBuiltin {
        name: "summary.stepfun",
        handler: crate::mainutils::essentials::do_summary_stepfun,
    },
    EvaluatedBuiltin {
        name: "splinefunH",
        handler: crate::mainutils::essentials::do_splinefun_h,
    },
    EvaluatedBuiltin {
        name: "kernapply",
        handler: crate::library::stats::filter::do_kernapply,
    },
    EvaluatedBuiltin {
        name: "is.tskernel",
        handler: crate::library::stats::filter::do_is_tskernel,
    },
    EvaluatedBuiltin {
        name: "cycle",
        handler: crate::library::stats::filter::do_cycle,
    },
    EvaluatedBuiltin {
        name: "time",
        handler: crate::library::stats::filter::do_time,
    },
    EvaluatedBuiltin {
        name: "as.ts",
        handler: crate::library::stats::filter::do_as_ts,
    },
    EvaluatedBuiltin {
        name: "is.ts",
        handler: crate::library::stats::filter::do_is_ts,
    },
    EvaluatedBuiltin {
        name: "is.mts",
        handler: crate::library::stats::filter::do_is_mts,
    },
    EvaluatedBuiltin {
        name: "is.leaf",
        handler: crate::library::stats::filter::do_is_leaf,
    },
    EvaluatedBuiltin {
        name: "is.stepfun",
        handler: crate::library::stats::filter::do_is_stepfun,
    },
    EvaluatedBuiltin {
        name: "na.contiguous",
        handler: crate::library::stats::filter::do_na_contiguous,
    },
    EvaluatedBuiltin {
        name: "na.pass",
        handler: crate::library::stats::filter::do_na_pass,
    },
    EvaluatedBuiltin {
        name: "napredict",
        handler: crate::library::stats::filter::do_napredict,
    },
    EvaluatedBuiltin {
        name: "naresid",
        handler: crate::library::stats::filter::do_napredict,
    },
    EvaluatedBuiltin {
        name: "na.fail",
        handler: crate::library::stats::filter::do_na_fail,
    },
    EvaluatedBuiltin {
        name: "labels",
        handler: crate::library::stats::filter::do_labels,
    },
    EvaluatedBuiltin {
        name: "na.action",
        handler: crate::library::stats::filter::do_na_action,
    },
    EvaluatedBuiltin {
        name: "model.weights",
        handler: crate::library::stats::filter::do_model_weights,
    },
    EvaluatedBuiltin {
        name: "model.matrix",
        handler: crate::library::stats::filter::do_model_matrix,
    },
    EvaluatedBuiltin {
        name: "model.matrix.default",
        handler: crate::library::stats::filter::do_model_matrix_default,
    },
    EvaluatedBuiltin {
        name: "model.matrix.lm",
        handler: crate::library::stats::filter::do_model_matrix_lm,
    },
    EvaluatedBuiltin {
        name: "reformulate",
        handler: crate::library::stats::filter::do_reformulate,
    },
    EvaluatedBuiltin {
        name: "is.empty.model",
        handler: crate::library::stats::filter::do_is_empty_model,
    },
    EvaluatedBuiltin {
        name: "alias",
        handler: crate::library::stats::filter::do_alias,
    },
    EvaluatedBuiltin {
        name: "model.frame",
        handler: crate::library::stats::filter::do_model_frame,
    },
    EvaluatedBuiltin {
        name: "model.frame.default",
        handler: crate::library::stats::filter::do_model_frame,
    },
    EvaluatedBuiltin {
        name: "expand.model.frame",
        handler: crate::library::stats::filter::do_expand_model_frame,
    },
    EvaluatedBuiltin {
        name: "model.response",
        handler: crate::library::stats::filter::do_model_response,
    },
    EvaluatedBuiltin {
        name: "coef",
        handler: crate::library::stats::filter::do_coef,
    },
    EvaluatedBuiltin {
        name: "coefficients",
        handler: crate::library::stats::filter::do_coef,
    },
    EvaluatedBuiltin {
        name: "fitted",
        handler: crate::library::stats::filter::do_fitted,
    },
    EvaluatedBuiltin {
        name: "fitted.values",
        handler: crate::library::stats::filter::do_fitted,
    },
    EvaluatedBuiltin {
        name: "resid",
        handler: crate::library::stats::filter::do_resid,
    },
    EvaluatedBuiltin {
        name: "residuals.lm",
        handler: crate::library::stats::filter::do_resid,
    },
    EvaluatedBuiltin {
        name: "residuals.glm",
        handler: crate::library::stats::filter::do_resid,
    },
    EvaluatedBuiltin {
        name: "residuals",
        handler: crate::library::stats::filter::do_resid,
    },
    EvaluatedBuiltin {
        name: "weighted.residuals",
        handler: crate::library::stats::filter::do_resid,
    },
    EvaluatedBuiltin {
        name: "df.residual",
        handler: crate::library::stats::filter::do_df_residual,
    },
    EvaluatedBuiltin {
        name: "makepredictcall",
        handler: crate::library::stats::filter::do_makepredictcall,
    },
    EvaluatedBuiltin {
        name: "nobs",
        handler: crate::library::stats::filter::do_nobs,
    },
    EvaluatedBuiltin {
        name: "weights",
        handler: crate::library::stats::filter::do_weights,
    },
    EvaluatedBuiltin {
        name: "formula",
        handler: crate::library::stats::filter::do_formula,
    },
    EvaluatedBuiltin {
        name: "DF2formula",
        handler: crate::library::stats::filter::do_df2formula,
    },
    EvaluatedBuiltin {
        name: "replications",
        handler: crate::library::stats::filter::do_replications,
    },
    EvaluatedBuiltin {
        name: "asOneSidedFormula",
        handler: crate::library::stats::filter::do_as_one_sided_formula,
    },
    EvaluatedBuiltin {
        name: "get_all_vars",
        handler: crate::library::stats::filter::do_get_all_vars,
    },
    EvaluatedBuiltin {
        name: "all.vars",
        handler: crate::library::stats::filter::do_all_vars,
    },
    EvaluatedBuiltin {
        name: "delete.response",
        handler: crate::library::stats::filter::do_delete_response,
    },
    EvaluatedBuiltin {
        name: "arima0",
        handler: crate::library::stats::filter::do_arima0,
    },
    EvaluatedBuiltin {
        name: "arima0.diag",
        handler: crate::library::stats::filter::do_arima0_diag,
    },
    EvaluatedBuiltin {
        name: "plclust",
        handler: crate::library::stats::filter::do_plclust,
    },
    EvaluatedBuiltin {
        name: "drop.terms",
        handler: crate::library::stats::filter::do_drop_terms,
    },
    EvaluatedBuiltin {
        name: "drop.scope",
        handler: crate::library::stats::filter::do_drop_scope,
    },
    EvaluatedBuiltin {
        name: "add.scope",
        handler: crate::library::stats::filter::do_add_scope,
    },
    EvaluatedBuiltin {
        name: "drop1",
        handler: crate::mainutils::essentials::do_drop1,
    },
    EvaluatedBuiltin {
        name: "all.names",
        handler: crate::library::stats::filter::do_all_names,
    },
    EvaluatedBuiltin {
        name: "as.formula",
        handler: crate::library::stats::filter::do_as_formula,
    },
    EvaluatedBuiltin {
        name: "tsSmooth",
        handler: crate::library::stats::filter::do_ts_smooth,
    },
    EvaluatedBuiltin {
        name: "terms.formula",
        handler: crate::library::stats::filter::do_terms,
    },
    EvaluatedBuiltin {
        name: "terms",
        handler: crate::library::stats::filter::do_terms,
    },
    EvaluatedBuiltin {
        name: "factor.scope",
        handler: crate::library::stats::filter::do_factor_scope,
    },
    EvaluatedBuiltin {
        name: "offset",
        handler: crate::library::stats::filter::do_offset,
    },
    EvaluatedBuiltin {
        name: "getCall",
        handler: crate::library::stats::filter::do_getCall,
    },
    EvaluatedBuiltin {
        name: "contr.treatment",
        handler: crate::library::stats::filter::do_contr_treatment,
    },
    EvaluatedBuiltin {
        name: "contr.sum",
        handler: crate::library::stats::filter::do_contr_sum,
    },
    EvaluatedBuiltin {
        name: "contr.poly",
        handler: crate::library::stats::filter::do_contr_poly,
    },
    EvaluatedBuiltin {
        name: "contr.helmert",
        handler: crate::library::stats::filter::do_contr_helmert,
    },
    EvaluatedBuiltin {
        name: "contr.SAS",
        handler: crate::library::stats::filter::do_contr_sas,
    },
    EvaluatedBuiltin {
        name: "contrasts",
        handler: crate::library::stats::filter::do_contrasts,
    },
    EvaluatedBuiltin {
        name: "contrasts<-",
        handler: crate::library::stats::filter::do_contrasts_set,
    },
    EvaluatedBuiltin {
        name: "psmirnov",
        handler: crate::mainutils::essentials::do_psmirnov,
    },
    EvaluatedBuiltin {
        name: "qsmirnov",
        handler: crate::mainutils::essentials::do_qsmirnov,
    },
    EvaluatedBuiltin {
        name: "rsmirnov",
        handler: crate::mainutils::essentials::do_rsmirnov,
    },
    EvaluatedBuiltin {
        name: "rfree1way",
        handler: crate::mainutils::essentials::do_rfree1way,
    },
    EvaluatedBuiltin {
        name: "power.free1way.test",
        handler: crate::mainutils::essentials::do_power_free1way_test,
    },
    EvaluatedBuiltin {
        name: "C",
        handler: crate::library::stats::filter::do_C,
    },
    EvaluatedBuiltin {
        name: "window",
        handler: crate::library::stats::filter::do_window,
    },
    EvaluatedBuiltin {
        name: "window<-",
        handler: crate::library::stats::filter::do_window_set,
    },
    EvaluatedBuiltin {
        name: "lag",
        handler: crate::library::stats::filter::do_lag,
    },
    EvaluatedBuiltin {
        name: "frequency",
        handler: crate::library::stats::filter::do_frequency,
    },
    EvaluatedBuiltin {
        name: "deltat",
        handler: crate::library::stats::filter::do_deltat,
    },
    EvaluatedBuiltin {
        name: "start",
        handler: crate::library::stats::filter::do_start,
    },
    EvaluatedBuiltin {
        name: "end",
        handler: crate::library::stats::filter::do_end,
    },
    EvaluatedBuiltin {
        name: "HoltWinters",
        handler: crate::library::stats::holtwinters::do_HoltWinters,
    },
    EvaluatedBuiltin {
        name: "pacf",
        handler: crate::library::stats::filter::do_pacf,
    },
    EvaluatedBuiltin {
        name: "shapiro.test",
        handler: crate::library::stats::swilk::do_shapiro_test,
    },
    EvaluatedBuiltin {
        name: "makeARIMA",
        handler: crate::library::stats::filter::do_makeARIMA,
    },
    EvaluatedBuiltin {
        name: "constrOptim",
        handler: crate::library::stats::optim::do_constr_optim,
    },
    EvaluatedBuiltin {
        name: "KalmanLike",
        handler: crate::library::stats::filter::do_kalman_like,
    },
    EvaluatedBuiltin {
        name: "KalmanRun",
        handler: crate::library::stats::filter::do_kalman_run,
    },
    EvaluatedBuiltin {
        name: "KalmanForecast",
        handler: crate::library::stats::filter::do_kalman_forecast,
    },
    EvaluatedBuiltin {
        name: "KalmanSmooth",
        handler: crate::library::stats::filter::do_kalman_smooth,
    },
    EvaluatedBuiltin {
        name: "StructTS",
        handler: crate::library::stats::filter::do_struct_ts,
    },
    EvaluatedBuiltin {
        name: "wilcox.test",
        handler: crate::mainutils::essentials::do_wilcox_test,
    },
    EvaluatedBuiltin {
        name: "arima.sim",
        handler: crate::library::stats::filter::do_arima_sim,
    },
    EvaluatedBuiltin {
        name: "arima",
        handler: crate::library::stats::filter::do_arima,
    },
    EvaluatedBuiltin {
        name: "spec.taper",
        handler: crate::library::stats::filter::do_spec_taper,
    },
    EvaluatedBuiltin {
        name: "ccf",
        handler: crate::library::stats::filter::do_ccf,
    },
    EvaluatedBuiltin {
        name: "ar",
        handler: crate::library::stats::filter::do_ar,
    },
    EvaluatedBuiltin {
        name: "ar.yw",
        handler: crate::library::stats::filter::do_ar,
    },
    EvaluatedBuiltin {
        name: "ar.burg",
        handler: crate::library::stats::filter::do_ar_burg,
    },
    EvaluatedBuiltin {
        name: "ar.ols",
        handler: crate::library::stats::filter::do_ar_ols,
    },
    EvaluatedBuiltin {
        name: "ar.mle",
        handler: crate::library::stats::filter::do_ar_mle,
    },
    EvaluatedBuiltin {
        name: "spec.pgram",
        handler: crate::library::stats::fourier::do_spec_pgram,
    },
    EvaluatedBuiltin {
        name: "spectrum",
        handler: crate::library::stats::fourier::do_spectrum,
    },
    EvaluatedBuiltin {
        name: "uniroot",
        handler: crate::library::stats::zeroin::do_uniroot,
    },
    EvaluatedBuiltin {
        name: "optim",
        handler: crate::library::stats::optim::do_optim,
    },
    EvaluatedBuiltin {
        name: "optimHess",
        handler: crate::library::stats::optim::do_optim_hess,
    },
    EvaluatedBuiltin {
        name: "integrate",
        handler: crate::library::stats::integrate::do_integrate,
    },
    EvaluatedBuiltin {
        name: "polyroot",
        handler: crate::mainutils::polyroot::do_polyroot,
    },
    EvaluatedBuiltin {
        name: "optimize",
        handler: crate::library::stats::zeroin::do_optimize,
    },
    EvaluatedBuiltin {
        name: "optimise",
        handler: crate::library::stats::zeroin::do_optimize,
    },
    EvaluatedBuiltin {
        name: "nlm",
        handler: crate::library::stats::zeroin::do_nlm,
    },
    EvaluatedBuiltin {
        name: "nlminb",
        handler: crate::library::stats::zeroin::do_nlminb,
    },
    EvaluatedBuiltin {
        name: "approx",
        handler: crate::library::stats::approx::do_approx,
    },
    EvaluatedBuiltin {
        name: "approxfun",
        handler: crate::library::stats::approx::do_approxfun,
    },
    EvaluatedBuiltin {
        name: ".approx_apply",
        handler: crate::library::stats::approx::do_approx_apply,
    },
    EvaluatedBuiltin {
        name: "spline",
        handler: crate::library::stats::splines::do_spline,
    },
    EvaluatedBuiltin {
        name: "splinefun",
        handler: crate::library::stats::splines::do_splinefun,
    },
    EvaluatedBuiltin {
        name: ".spline_apply",
        handler: crate::library::stats::splines::do_spline_apply,
    },
    EvaluatedBuiltin {
        name: "nextn",
        handler: crate::library::stats::fourier::do_nextn,
    },
    EvaluatedBuiltin {
        name: "diffinv",
        handler: crate::mainutils::essentials::do_diffinv,
    },
    EvaluatedBuiltin {
        name: "convolve",
        handler: crate::library::stats::fourier::do_convolve,
    },
    EvaluatedBuiltin {
        name: "deriv",
        handler: crate::mainutils::essentials::do_deriv,
    },
    EvaluatedBuiltin {
        name: "deriv3",
        handler: crate::mainutils::essentials::do_deriv3,
    },
    EvaluatedBuiltin {
        name: "sweep",
        handler: crate::mainutils::essentials::do_sweep,
    },
    EvaluatedBuiltin {
        name: "sorted_fpass",
        handler: crate::mainutils::radixsort::do_sorted_fpass,
    },
    EvaluatedBuiltin {
        name: "wrap_meta",
        handler: crate::mainutils::radixsort::do_wrap_meta,
    },
    EvaluatedBuiltin {
        name: "pmin",
        handler: crate::mainutils::essentials::do_pmin,
    },
    EvaluatedBuiltin {
        name: "pmax",
        handler: crate::mainutils::essentials::do_pmax,
    },
    EvaluatedBuiltin {
        name: "pmax.int",
        handler: crate::mainutils::essentials::do_pmax,
    },
    EvaluatedBuiltin {
        name: "pmin.int",
        handler: crate::mainutils::essentials::do_pmin,
    },
    EvaluatedBuiltin {
        name: "which.min",
        handler: crate::mainutils::essentials::do_which_min,
    },
    EvaluatedBuiltin {
        name: "which.max",
        handler: crate::mainutils::essentials::do_which_max,
    },
    EvaluatedBuiltin {
        name: "setdiff",
        handler: crate::mainutils::essentials::do_setdiff,
    },
    EvaluatedBuiltin {
        name: "rownames",
        handler: crate::mainutils::essentials::do_rownames,
    },
    EvaluatedBuiltin {
        name: "row.names",
        handler: crate::mainutils::essentials::do_rownames,
    },
    EvaluatedBuiltin {
        name: "row.names.stored",
        handler: crate::mainutils::essentials::do_row_names_stored,
    },
    EvaluatedBuiltin {
        name: "colnames",
        handler: crate::mainutils::essentials::do_colnames,
    },
    EvaluatedBuiltin {
        name: "data.frame",
        handler: crate::mainutils::essentials::do_data_frame,
    },
    EvaluatedBuiltin {
        name: "dirname",
        handler: crate::mainutils::essentials::do_dirname,
    },
    EvaluatedBuiltin {
        name: "dir.exists",
        handler: crate::mainutils::platform::do_direxists,
    },
    EvaluatedBuiltin {
        name: "dir.create",
        handler: crate::mainutils::platform::do_dircreate,
    },
    EvaluatedBuiltin {
        name: "do.call",
        handler: crate::mainutils::essentials::do_do_call,
    },
    EvaluatedBuiltin {
        name: "summary.default",
        handler: crate::mainutils::essentials::do_summary_default,
    },
    EvaluatedBuiltin {
        name: "summary.lm",
        handler: crate::mainutils::essentials::do_summary_lm,
    },
    EvaluatedBuiltin {
        name: "summary.glm",
        handler: crate::mainutils::essentials::do_summary_glm,
    },
    EvaluatedBuiltin {
        name: "model.tables",
        handler: crate::mainutils::essentials::do_model_tables,
    },
    EvaluatedBuiltin {
        name: "summary.aov",
        handler: crate::mainutils::essentials::do_summary_aov,
    },
    EvaluatedBuiltin {
        name: "summary.manova",
        handler: crate::mainutils::essentials::do_summary_manova,
    },
    EvaluatedBuiltin {
        name: "anova",
        handler: crate::mainutils::essentials::do_anova_lm,
    },
    EvaluatedBuiltin {
        name: "anova.lm",
        handler: crate::mainutils::essentials::do_anova_lm,
    },
    EvaluatedBuiltin {
        name: "unlist",
        handler: crate::mainutils::essentials::do_unlist,
    },
    EvaluatedBuiltin {
        name: "summary.data.frame",
        handler: crate::mainutils::essentials::do_summary_data_frame,
    },
    EvaluatedBuiltin {
        name: "crossprod",
        handler: crate::mainutils::essentials::do_crossprod,
    },
    EvaluatedBuiltin {
        name: "tcrossprod",
        handler: crate::mainutils::essentials::do_tcrossprod,
    },
    EvaluatedBuiltin {
        name: "rcond",
        handler: crate::mainutils::essentials::do_rcond,
    },
    EvaluatedBuiltin {
        name: "max.col",
        handler: crate::mainutils::array::do_maxcol,
    },
    EvaluatedBuiltin {
        name: "det",
        handler: crate::mainutils::essentials::do_det,
    },
    EvaluatedBuiltin {
        name: "pmatch",
        handler: crate::mainutils::essentials::do_pmatch,
    },
    EvaluatedBuiltin {
        name: "round",
        handler: crate::mainutils::essentials::do_round,
    },
    EvaluatedBuiltin {
        name: "representation",
        handler: crate::mainutils::essentials::do_representation,
    },
    EvaluatedBuiltin {
        name: "possibleExtends",
        handler: crate::mainutils::essentials::do_possibleExtends,
    },
    EvaluatedBuiltin {
        name: "removeGeneric",
        handler: crate::mainutils::essentials::do_removeGeneric,
    },
    EvaluatedBuiltin {
        name: "removeMethod",
        handler: crate::mainutils::essentials::do_removeMethod,
    },
    EvaluatedBuiltin {
        name: "rank",
        handler: crate::mainutils::essentials::do_rank,
    },
    EvaluatedBuiltin {
        name: "duplicated",
        handler: crate::mainutils::essentials::do_duplicated,
    },
    EvaluatedBuiltin {
        name: "duplicated.array",
        handler: crate::mainutils::essentials::do_duplicated_array,
    },
    EvaluatedBuiltin {
        name: "colSums",
        handler: crate::mainutils::essentials::do_colSums,
    },
    EvaluatedBuiltin {
        name: "rowSums",
        handler: crate::mainutils::essentials::do_rowSums,
    },
    EvaluatedBuiltin {
        name: "colMeans",
        handler: crate::mainutils::essentials::do_colMeans,
    },
    EvaluatedBuiltin {
        name: "rowMeans",
        handler: crate::mainutils::essentials::do_rowMeans,
    },
    EvaluatedBuiltin {
        name: "row",
        handler: crate::mainutils::essentials::do_row,
    },
    EvaluatedBuiltin {
        name: "cov",
        handler: crate::mainutils::essentials::do_cov,
    },
    EvaluatedBuiltin {
        name: "cov.wt",
        handler: crate::mainutils::essentials::do_cov_wt,
    },
    EvaluatedBuiltin {
        name: "cov2cor",
        handler: crate::mainutils::essentials::do_cov2cor,
    },
    EvaluatedBuiltin {
        name: "cor",
        handler: crate::mainutils::essentials::do_cor,
    },
    EvaluatedBuiltin {
        name: "scale",
        handler: crate::mainutils::essentials::do_scale,
    },
    EvaluatedBuiltin {
        name: "rle",
        handler: crate::mainutils::essentials::do_rle,
    },
    EvaluatedBuiltin {
        name: "inverse.rle",
        handler: crate::mainutils::essentials::do_inverse_rle,
    },
    EvaluatedBuiltin {
        name: "reshape",
        handler: crate::mainutils::essentials::do_reshape,
    },
    EvaluatedBuiltin {
        name: "dget",
        handler: crate::mainutils::essentials::do_dget,
    },
    EvaluatedBuiltin {
        name: "packageStartupMessage",
        handler: crate::mainutils::essentials::do_package_startup_message,
    },
    EvaluatedBuiltin {
        name: "summary.numeric",
        handler: crate::mainutils::essentials::do_summary_numeric,
    },
    EvaluatedBuiltin {
        name: "summary.integer",
        handler: crate::mainutils::essentials::do_summary_integer,
    },
    EvaluatedBuiltin {
        name: "summary.logical",
        handler: crate::mainutils::essentials::do_summary_logical,
    },
    EvaluatedBuiltin {
        name: "is.unsorted",
        handler: crate::mainutils::essentials::do_is_unsorted,
    },
    EvaluatedBuiltin {
        name: "split",
        handler: crate::mainutils::essentials::do_split,
    },
    EvaluatedBuiltin {
        name: "unsplit",
        handler: crate::mainutils::essentials::do_unsplit,
    },
    EvaluatedBuiltin {
        name: "prop.table",
        handler: crate::mainutils::essentials::do_prop_table,
    },
    EvaluatedBuiltin {
        name: "addmargins",
        handler: crate::mainutils::essentials::do_addmargins,
    },
    EvaluatedBuiltin {
        name: "ftable",
        handler: crate::mainutils::essentials::do_ftable,
    },
    EvaluatedBuiltin {
        name: "xtabs",
        handler: crate::mainutils::essentials::do_xtabs,
    },
    EvaluatedBuiltin {
        name: "aggregate",
        handler: crate::mainutils::essentials::do_aggregate,
    },
    EvaluatedBuiltin {
        name: "aggregate.data.frame",
        handler: crate::mainutils::essentials::do_aggregate_df,
    },
    EvaluatedBuiltin {
        name: "aggregate.ts",
        handler: crate::library::stats::filter::do_aggregate_ts,
    },
    EvaluatedBuiltin {
        name: "ave",
        handler: crate::mainutils::essentials::do_ave,
    },
    EvaluatedBuiltin {
        name: "by",
        handler: crate::mainutils::essentials::do_by,
    },
    EvaluatedBuiltin {
        name: "interaction",
        handler: crate::mainutils::essentials::do_interaction,
    },
    EvaluatedBuiltin {
        name: "relevel",
        handler: crate::mainutils::essentials::do_relevel,
    },
    EvaluatedBuiltin {
        name: "droplevels",
        handler: crate::mainutils::essentials::do_droplevels,
    },
    EvaluatedBuiltin {
        name: "ordered",
        handler: crate::mainutils::essentials::do_ordered,
    },
    EvaluatedBuiltin {
        name: "as.ordered",
        handler: crate::mainutils::essentials::do_as_ordered,
    },
    EvaluatedBuiltin {
        name: "gl",
        handler: crate::mainutils::essentials::do_gl,
    },
    EvaluatedBuiltin {
        name: "is.ordered",
        handler: crate::mainutils::essentials::do_is_ordered,
    },
    EvaluatedBuiltin {
        name: "Sys.which",
        handler: crate::mainutils::essentials::do_Sys_which,
    },
    EvaluatedBuiltin {
        name: "path.expand",
        handler: crate::mainutils::platform::do_pathexpand,
    },
    EvaluatedBuiltin {
        name: "Cstack_info",
        handler: crate::mainutils::platform::do_Cstack_info,
    },
    EvaluatedBuiltin {
        name: "data.matrix",
        handler: crate::mainutils::essentials::do_data_matrix,
    },
    EvaluatedBuiltin {
        name: "registerS3method",
        handler: crate::mainutils::essentials::do_registerS3method,
    },
    EvaluatedBuiltin {
        name: "dyn.load",
        handler: crate::mainutils::rdynload::do_dynload,
    },
    EvaluatedBuiltin {
        name: "dyn.unload",
        handler: crate::mainutils::rdynload::do_dynunload,
    },
    EvaluatedBuiltin {
        name: "packageVersion",
        handler: crate::mainutils::essentials::do_package_version,
    },
    EvaluatedBuiltin {
        name: "packageDescription",
        handler: crate::mainutils::essentials::do_package_description,
    },
    EvaluatedBuiltin {
        name: "data",
        handler: crate::mainutils::essentials::do_data,
    },
    EvaluatedBuiltin {
        name: "detach",
        handler: crate::mainutils::envir::do_detach,
    },
    EvaluatedBuiltin {
        name: "demo",
        handler: crate::mainutils::essentials::do_demo,
    },
    EvaluatedBuiltin {
        name: "dlnorm",
        handler: crate::mainutils::essentials::do_dlnorm,
    },
    EvaluatedBuiltin {
        name: "plnorm",
        handler: crate::mainutils::essentials::do_plnorm,
    },
    EvaluatedBuiltin {
        name: "qlnorm",
        handler: crate::mainutils::essentials::do_qlnorm,
    },
    EvaluatedBuiltin {
        name: "dlogis",
        handler: crate::mainutils::essentials::do_dlogis,
    },
    EvaluatedBuiltin {
        name: "plogis",
        handler: crate::mainutils::essentials::do_plogis,
    },
    EvaluatedBuiltin {
        name: "qlogis",
        handler: crate::mainutils::essentials::do_qlogis,
    },
    EvaluatedBuiltin {
        name: "dsignrank",
        handler: crate::mainutils::essentials::do_dsignrank,
    },
    EvaluatedBuiltin {
        name: "psignrank",
        handler: crate::mainutils::essentials::do_psignrank,
    },
    EvaluatedBuiltin {
        name: "qsignrank",
        handler: crate::mainutils::essentials::do_qsignrank,
    },
    EvaluatedBuiltin {
        name: "dwilcox",
        handler: crate::mainutils::essentials::do_dwilcox,
    },
    EvaluatedBuiltin {
        name: "weighted.mean",
        handler: crate::mainutils::essentials::do_weighted_mean,
    },
    EvaluatedBuiltin {
        name: "pwilcox",
        handler: crate::mainutils::essentials::do_pwilcox,
    },
    EvaluatedBuiltin {
        name: "qwilcox",
        handler: crate::mainutils::essentials::do_qwilcox,
    },
    EvaluatedBuiltin {
        name: "dhyper",
        handler: crate::mainutils::essentials::do_dhyper,
    },
    EvaluatedBuiltin {
        name: "phyper",
        handler: crate::mainutils::essentials::do_phyper,
    },
    EvaluatedBuiltin {
        name: "qhyper",
        handler: crate::mainutils::essentials::do_qhyper,
    },
    EvaluatedBuiltin {
        name: "ptukey",
        handler: crate::mainutils::essentials::do_ptukey,
    },
    EvaluatedBuiltin {
        name: "qtukey",
        handler: crate::mainutils::essentials::do_qtukey,
    },
    EvaluatedBuiltin {
        name: "dmultinom",
        handler: crate::mainutils::essentials::do_dmultinom,
    },
    EvaluatedBuiltin {
        name: "rbind",
        handler: crate::mainutils::essentials::do_rbind,
    },
    EvaluatedBuiltin {
        name: "var",
        handler: crate::mainutils::essentials::do_var,
    },
    EvaluatedBuiltin {
        name: "sd",
        handler: crate::mainutils::essentials::do_sd,
    },
    EvaluatedBuiltin {
        name: "IQR",
        handler: crate::mainutils::essentials::do_iqr,
    },
    EvaluatedBuiltin {
        name: "quantile",
        handler: crate::mainutils::essentials::do_quantile,
    },
    EvaluatedBuiltin {
        name: "cummin",
        handler: crate::mainutils::essentials::do_cummin,
    },
    EvaluatedBuiltin {
        name: "cummax",
        handler: crate::mainutils::essentials::do_cummax,
    },
    EvaluatedBuiltin {
        name: "dimnames",
        handler: crate::mainutils::essentials::do_dimnames,
    },
    EvaluatedBuiltin {
        name: "digamma",
        handler: crate::mainutils::essentials::do_digamma,
    },
    EvaluatedBuiltin {
        name: "psigamma",
        handler: crate::mainutils::essentials::do_psigamma,
    },
    EvaluatedBuiltin {
        name: "pos.to.env",
        handler: crate::mainutils::essentials::do_pos_to_env,
    },
    EvaluatedBuiltin {
        name: "sort.list",
        handler: crate::mainutils::essentials::do_sort_list,
    },
    EvaluatedBuiltin {
        name: "cumsum",
        handler: crate::mainutils::essentials::do_cumsum,
    },
    EvaluatedBuiltin {
        name: "cumprod",
        handler: crate::mainutils::essentials::do_cumprod,
    },
    EvaluatedBuiltin {
        name: "cumvar",
        handler: crate::mainutils::essentials::do_cumvar,
    },
    EvaluatedBuiltin {
        name: "seq_len",
        handler: crate::mainutils::seq::do_seq_len,
    },
    EvaluatedBuiltin {
        name: "seq_along",
        handler: crate::mainutils::seq::do_seq_along,
    },
    EvaluatedBuiltin {
        name: "rev",
        handler: crate::mainutils::essentials::do_rev,
    },
    EvaluatedBuiltin {
        name: "drop",
        handler: crate::mainutils::essentials::do_drop,
    },
    EvaluatedBuiltin {
        name: "diag",
        handler: crate::mainutils::essentials::do_diag,
    },
    EvaluatedBuiltin {
        name: "dim",
        handler: crate::mainutils::essentials::do_dim,
    },
    EvaluatedBuiltin {
        name: "hasTsp",
        handler: crate::library::stats::filter::do_has_tsp,
    },
    EvaluatedBuiltin {
        name: "rownames<-",
        handler: crate::mainutils::essentials::do_rownames_set,
    },
    EvaluatedBuiltin {
        name: "colnames<-",
        handler: crate::mainutils::essentials::do_colnames_set,
    },
    EvaluatedBuiltin {
        name: "regexpr",
        handler: crate::mainutils::essentials::do_regexpr,
    },
    EvaluatedBuiltin {
        name: "regmatches",
        handler: crate::mainutils::essentials::do_regmatches,
    },
    EvaluatedBuiltin {
        name: "regexec",
        handler: crate::mainutils::essentials::do_regexec,
    },
    EvaluatedBuiltin {
        name: "sample.int",
        handler: crate::mainutils::essentials::do_sample_int,
    },
    EvaluatedBuiltin {
        name: "proc.time",
        handler: crate::mainutils::essentials::do_proc_time,
    },
    EvaluatedBuiltin {
        name: "dnorm",
        handler: crate::mainutils::essentials::do_dnorm,
    },
    EvaluatedBuiltin {
        name: "pnorm",
        handler: crate::mainutils::essentials::do_pnorm,
    },
    EvaluatedBuiltin {
        name: "qnorm",
        handler: crate::mainutils::essentials::do_qnorm,
    },
    EvaluatedBuiltin {
        name: "dpois",
        handler: crate::mainutils::essentials::do_dpois,
    },
    EvaluatedBuiltin {
        name: "ppois",
        handler: crate::mainutils::essentials::do_ppois,
    },
    EvaluatedBuiltin {
        name: "qpois",
        handler: crate::mainutils::essentials::do_qpois,
    },
    EvaluatedBuiltin {
        name: "dbinom",
        handler: crate::mainutils::essentials::do_dbinom,
    },
    EvaluatedBuiltin {
        name: "pbinom",
        handler: crate::mainutils::essentials::do_pbinom,
    },
    EvaluatedBuiltin {
        name: "qbinom",
        handler: crate::mainutils::essentials::do_qbinom,
    },
    EvaluatedBuiltin {
        name: "dexp",
        handler: crate::mainutils::essentials::do_dexp,
    },
    EvaluatedBuiltin {
        name: "pexp",
        handler: crate::mainutils::essentials::do_pexp,
    },
    EvaluatedBuiltin {
        name: "qexp",
        handler: crate::mainutils::essentials::do_qexp,
    },
    EvaluatedBuiltin {
        name: "dgamma",
        handler: crate::mainutils::essentials::do_dgamma,
    },
    EvaluatedBuiltin {
        name: "pgamma",
        handler: crate::mainutils::essentials::do_pgamma,
    },
    EvaluatedBuiltin {
        name: "qgamma",
        handler: crate::mainutils::essentials::do_qgamma,
    },
    EvaluatedBuiltin {
        name: "dbeta",
        handler: crate::mainutils::essentials::do_dbeta,
    },
    EvaluatedBuiltin {
        name: "pbeta",
        handler: crate::mainutils::essentials::do_pbeta,
    },
    EvaluatedBuiltin {
        name: "qbeta",
        handler: crate::mainutils::essentials::do_qbeta,
    },
    EvaluatedBuiltin {
        name: "dt",
        handler: crate::mainutils::essentials::do_dt,
    },
    EvaluatedBuiltin {
        name: "pt",
        handler: crate::mainutils::essentials::do_pt,
    },
    EvaluatedBuiltin {
        name: "qt",
        handler: crate::mainutils::essentials::do_qt,
    },
    EvaluatedBuiltin {
        name: "dchisq",
        handler: crate::mainutils::essentials::do_dchisq,
    },
    EvaluatedBuiltin {
        name: "pchisq",
        handler: crate::mainutils::essentials::do_pchisq,
    },
    EvaluatedBuiltin {
        name: "qchisq",
        handler: crate::mainutils::essentials::do_qchisq,
    },
    EvaluatedBuiltin {
        name: "dcauchy",
        handler: crate::mainutils::essentials::do_dcauchy,
    },
    EvaluatedBuiltin {
        name: "pcauchy",
        handler: crate::mainutils::essentials::do_pcauchy,
    },
    EvaluatedBuiltin {
        name: "qcauchy",
        handler: crate::mainutils::essentials::do_qcauchy,
    },
    EvaluatedBuiltin {
        name: "dweibull",
        handler: crate::mainutils::essentials::do_dweibull,
    },
    EvaluatedBuiltin {
        name: "pweibull",
        handler: crate::mainutils::essentials::do_pweibull,
    },
    EvaluatedBuiltin {
        name: "qweibull",
        handler: crate::mainutils::essentials::do_qweibull,
    },
    EvaluatedBuiltin {
        name: "df",
        handler: crate::mainutils::essentials::do_df,
    },
    EvaluatedBuiltin {
        name: "pf",
        handler: crate::mainutils::essentials::do_pf,
    },
];
