//! Retain evaluated grid arguments so replay can resolve units on the new device.
use super::{GridState, apply_grid_operation};
use crate::mainutils::duplicate::lazy_duplicate;
use crate::sexp::{
    accessors::*,
    constructors::Rf_cons,
    ffi::{SEXP, SEXPTYPE},
    globals::R_NilValue,
    object::{Sexp, SexpResult},
    owner::OwnerToken,
    protect::protect,
    symbol::Rf_install,
};
use r_graphics_engine::DrawTarget;
use std::ffi::CStr;

const MAX_COMMANDS: usize = 4096;
const MAX_ARGUMENT_ELEMENTS: usize = 262_144;
const MAX_ARGUMENT_DEPTH: usize = 64;
const INVALID: &str = "invalid recorded grid commands";

const OPERATIONS: [&CStr; 16] = [
    c"newpage",
    c"push",
    c"pop",
    c"up",
    c"down",
    c"seek",
    c"draw",
    c"record",
    c"record.push",
    c"record.pop",
    c"record.up",
    c"record.down",
    c"grob.push",
    c"grob.pop",
    c"grob.up",
    c"grob.down",
];

#[derive(Clone, Copy)]
pub(crate) struct Command(usize);

impl Command {
    pub(super) fn named(name: &str) -> Option<Self> {
        OPERATIONS
            .iter()
            .position(|s| s.to_bytes() == name.as_bytes())
            .map(Self)
    }

    pub(crate) fn starts_page(self) -> bool {
        self.0 == 0
    }

    fn name(self) -> &'static str {
        OPERATIONS[self.0]
            .to_str()
            .expect("static ASCII grid command")
    }
}

#[derive(Clone, Default)]
pub(crate) struct CommandLog {
    head: Option<Sexp<'static>>,
    len: usize,
}

impl CommandLog {
    pub(crate) fn head(&self) -> Option<&Sexp<'static>> {
        self.head.as_ref()
    }

    pub(crate) fn append(&mut self, kind: Command, value: SEXP) -> SexpResult<bool> {
        if self.len >= MAX_COMMANDS {
            return Ok(false);
        }
        unsafe {
            let owner = OwnerToken::current()?;
            let tag = Rf_install(OPERATIONS[kind.0].as_ptr());
            let tail = self
                .head
                .as_ref()
                .map_or_else(|| R_NilValue(), Sexp::as_raw);
            let pair = Rf_cons(lazy_duplicate(value), tail);
            let _pair = protect(pair);
            SETTAG(pair, tag);
            self.head = Some(owner.sexp(pair)?.into_owned()?);
            self.len += 1;
        }
        Ok(true)
    }
}

pub(crate) struct ReplayCommands {
    log: CommandLog,
    steps: Vec<(Command, SEXP)>,
}

impl ReplayCommands {
    pub(crate) fn read(head: &Sexp<'static>) -> Result<Self, &'static str> {
        let mut steps = Vec::new();
        let mut current = head.as_raw();
        let mut remaining = MAX_ARGUMENT_ELEMENTS;
        let tags = OPERATIONS.map(|name| crate::sexp::symbol::install_cstr(&name));
        while current != R_NilValue() {
            if steps.len() == MAX_COMMANDS || TYPEOF(current) != SEXPTYPE::LISTSXP {
                return Err(INVALID);
            }
            let index = tags
                .iter()
                .position(|tag| *tag == TAG(current))
                .ok_or(INVALID)?;
            let value = CAR(current);
            validate_argument(value, 0, &mut remaining)?;
            steps.push((Command(index), value));
            current = CDR(current);
        }
        steps.reverse();
        if steps.is_empty() || steps.iter().skip(1).any(|(kind, _)| kind.starts_page()) {
            return Err(INVALID);
        }
        let log = CommandLog {
            head: Some(head.clone()),
            len: steps.len(),
        };
        Ok(Self { log, steps })
    }

    pub(crate) fn first_grid_device_command(&self) -> Option<usize> {
        // GNU's native grid page and viewport commands contain registrations
        // that must be restored after same-process serialization.
        self.steps.iter().position(|(kind, value)| {
            (1..=5).contains(&kind.0) || (kind.starts_page() && super::logical_flag(*value, true))
        })
    }

    pub(crate) fn draw_prefix(&self, end: usize, target: &mut dyn DrawTarget, dpi: f64) {
        Self::draw_steps(&self.steps[..end], target, dpi);
    }

    pub(crate) fn draw(self, target: &mut dyn DrawTarget, dpi: f64) -> (GridState, CommandLog) {
        (Self::draw_steps(&self.steps, target, dpi), self.log)
    }

    fn draw_steps(steps: &[(Command, SEXP)], target: &mut dyn DrawTarget, dpi: f64) -> GridState {
        let mut state = GridState::default();
        let dims = target.dimensions();
        state.ensure(dims, dpi);
        for &(command, value) in steps {
            apply_grid_operation(command.name(), value, &mut state, target, dims, dpi);
        }
        state
    }
}

fn validate_argument(value: SEXP, depth: usize, remaining: &mut usize) -> Result<(), &'static str> {
    if value.is_null() || value == R_NilValue() {
        return Ok(());
    }
    if depth >= MAX_ARGUMENT_DEPTH {
        return Err("recorded grid argument is too deeply nested");
    }
    if ALTREP(value) != 0 {
        return Err("recorded grid arguments require ordinary vectors");
    }
    *remaining = remaining.checked_sub(1).ok_or(INVALID)?;
    let kind = SEXPTYPE(TYPEOF(value));
    match kind {
        SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => {
            *remaining = remaining
                .checked_sub(XLENGTH(value) as usize)
                .ok_or(INVALID)?;
            let vector = super::node_factory().wrap(value).map_err(|_| INVALID)?;
            for i in 0..XLENGTH(value) {
                let element = vector
                    .try_vector_elt(i)
                    .map(|element| element.as_raw())
                    .map_err(|_| INVALID)?;
                validate_argument(element, depth + 1, remaining)?;
            }
        }
        SEXPTYPE::LANGSXP | SEXPTYPE::LISTSXP => {
            validate_argument(CAR(value), depth + 1, remaining)?;
            validate_argument(CDR(value), depth + 1, remaining)?;
        }
        SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP | SEXPTYPE::STRSXP => {
            *remaining = remaining
                .checked_sub(XLENGTH(value) as usize)
                .ok_or(INVALID)?;
        }
        SEXPTYPE::SYMSXP => return Ok(()),
        _ => return Err("recorded grid argument has an unsupported R type"),
    }
    validate_argument(ATTRIB(value), depth + 1, remaining)
}
