//! Device text metrics. Decode R inputs before borrowing the renderer; allocate
//! the R result only after the renderer loan has ended.
use crate::mainutils::essentials::base_error;
use crate::sexp::ffi::SEXP;

#[cfg(not(feature = "renderplot-device"))]
pub(super) unsafe fn measure(_args: SEXP, _height: bool) -> SEXP {
    base_error("graphics string metrics require the renderplot-device feature")
}

#[cfg(feature = "renderplot-device")]
fn par_numbers(name: &str) -> Vec<f64> {
    use super::par::{ParValue, parameter};
    match parameter(name) {
        ParValue::Real(values) => values,
        ParValue::Integer(values) | ParValue::Logical(values) => {
            values.into_iter().map(f64::from).collect()
        }
        _ => vec![],
    }
}

#[cfg(feature = "renderplot-device")]
pub(super) unsafe fn measure(args: SEXP, height: bool) -> SEXP {
    use crate::mainutils::{
        coerce::{asInteger, asReal, coerceVector},
        plotmath::Label,
    };
    use crate::sexp::{accessors::*, ffi::SEXPTYPE, globals::R_NilValue, protect::protect};
    use r_graphics_engine::{FontFace, PlotParameters};
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
        let scale = if cex == nil {
            par_numbers("cex")[0]
        } else {
            asReal(cex)
        };
        if !scale.is_finite() || scale <= 0. {
            base_error("invalid 'cex' value");
        }
        let face = match if font == nil {
            par_numbers("font")[0] as i32
        } else {
            asInteger(font)
        } {
            1 => FontFace::Plain,
            2 => FontFace::Bold,
            3 => FontFace::Italic,
            4 => FontFace::BoldItalic,
            _ => base_error("unsupported font for portable string metrics"),
        };
        let family_arg =
            crate::mainutils::essentials::arg_by_name_or_position(cell, &["family"], usize::MAX);
        let family = if family_arg == nil {
            match super::par::parameter("family") {
                super::par::ParValue::String(value) => value,
                _ => String::new(),
            }
        } else {
            crate::mainutils::essentials::elt_to_string(family_arg, 0)
        };
        if !matches!(family.as_str(), "" | "sans" | "DejaVu Sans") {
            base_error("unsupported font family for portable string metrics");
        }
        let size = par_numbers("ps")[0] * scale;
        if !size.is_finite() || size > f32::MAX as f64 {
            base_error("invalid 'cex' value");
        }
        let params = PlotParameters {
            font_size: size as f32,
            font_face: face,
            dpi: 72.,
            ..Default::default()
        };
        let line_height = par_numbers("lheight")[0] * size * 1.2;
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
            "strwidth('x',font=5)",
            "strwidth('x',family='serif')",
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
