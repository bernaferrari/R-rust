//! Graphics and device builtin entries.

use super::EvaluatedBuiltin;

pub(super) const GRAPHICS_EXTERNAL: &[EvaluatedBuiltin] = &[
    #[cfg(feature = "renderplot-device")]
    EvaluatedBuiltin {
        name: ".rport_rasterImage",
        handler: crate::mainutils::portable_plot::raster_image,
    },
    #[cfg(feature = "renderplot-device")]
    EvaluatedBuiltin {
        name: ".rport_recordPlot",
        handler: crate::mainutils::graphics_recording::record,
    },
    #[cfg(feature = "renderplot-device")]
    EvaluatedBuiltin {
        name: ".rport_replayPlot",
        handler: crate::mainutils::graphics_recording::replay,
    },
    EvaluatedBuiltin {
        name: "lines",
        handler: crate::mainutils::essentials::do_lines,
    },
    EvaluatedBuiltin {
        name: "lines.default",
        handler: crate::mainutils::essentials::do_lines_default,
    },
    EvaluatedBuiltin {
        name: "points",
        handler: crate::mainutils::essentials::do_points,
    },
    EvaluatedBuiltin {
        name: "points.default",
        handler: crate::mainutils::essentials::do_points_default,
    },
    EvaluatedBuiltin {
        name: "segments",
        handler: crate::mainutils::essentials::do_segments,
    },
    EvaluatedBuiltin {
        name: "arrows",
        handler: crate::mainutils::essentials::do_arrows,
    },
    EvaluatedBuiltin {
        name: "abline",
        handler: crate::mainutils::essentials::do_abline,
    },
    EvaluatedBuiltin {
        name: "rect",
        handler: crate::mainutils::essentials::do_rect,
    },
    EvaluatedBuiltin {
        name: "polygon",
        handler: crate::mainutils::essentials::do_polygon,
    },
    EvaluatedBuiltin {
        name: "text",
        handler: crate::mainutils::essentials::do_text,
    },
    EvaluatedBuiltin {
        name: "text.default",
        handler: crate::mainutils::essentials::do_text_default,
    },
    EvaluatedBuiltin {
        name: "title",
        handler: crate::mainutils::essentials::do_title,
    },
    EvaluatedBuiltin {
        name: "axis",
        handler: crate::mainutils::essentials::do_axis,
    },
    EvaluatedBuiltin {
        name: "plot.new",
        handler: crate::mainutils::essentials::do_plot_new,
    },
    EvaluatedBuiltin {
        name: "plot.window",
        handler: crate::mainutils::essentials::do_plot_window,
    },
    EvaluatedBuiltin {
        name: "plot.default",
        handler: crate::mainutils::essentials::do_plot_default,
    },
    EvaluatedBuiltin {
        name: "biplot.default",
        handler: crate::mainutils::essentials::do_biplot_default,
    },
    EvaluatedBuiltin {
        name: "screeplot.default",
        handler: crate::mainutils::essentials::do_screeplot_default,
    },
    EvaluatedBuiltin {
        name: "monthplot.default",
        handler: crate::mainutils::essentials::do_monthplot_default,
    },
    EvaluatedBuiltin {
        name: "interaction.plot",
        handler: crate::mainutils::essentials::do_interaction_plot,
    },
    EvaluatedBuiltin {
        name: "lag.plot",
        handler: crate::mainutils::essentials::do_lag_plot,
    },
    EvaluatedBuiltin {
        name: "boxplot.stats",
        handler: crate::mainutils::essentials::do_boxplot_stats,
    },
    EvaluatedBuiltin {
        name: "qqplot",
        handler: crate::mainutils::essentials::do_qqplot,
    },
    EvaluatedBuiltin {
        name: "rect.hclust",
        handler: crate::mainutils::essentials::do_rect_hclust,
    },
    EvaluatedBuiltin {
        name: "stem",
        handler: crate::library::graphics::stem::do_stem,
    },
    EvaluatedBuiltin {
        name: "plot.stepfun",
        handler: crate::mainutils::essentials::do_plot_stepfun,
    },
    EvaluatedBuiltin {
        name: "plot.ecdf",
        handler: crate::mainutils::essentials::do_plot_ecdf,
    },
    EvaluatedBuiltin {
        name: "plot.spec.coherency",
        handler: crate::mainutils::essentials::do_plot_spec_coherency,
    },
    EvaluatedBuiltin {
        name: "plot.spec.phase",
        handler: crate::mainutils::essentials::do_plot_spec_phase,
    },
    EvaluatedBuiltin {
        name: "ppplot",
        handler: crate::mainutils::essentials::do_ppplot,
    },
    EvaluatedBuiltin {
        name: "termplot",
        handler: crate::mainutils::essentials::do_termplot,
    },
    EvaluatedBuiltin {
        name: "plot.ts",
        handler: crate::library::stats::filter::do_plot_ts,
    },
    EvaluatedBuiltin {
        name: "ts.plot",
        handler: crate::library::stats::filter::do_ts_plot,
    },
    EvaluatedBuiltin {
        name: "deviance",
        handler: crate::library::stats::filter::do_deviance,
    },
    EvaluatedBuiltin {
        // grDevices color primitives (existing implementations in
        // library::grdevices::colors, previously unreachable from R code;
        // needed by real packages such as crayon via col2rgb/colors).
        name: "col2rgb",
        handler: crate::library::grdevices::colors::do_col2rgb_builtin,
    },
    EvaluatedBuiltin {
        name: "colors",
        handler: crate::library::grdevices::colors::do_colors_builtin,
    },
    EvaluatedBuiltin {
        name: "palette",
        handler: crate::library::grdevices::colors::do_palette_builtin,
    },
    EvaluatedBuiltin {
        name: "rgb",
        handler: crate::library::grdevices::colors::do_rgb_builtin,
    },
    EvaluatedBuiltin {
        name: "hsv",
        handler: crate::library::grdevices::colors::do_hsv_builtin,
    },
    EvaluatedBuiltin {
        name: "gray",
        handler: crate::library::grdevices::colors::do_gray_builtin,
    },
    EvaluatedBuiltin {
        name: "grey",
        handler: crate::library::grdevices::colors::do_gray_builtin,
    },
    EvaluatedBuiltin {
        name: "parent.frame",
        handler: crate::mainutils::essentials::do_parent_frame,
    },
    EvaluatedBuiltin {
        name: "textConnection",
        handler: crate::mainutils::connections::do_textConnection,
    },
    EvaluatedBuiltin {
        name: "textConnectionValue",
        handler: crate::mainutils::connections::do_textConnectionValue,
    },
    EvaluatedBuiltin {
        // parse()/source() file-parse path: strict newline-else (gram.y
        // context-stack semantics); the interactive eval path stays lenient.
        name: "parse",
        handler: crate::mainutils::source::do_parse,
    },
    EvaluatedBuiltin {
        name: "parent.env",
        handler: crate::mainutils::essentials::do_parent_env,
    },
    EvaluatedBuiltin {
        // `parent.env(env) <- value` replacement form (same
        // `do_set_parent_env` handler; needed by real packages such as
        // crayon's rstudio-detect shim).
        name: "parent.env<-",
        handler: crate::mainutils::essentials::do_set_parent_env,
    },
    EvaluatedBuiltin {
        name: "par",
        handler: crate::mainutils::essentials::do_par,
    },
    EvaluatedBuiltin {
        name: "layout",
        handler: crate::mainutils::essentials::do_layout,
    },
    EvaluatedBuiltin {
        name: "getGraphicsEvent",
        handler: crate::mainutils::essentials::do_getGraphicsEvent,
    },
];
