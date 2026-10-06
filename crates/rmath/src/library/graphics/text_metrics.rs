//! Device text metrics. Decode R inputs before borrowing the renderer; allocate
//! the R result only after the renderer loan has ended.
use crate::mainutils::essentials::base_error;
use crate::sexp::ffi::SEXP;

#[cfg(not(feature = "renderplot-device"))]
pub(super) unsafe fn measure(_args: SEXP, _height: bool) -> SEXP {
    base_error("graphics string metrics require the renderplot-device feature")
}

#[cfg(feature = "renderplot-device")]
fn positive_par_number(name: &str) -> f64 {
    use super::par::{ParValue, parameter};
    let value = match parameter(name) {
        ParValue::Real(values) if values.len() == 1 => values[0],
        ParValue::Integer(values) | ParValue::Logical(values) if values.len() == 1 => {
            f64::from(values[0])
        }
        _ => base_error(format!("invalid '{name}' graphics parameter")),
    };
    if !value.is_finite() || value <= 0. {
        base_error(format!("invalid '{name}' graphics parameter"));
    }
    value
}

/// Decode owned font parameters before borrowing a drawing target.
///
/// # Safety
/// R arguments must be live and rooted in the active session for coercion.
#[cfg(feature = "renderplot-device")]
pub(crate) unsafe fn text_parameters(
    cex: SEXP,
    font: SEXP,
    extras: SEXP,
) -> (r_graphics_engine::PlotParameters, f64) {
    use crate::mainutils::coerce::asReal;
    unsafe {
        let base_scale = positive_par_number("cex");
        let scale = if cex == crate::sexp::globals::R_NilValue() {
            base_scale
        } else {
            asReal(cex) * base_scale
        };
        let font = font_codes(font, 1)
            .first()
            .copied()
            .unwrap_or(crate::sexp::ffi::NA_INTEGER);
        font_parameters(scale, font, extras)
    }
}

/// Decode drawing parameters, using GNU text's fallback for invalid numeric cex.
///
/// # Safety
/// The argument pairlist and its children must be live and rooted in the active session.
#[cfg(feature = "renderplot-device")]
pub(crate) unsafe fn drawing_parameters(
    args: SEXP,
    count: usize,
) -> Vec<r_graphics_engine::PlotParameters> {
    use crate::mainutils::essentials::arg_by_name_or_position;
    unsafe {
        let cex = arg_by_name_or_position(args, &["cex"], usize::MAX);
        let font = arg_by_name_or_position(args, &["font"], usize::MAX);
        let base_scale = positive_par_number("cex");
        drawing_parameters_with_scale(cex, font, args, count, base_scale)
    }
}

/// Margin text uses absolute magnification, independently of par("cex").
#[cfg(feature = "renderplot-device")]
pub(crate) unsafe fn margin_parameters(
    cex: SEXP,
    font: SEXP,
    extras: SEXP,
    count: usize,
) -> Vec<r_graphics_engine::PlotParameters> {
    unsafe { drawing_parameters_with_scale(cex, font, extras, count, 1.) }
}

#[cfg(feature = "renderplot-device")]
unsafe fn drawing_parameters_with_scale(
    cex: SEXP,
    font: SEXP,
    args: SEXP,
    count: usize,
    base_scale: f64,
) -> Vec<r_graphics_engine::PlotParameters> {
    unsafe {
        let scales = numeric_prefix(cex, "cex", count);
        let fonts = font_codes(font, count);
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(count)
            .unwrap_or_else(|_| base_error("cannot reserve text drawing parameters"));
        for index in 0..count {
            let scale = if scales.is_empty() {
                1.
            } else {
                scales[index % scales.len()]
            };
            let scale = if scale.is_finite() && scale > 0. {
                scale
            } else {
                1.
            };
            let font = if fonts.is_empty() {
                crate::sexp::ffi::NA_INTEGER
            } else {
                fonts[index % fonts.len()]
            };
            parameters.push(font_parameters(scale * base_scale, font, args).0);
        }
        parameters
    }
}

// Copy only values that drawing can use. Checked element reads also keep
// compact sequences lazy when a tiny plot receives a very large argument.
#[cfg(feature = "renderplot-device")]
unsafe fn numeric_prefix(value: SEXP, name: &str, limit: usize) -> Vec<f64> {
    use crate::sexp::{accessors::*, ffi::SEXPTYPE, object::Sexp};
    unsafe {
        let count = usize::try_from(XLENGTH(value))
            .unwrap_or_else(|_| base_error("graphics argument is too long"));
        if count == 0 {
            return vec![];
        }
        let kind = SEXPTYPE(TYPEOF(value));
        if !matches!(
            kind,
            SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
        ) {
            base_error(format!("invalid '{name}' argument"));
        }
        // SAFETY: the caller roots numeric arguments in the active session.
        let value =
            Sexp::from_raw(value).unwrap_or_else(|| base_error("invalid graphics argument"));
        let mut values = Vec::new();
        values
            .try_reserve_exact(count.min(limit))
            .unwrap_or_else(|_| base_error("cannot reserve text argument values"));
        for index in 0..count.min(limit) {
            // This index is below the original nonnegative R_xlen_t length.
            let index = index as i64;
            let element = match kind {
                SEXPTYPE::REALSXP => value.real_elt(index),
                SEXPTYPE::INTSXP => value.integer_elt(index).map(f64::from),
                _ => value.logical_elt(index).map(f64::from),
            }
            .unwrap_or_else(|| base_error("cannot read graphics argument element"));
            values.push(element);
        }
        values
    }
}

#[cfg(feature = "renderplot-device")]
unsafe fn font_codes(font: SEXP, limit: usize) -> Vec<i32> {
    unsafe {
        numeric_prefix(font, "font", limit)
            .into_iter()
            .map(|value| {
                if value.is_finite() && (1. ..6.).contains(&value) {
                    value as i32
                } else {
                    crate::sexp::ffi::NA_INTEGER
                }
            })
            .collect()
    }
}

#[cfg(feature = "renderplot-device")]
unsafe fn font_parameters(
    scale: f64,
    requested_font: i32,
    extras: SEXP,
) -> (r_graphics_engine::PlotParameters, f64) {
    use crate::mainutils::coerce::coerceVector;
    use crate::sexp::{accessors::*, ffi::SEXPTYPE, globals::R_NilValue, protect::protect};
    use r_graphics_engine::{FontFace, PlotParameters};
    unsafe {
        let nil = R_NilValue();
        if !scale.is_finite() || scale <= 0. {
            base_error("invalid 'cex' value");
        }
        let font = if requested_font == crate::sexp::ffi::NA_INTEGER {
            positive_par_number("font") as i32
        } else {
            requested_font
        };
        let face = match font {
            1 => FontFace::Plain,
            2 => FontFace::Bold,
            3 => FontFace::Italic,
            4 => FontFace::BoldItalic,
            _ => base_error("unsupported font for portable string metrics"),
        };
        let mut family_arg = None;
        let mut cell = extras;
        while !cell.is_null() && cell != nil {
            match crate::mainutils::essentials::tag_name(cell).as_deref() {
                Some("family") => family_arg = Some(CAR(cell)),
                Some("vfont") if CAR(cell) != nil => {
                    base_error("Hershey fonts are not supported by the portable device")
                }
                _ => {}
            }
            cell = CDR(cell);
        }
        let family = if let Some(family_arg) = family_arg {
            let value = coerceVector(family_arg, SEXPTYPE::STRSXP.0);
            let _value = protect(value);
            if XLENGTH(value) != 1 {
                base_error("invalid 'family' graphics parameter");
            }
            if STRING_ELT(value, 0) == crate::sexp::globals::R_NaString() {
                String::new()
            } else {
                crate::mainutils::essentials::elt_to_string(value, 0)
            }
        } else {
            match super::par::parameter("family") {
                super::par::ParValue::String(value) => value,
                _ => base_error("invalid 'family' graphics parameter"),
            }
        };
        if !matches!(family.as_str(), "" | "sans" | "DejaVu Sans") {
            base_error("unsupported font family for portable string metrics");
        }
        let size = positive_par_number("ps") * scale;
        let font_size = size as f32;
        if !font_size.is_finite() || font_size <= 0. {
            base_error("font size is outside the portable renderer range");
        }
        let line_height = positive_par_number("lheight") * size * 1.2;
        if !line_height.is_finite() {
            base_error("graphics line height overflow");
        }
        (
            PlotParameters {
                font_size,
                font_face: face,
                dpi: 72.,
                ..Default::default()
            },
            line_height,
        )
    }
}

#[cfg(feature = "renderplot-device")]
pub(super) unsafe fn measure(args: SEXP, height: bool) -> SEXP {
    use crate::mainutils::{
        coerce::{asInteger, coerceVector},
        plotmath::Label,
    };
    use crate::sexp::{accessors::*, ffi::SEXPTYPE, globals::R_NilValue, protect::protect};
    unsafe {
        let nil = R_NilValue();
        let mut cell = CDR(args);
        let mut arguments = [nil; 5];
        for argument in &mut arguments {
            if cell.is_null() || cell == nil {
                base_error("too few arguments");
            }
            *argument = CAR(cell);
            cell = CDR(cell);
        }
        let [input, units, cex, font, vfont] = arguments;
        let units = asInteger(units);
        if !(1..=3).contains(&units) {
            base_error("invalid units");
        }
        if vfont != nil {
            base_error("Hershey font metrics are not supported by the portable device");
        }
        let (params, line_height) = text_parameters(cex, font, cell);
        let strings = if matches!(
            SEXPTYPE(TYPEOF(input)),
            SEXPTYPE::LANGSXP | SEXPTYPE::SYMSXP | SEXPTYPE::EXPRSXP
        ) {
            input
        } else {
            coerceVector(input, SEXPTYPE::STRSXP.0)
        };
        let _strings = protect(strings);
        let labels = crate::mainutils::plotmath::labels(strings);
        let missing: Vec<_> = if TYPEOF(strings) == SEXPTYPE::STRSXP {
            (0..XLENGTH(strings))
                .map(|index| STRING_ELT(strings, index) == crate::sexp::globals::R_NaString())
                .collect()
        } else {
            vec![false; labels.len()]
        };

        let (backend, coordinates) =
            crate::sexp::instance::with_required_current_instance(|instance| {
                (
                    (*instance).current_renderplot_backend,
                    (*instance).portable_graphics.current,
                )
            });
        let backend = backend.unwrap_or_else(|| {
            base_error("string metrics require an active portable graphics device")
        });
        let target = &*backend;
        let (width, device_height) = target.dimensions();
        let factor = match units {
            3 => 1. / 72.,
            2 => {
                let figure = coordinates
                    .map(|coordinates| coordinates.figure)
                    .unwrap_or([0., 0., width as f32, device_height as f32]);
                1. / if height {
                    f64::from(figure[3] - figure[1])
                } else {
                    f64::from(figure[2] - figure[0])
                }
            }
            _ => {
                let coordinates =
                    coordinates.unwrap_or_else(|| base_error("plot.new has not been called yet"));
                if height {
                    (coordinates.limits[3] - coordinates.limits[2])
                        / f64::from(coordinates.rect[3] - coordinates.rect[1])
                } else {
                    (coordinates.limits[1] - coordinates.limits[0])
                        / f64::from(coordinates.rect[2] - coordinates.rect[0])
                }
            }
        };
        let measured: Vec<f64> = labels
            .iter()
            .zip(missing)
            .map(|(label, missing)| {
                let pixels = if missing {
                    0.
                } else {
                    match label {
                        Label::Text(text) if height => {
                            f64::from(target.measure_math_text("M", &params).ascent)
                                + text.bytes().filter(|byte| *byte == b'\n').count() as f64
                                    * line_height
                        }
                        Label::Text(text) => text
                            .split('\n')
                            .map(|line| f64::from(target.measure_text(line, &params).width))
                            .fold(0., f64::max),
                        Label::Math(expression) => {
                            let layout = expression.layout(target, &params);
                            f64::from(if height {
                                layout.ascent + layout.descent
                            } else {
                                layout.width
                            })
                        }
                    }
                };
                let value = pixels * factor;
                if !value.is_finite() {
                    base_error("graphics string metric conversion overflow");
                }
                value
            })
            .collect();
        // No renderer reference is used after this point, including during allocation/GC.
        let result =
            crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::REALSXP, measured.len() as i64);
        for (index, value) in measured.into_iter().enumerate() {
            *REAL(result).add(index) = value;
        }
        result
    }
}

#[cfg(all(test, feature = "renderplot-device"))]
mod tests {
    use crate::sexp::session::RSession;

    fn measure(code: &str) -> Vec<f64> {
        let mut session = RSession::new_without_default_packages();
        let mut scene = r_graphics_engine::Scene::new(640, 480);
        session.eval_script_with_output_capture_then_renderplot(
            &format!("plot.new();{code}"),
            &mut scene,
            |value, _, _| unsafe {
                // SAFETY: the session roots its result for this synchronous callback.
                let value = value.unwrap();
                (0..value.clone().len())
                    .map(|index| value.real_elt(index).unwrap())
                    .collect()
            },
        )
    }

    #[test]
    fn graphics_string_metrics_follow_text_and_font_scaling() {
        let widths = measure("strwidth(c('i','WWW'),units='inches')");
        assert!(
            widths[1] > widths[0] * 3.,
            "glyph widths must differ: {widths:?}"
        );
        let widths =
            measure("c(strwidth('WWW',units='inches'),strwidth('WWW',units='inches',cex=2))");
        assert!((widths[1] / widths[0] - 2.).abs() < 1e-6);
        let widths = measure(
            "c(strwidth('WWW',units='inches',font=1),strwidth('WWW',units='inches',font=2))",
        );
        assert!(widths[1] > widths[0]);
        let maths = measure(
            "c(strwidth(expression(frac(x,2)),units='inches'),strheight(expression(frac(x,2)),units='inches'))",
        );
        assert!(maths.iter().all(|value| value.is_finite() && *value > 0.));
    }

    #[test]
    fn graphics_string_metrics_preserve_base_scale_and_missing_font() {
        let widths = measure(
            "w<-strwidth('WWW',units='inches');par(cex=2);c(w,strwidth('WWW',units='inches'),strwidth('WWW',units='inches',cex=1),strwidth('WWW',units='inches',cex=2))",
        );
        for (index, multiplier) in [(1, 2.), (2, 2.), (3, 4.)] {
            assert!((widths[index] / widths[0] - multiplier).abs() < 1e-6);
        }
        let widths = measure(
            "par(font=2);c(strwidth('WWW',units='inches'),strwidth('WWW',units='inches',font=NA_integer_))",
        );
        assert_eq!(widths[0], widths[1]);
        let widths = measure(
            "c(strwidth('WWW',units='inches'),strwidth('WWW',units='inches',family=NA_character_))",
        );
        assert_eq!(widths[0], widths[1]);
    }

    #[test]
    fn graphics_string_metrics_reject_malformed_parameter_state() {
        let mut session = RSession::new_without_default_packages();
        let mut scene = r_graphics_engine::Scene::new(640, 480);
        for parameter in ["ps", "cex", "font", "lheight"] {
            for value in ["numeric(0)", "'bad'", "NA_real_", "0", "-1", "c(1,2)"] {
                let error = session.eval_script_with_output_capture_then_renderplot(
                    &format!("par(ps=12,cex=1,font=1,lheight=1);plot.new();par({parameter}={value});strwidth('x',units='inches')"),
                    &mut scene,
                    |value, _, _| value.unwrap_err().message,
                );
                assert!(
                    error.contains(&format!("invalid '{parameter}' graphics parameter")),
                    "{parameter}={value}: {error}"
                );
            }
        }
        let (value, _, _) =
            session.eval_code_with_output_capture("par(ps=12,cex=1,font=1,lheight=1);1L+1L");
        unsafe {
            // SAFETY: no R reentry occurs while reading the live session result.
            assert_eq!(value.unwrap().integer_elt(0), Some(2));
        }
    }

    #[test]
    fn graphics_string_metrics_out_of_range_fonts_use_current_font() {
        let widths = measure(
            "par(font=2);c(strwidth('WWW',units='inches'),strwidth('WWW',units='inches',font=0),strwidth('WWW',units='inches',font=6))",
        );
        assert_eq!(widths, vec![widths[0]; 3]);
    }

    #[test]
    fn graphics_string_metrics_agree_with_drawn_font_parameters() {
        let mut session = RSession::new_without_default_packages();
        let mut scene = r_graphics_engine::Scene::new(640, 480);
        let width = session.eval_script_with_output_capture_then_renderplot(
            "plot.new();par(ps=18,cex=2,font=2);text(.5,.5,'WWW',cex=1.5);strwidth('WWW',units='inches',cex=1.5)",
            &mut scene,
            |value, _, _| unsafe {
                // SAFETY: the callback's result is live and no R reentry occurs.
                value.unwrap().real_elt(0).unwrap()
            },
        );
        let params = scene
            .operations()
            .iter()
            .find_map(|operation| match operation {
                r_graphics_engine::DrawOperation::Text { text, params, .. } if text == "WWW" => {
                    Some(params)
                }
                _ => None,
            })
            .expect("text must be drawn");
        assert_eq!(params.font_face, r_graphics_engine::FontFace::Bold);
        assert_eq!(params.font_size, 54.);
        use r_graphics_engine::DrawTarget;
        assert!((f64::from(scene.measure_text("WWW", params).width) - width * 72.).abs() < 1e-5);
    }

    #[test]
    fn graphics_string_metrics_drawing_recycles_label_fonts_and_scales() {
        let mut session = RSession::new_without_default_packages();
        let mut scene = r_graphics_engine::Scene::new(640, 480);
        session.eval_script_with_output_capture_then_renderplot(
            "plot.new();par(ps=18,cex=2);text(c(.2,.4,.6,.8),rep(.5,4),c('one','two','three','four'),cex=c(1,2,0),font=1:2)",
            &mut scene,
            |value, _, _| { value.unwrap(); },
        );
        let parameters: Vec<_> = scene
            .operations()
            .iter()
            .filter_map(|operation| match operation {
                r_graphics_engine::DrawOperation::Text { params, .. } => {
                    Some((params.font_size, params.font_face))
                }
                _ => None,
            })
            .collect();
        use r_graphics_engine::FontFace::{Bold, Plain};
        assert_eq!(
            parameters,
            vec![(36., Plain), (72., Bold), (36., Plain), (36., Bold)]
        );
    }

    #[test]
    fn graphics_string_metrics_survive_gc_torture_and_restore_parameters() {
        let expression = "c(strwidth(c('i','WWW'),units='inches'),strheight(expression(frac(x,2)),units='inches'))";
        let baseline = measure(expression);
        let tortured = measure(&format!(
            "gctorture2(20);w<-{expression};gctorture(FALSE);w"
        ));
        assert_eq!(baseline, tortured);
        assert_eq!(
            measure(
                "before<-par('font');strwidth('WWW',units='inches',font=2);c(as.double(before),as.double(par('font')))"
            ),
            vec![1., 1.]
        );
    }

    #[test]
    fn graphics_string_metrics_drawing_reads_only_used_compact_values() {
        let mut session = RSession::new_without_default_packages();
        let mut scene = r_graphics_engine::Scene::new(640, 480);
        session.eval_script_with_output_capture_then_renderplot(
            "plot.new();text(c(.2,.4,.6,.8),rep(.5,4),c('one','two','three','four'),cex=1:1000000000,font=1:1000000000)",
            &mut scene,
            |value, _, _| { value.unwrap(); },
        );
        let parameters: Vec<_> = scene
            .operations()
            .iter()
            .filter_map(|operation| match operation {
                r_graphics_engine::DrawOperation::Text { params, .. } => {
                    Some((params.font_size, params.font_face))
                }
                _ => None,
            })
            .collect();
        use r_graphics_engine::FontFace::{Bold, BoldItalic, Italic, Plain};
        assert_eq!(
            parameters,
            vec![(12., Plain), (24., Bold), (36., Italic), (48., BoldItalic)]
        );
    }

    #[test]
    fn graphics_string_metrics_drawing_normalizes_cex_separately() {
        let mut session = RSession::new_without_default_packages();
        let mut scene = r_graphics_engine::Scene::new(640, 480);
        for cex in ["0", "-1", "NA_real_", "numeric(0)", "character(0)", "NULL"] {
            session.eval_script_with_output_capture_then_renderplot(
                &format!("plot.new();par(cex=2);text(.5,.5,'fallback',cex={cex})"),
                &mut scene,
                |value, _, _| {
                    value.unwrap();
                },
            );
            let params = scene
                .operations()
                .iter()
                .rev()
                .find_map(|operation| match operation {
                    r_graphics_engine::DrawOperation::Text { text, params, .. }
                        if text == "fallback" =>
                    {
                        Some(params)
                    }
                    _ => None,
                })
                .expect("text must be drawn");
            assert_eq!(params.font_size, 24., "cex={cex}");
        }
        let error = session.eval_script_with_output_capture_then_renderplot(
            "plot.new();text(.5,.5,'bad',cex='2')",
            &mut scene,
            |value, _, _| value.unwrap_err().message,
        );
        assert!(error.contains("invalid 'cex' argument"), "{error}");
    }

    #[test]
    fn graphics_string_metrics_handle_units_empty_na_and_registered_routes() {
        let widths = measure(
            "c(strwidth('WWW',units='inches'),.External.graphics('C_strWidth','WWW',3L,NULL,NULL,NULL))",
        );
        assert_eq!(widths[0], widths[1]);
        assert_eq!(
            measure("strwidth(c('',NA_character_),units='inches')"),
            vec![0.; 2]
        );
        let heights = measure("strheight(c('',NA_character_,'M'),units='inches')");
        assert_eq!(heights[0], heights[2]);
        assert_eq!(heights[1], 0.);
        assert!(heights[0] > 0.);
        let values = measure(
            "c(strwidth('WWW',units='inches'),strwidth('WWW',units='figure'),strwidth('WWW',units='user'),diff(par('usr')[1:2]),par('pin')[1])",
        );
        assert!((values[1] * 640. - values[0] * 72.).abs() < 1e-5);
        assert!((values[2] * values[4] / values[3] - values[0]).abs() < 1e-5);
        let heights = measure(
            "c(strheight('M',units='inches'),strheight('M',units='inches',cex=2),strheight('M\nM',units='inches'))",
        );
        assert!((heights[1] / heights[0] - 2.).abs() < 1e-6);
        assert!(heights[2] > heights[0]);
    }

    #[test]
    fn graphics_string_metrics_reject_unsupported_devices_and_fonts_recoverably() {
        let mut session = RSession::new_without_default_packages();
        assert!(
            session
                .eval_code_with_output_capture("strwidth('WWW',units='inches')")
                .0
                .is_err()
        );
        let mut scene = r_graphics_engine::Scene::new(640, 480);
        for code in [
            "strwidth('x',units='bad')",
            "strwidth('x',cex=0)",
            "strwidth('x',cex=1e-100)",
            "strheight('x',cex=1e100)",
            "strwidth('x',font=5)",
            "strwidth('x',font='2')",
            "strwidth('x',family='serif')",
            "strwidth('x',family=character())",
            "strwidth('x',family=c('sans','sans'))",
            "strwidth('x',family=NULL)",
            "strwidth('x',vfont=c('serif','plain'))",
        ] {
            let error = session.eval_script_with_output_capture_then_renderplot(
                &format!("plot.new();{code}"),
                &mut scene,
                |value, _, _| value.is_err(),
            );
            assert!(error, "{code}");
        }
        let (value, _, _) = session.eval_code_with_output_capture("1L+1L");
        unsafe {
            // SAFETY: the live result is read without any reentry.
            assert_eq!(value.unwrap().integer_elt(0), Some(2));
        }
    }
}
