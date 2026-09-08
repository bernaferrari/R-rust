//! R-owned graphics recordings: serialized device commands, never raw pointers.
use crate::eval::attrib_core::{R_ClassSymbol, getAttrib, setAttrib};
use crate::library::graphics::par::{ParValue, set_plot_parameter};
use crate::mainutils::essentials::{base_error, elt_to_string};
use crate::mainutils::portable_plot::Coordinates;
use crate::sexp::{
    accessors::*,
    constructors::*,
    ffi::{SEXP, SEXPTYPE},
    globals::R_NilValue,
    instance::with_required_current_instance,
    protect::protect,
    symbol::Rf_install,
};
use r_graphics_engine::Scene;

const RECORDING_STATE_LEN: usize = 18;

unsafe fn recording_state_symbol() -> SEXP {
    unsafe { Rf_install(c"rport.graphics.state".as_ptr()) }
}

fn scale_rect(rect: &mut [f32; 4], sx: f32, sy: f32) {
    rect[0] *= sx;
    rect[2] *= sx;
    rect[1] *= sy;
    rect[3] *= sy;
}

unsafe fn snapshot_portable_state(recording: SEXP) {
    unsafe {
        let Some(coords) =
            with_required_current_instance(|inst| unsafe { (*inst).portable_graphics.current })
        else {
            return;
        };
        let state_symbol = recording_state_symbol();
        let state = Rf_allocVector3(SEXPTYPE::REALSXP, RECORDING_STATE_LEN as i64);
        let _state_guard = protect(state);
        let values = [
            coords.limits[0],
            coords.limits[1],
            coords.limits[2],
            coords.limits[3],
            coords.rect[0] as f64,
            coords.rect[1] as f64,
            coords.rect[2] as f64,
            coords.rect[3] as f64,
            coords.figure[0] as f64,
            coords.figure[1] as f64,
            coords.figure[2] as f64,
            coords.figure[3] as f64,
            coords.device[0] as f64,
            coords.device[1] as f64,
            coords.device[2] as f64,
            coords.device[3] as f64,
            if coords.log[0] { 1. } else { 0. },
            if coords.log[1] { 1. } else { 0. },
        ];
        for (index, value) in values.into_iter().enumerate() {
            *REAL(state).add(index) = value;
        }
        setAttrib(recording, state_symbol, state);
    }
}

unsafe fn decode_portable_state(recording: SEXP) -> Result<Option<Coordinates>, &'static str> {
    unsafe {
        let state = getAttrib(recording, recording_state_symbol());
        if state == R_NilValue() {
            return Ok(None);
        }
        if TYPEOF(state) != SEXPTYPE::REALSXP || XLENGTH(state) != RECORDING_STATE_LEN as i64 {
            return Err("recorded plot has invalid graphics state metadata");
        }
        let values: Vec<f64> = (0..RECORDING_STATE_LEN)
            .map(|index| *REAL(state).add(index))
            .collect();
        const MAX_METADATA_COORDINATE: f64 = 1.0e9;
        let converted_coordinates = values[4..16]
            .iter()
            .map(|value| *value as f32)
            .collect::<Vec<_>>();
        if values.iter().any(|value| !value.is_finite())
            || converted_coordinates.iter().any(|value| !value.is_finite())
            || values[4..16]
                .iter()
                .any(|value| value.abs() > MAX_METADATA_COORDINATE)
            || values[0] == values[1]
            || values[2] == values[3]
            || values[4] >= values[6]
            || values[5] >= values[7]
            || values[8] >= values[10]
            || values[9] >= values[11]
            || values[12] >= values[14]
            || values[13] >= values[15]
            || !values[16].is_finite()
            || !values[17].is_finite()
            || ![0., 1.].contains(&values[16])
            || ![0., 1.].contains(&values[17])
        {
            return Err("recorded plot has invalid graphics state metadata");
        }
        Ok(Some(Coordinates {
            limits: [values[0], values[1], values[2], values[3]],
            rect: [
                values[4] as f32,
                values[5] as f32,
                values[6] as f32,
                values[7] as f32,
            ],
            figure: [
                values[8] as f32,
                values[9] as f32,
                values[10] as f32,
                values[11] as f32,
            ],
            device: [
                values[12] as f32,
                values[13] as f32,
                values[14] as f32,
                values[15] as f32,
            ],
            log: [values[16] != 0., values[17] != 0.],
        }))
    }
}

unsafe fn restore_portable_state(
    state: Coordinates,
    source_dimensions: (u32, u32),
    target_dimensions: (u32, u32),
) {
    let (source_width, source_height) = source_dimensions;
    let (target_width, target_height) = target_dimensions;
    if source_width == 0 || source_height == 0 {
        return;
    }
    let sx = target_width as f32 / source_width as f32;
    let sy = target_height as f32 / source_height as f32;
    let mut scaled = state;
    scale_rect(&mut scaled.rect, sx, sy);
    scale_rect(&mut scaled.figure, sx, sy);
    scale_rect(&mut scaled.device, sx, sy);
    with_required_current_instance(|inst| unsafe {
        (*inst).portable_graphics.current = Some(scaled);
    });
    set_plot_parameter("usr", ParValue::Real(scaled.limits.to_vec()));
    set_plot_parameter("xlog", ParValue::Logical(vec![i32::from(scaled.log[0])]));
    set_plot_parameter("ylog", ParValue::Logical(vec![i32::from(scaled.log[1])]));
}

pub(crate) unsafe fn do_record(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "recordPlot",
            "function(load=NULL,attach=NULL) {if(!is.null(load)||!is.null(attach)) stop('recordPlot package reload metadata is not supported'); .rport_recordPlot()}",
            args,
            rho,
            false,
        )
    }
}
pub(crate) unsafe fn do_replay(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "replayPlot",
            "function(x,reloadPkgs=FALSE) {if(reloadPkgs) stop('replayPlot package reload is not supported'); .rport_replayPlot(x)}",
            args,
            rho,
            false,
        )
    }
}
pub(crate) unsafe fn record(_: SEXP, _: SEXP, _: SEXP, _: SEXP) -> SEXP {
    unsafe {
        let bytes = with_required_current_instance(|inst| {
            (*inst)
                .graphics_recording
                .as_ref()
                .map(|s| s.borrow().encode())
        })
        .unwrap_or_else(|| base_error("no recorded graphics device"))
        .unwrap_or_else(|e| base_error(format!("cannot record plot: {e}")));
        let result = Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as i64);
        let _result = protect(result);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(result), bytes.len());
        setAttrib(
            result,
            R_ClassSymbol(),
            Rf_mkString(c"recordedplot".as_ptr()),
        );
        snapshot_portable_state(result);
        result
    }
}
pub(crate) unsafe fn replay(_: SEXP, _: SEXP, args: SEXP, _: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            base_error("missing recorded plot");
        }
        let recording = CAR(args);
        let class = getAttrib(recording, R_ClassSymbol());
        if TYPEOF(recording) != SEXPTYPE::RAWSXP
            || TYPEOF(class) != SEXPTYPE::STRSXP
            || XLENGTH(class) == 0
            || elt_to_string(class, 0) != "recordedplot"
        {
            base_error("invalid recorded plot");
        }
        let length = XLENGTH(recording) as usize;
        let bytes = if length == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(RAW(recording), length)
        };
        let scene = Scene::decode(bytes)
            .unwrap_or_else(|e| base_error(format!("invalid recorded plot: {e}")));
        let target = with_required_current_instance(|inst| (*inst).current_renderplot_backend)
            .unwrap_or_else(|| base_error("no active graphics device"));
        let state = decode_portable_state(recording).unwrap_or_else(|e| base_error(e));
        let source_dimensions = scene.dimensions();
        let target_dimensions = (&*target).dimensions();
        scene.replay_scaled(&mut *target);
        if let Some(state) = state {
            restore_portable_state(state, source_dimensions, target_dimensions);
        }
        crate::eval::runtime::set_visible(0);
        R_NilValue()
    }
}
