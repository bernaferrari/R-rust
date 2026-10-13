#![forbid(unsafe_code)]
//! Original single-column least squares, with owning inputs and initialized outputs.
use crate::sexp::{
    ffi::{Envsxp, NodeBody, SEXPTYPE},
    object::{Sexp, SexpResult},
    owner::RuntimeAccess,
};

pub(super) type Value = Sexp<'static>;
type Result<T> = std::result::Result<T, String>;
fn checked<T>(result: SexpResult<T>) -> Result<T> {
    result.map_err(|error| error.to_string())
}
fn active(access: &RuntimeAccess) -> Result<()> {
    checked(access.require_active())
}
fn owned(value: Sexp<'_>) -> Result<Value> {
    checked(value.into_owned())
}
fn attribute(value: &Value, name: &[u8]) -> Result<Value> {
    let mut cell = checked(value.try_attrib())?;
    let mut visited = std::collections::HashSet::new();
    while !cell.is_nil() {
        if !visited.insert(checked(cell.allocation())?.link()) {
            return Err("cyclic attribute list".into());
        }
        if checked(cell.try_tag_name_eq(name))? {
            return owned(checked(cell.try_car())?);
        }
        cell = checked(cell.try_cdr())?;
    }
    owned(cell)
}

/// Captured before response evaluation or any data/environment allocation.
pub(super) struct Request {
    pub expression: Value,
    pub parent: Value,
    data: Value,
    data_rows: Value,
    call: Value,
}
impl Request {
    pub(super) fn capture(
        access: &RuntimeAccess,
        arguments: &Value,
        call: Value,
        parent: Value,
    ) -> Result<Option<Self>> {
        active(access)?;
        if arguments.is_nil() {
            return Ok(None);
        }
        let formula = owned(checked(arguments.try_car())?)?;
        if formula.typeof_() != SEXPTYPE::LANGSXP {
            return Ok(None);
        }
        let head = owned(checked(formula.try_car())?)?;
        if head.typeof_() != SEXPTYPE::SYMSXP
            || checked(checked(head.try_printname())?.try_as_string())? != "~"
        {
            return Ok(None);
        }
        let operands = owned(checked(formula.try_cdr())?)?;
        let expression = owned(checked(operands.try_car())?)?;
        let tail = owned(checked(operands.try_cdr())?)?;
        if tail.is_nil() {
            return Ok(None);
        }
        let rhs = owned(checked(tail.try_car())?)?;
        if rhs.header().sxpinfo.alt() || rhs.len() != 1 {
            return Ok(None);
        }
        let intercept = match rhs.typeof_() {
            SEXPTYPE::INTSXP => checked(rhs.try_integer_elt(0))? == 1,
            SEXPTYPE::REALSXP => checked(rhs.try_real_elt(0))? == 1.,
            _ => false,
        };
        if !intercept {
            return Ok(None);
        }
        let environment = attribute(&formula, b".Environment")?;
        let parent = if environment.typeof_() == SEXPTYPE::ENVSXP {
            environment
        } else {
            parent
        };
        let mut data = access.domain().nil();
        let mut cell = owned(checked(arguments.try_cdr())?)?;
        let mut seen = std::collections::HashSet::new();
        while !cell.is_nil() {
            if !seen.insert(checked(cell.allocation())?.link()) {
                return Err("cyclic lm argument list".into());
            }
            let tag = checked(cell.try_tag())?;
            let name = if tag.is_nil() {
                String::new()
            } else {
                checked(checked(tag.try_printname())?.try_as_string())?
            };
            if name == "data" || (name.is_empty() && data.is_nil()) {
                data = owned(checked(cell.try_car())?)?;
                if name == "data" {
                    break;
                }
            }
            cell = owned(checked(cell.try_cdr())?)?;
        }
        let data_rows = attribute(&data, b"row.names")?;
        active(access)?;
        Ok(Some(Self {
            expression,
            parent,
            data,
            data_rows,
            call,
        }))
    }

    fn labels(&self, access: &RuntimeAccess, response: &Value) -> Result<Vec<String>> {
        let supplied = if self.data_rows.is_nil() {
            attribute(response, b"names")?
        } else {
            self.data_rows.clone()
        };
        if supplied.typeof_() == SEXPTYPE::STRSXP && supplied.len() == response.len() {
            let mut labels = Vec::new();
            for index in 0..supplied.len() {
                labels.push(
                    checked(supplied.try_string_value_elt(index))?
                        .ok_or("missing model row name")?,
                );
                active(access)?;
            }
            Ok(labels)
        } else {
            Ok((1..=response.len())
                .map(|index| index.to_string())
                .collect())
        }
    }

    /// Copy value roots before any name provider or subsequent allocation.
    pub(super) fn bindings(&self, access: &RuntimeAccess) -> Result<Vec<(String, Value)>> {
        if self.data.typeof_() != SEXPTYPE::VECSXP {
            return Ok(Vec::new());
        }
        let names = attribute(&self.data, b"names")?;
        let mut values = Vec::new();
        for index in 0..self.data.len() {
            values.push(owned(checked(self.data.try_vector_elt(index))?)?);
            active(access)?;
        }
        if names.typeof_() != SEXPTYPE::STRSXP {
            return Ok(Vec::new());
        }
        let mut bindings = Vec::new();
        for (index, value) in values.into_iter().enumerate().take(names.len() as usize) {
            let name = checked(names.try_string_value_elt(index as i64))?;
            active(access)?;
            if let Some(name) = name.filter(|name| !name.is_empty()) {
                bindings.push((name, value));
            }
        }
        Ok(bindings)
    }

    pub(super) fn environment(
        &self,
        access: &RuntimeAccess,
        bindings: &[(Value, Value)],
    ) -> Result<Value> {
        if self.data.typeof_() == SEXPTYPE::ENVSXP {
            return Ok(self.data.clone());
        }
        if self.data.typeof_() != SEXPTYPE::VECSXP {
            return Ok(self.parent.clone());
        }
        let domain = access.domain();
        let allocator = checked(access.allocator(&domain))?;
        let mut frame = domain.nil();
        for (symbol, value) in bindings.iter().rev() {
            frame = checked(allocator.pairlist_cell(value, &frame, symbol))?;
        }
        let frame_link = checked(domain.link(&frame))?;
        let parent = checked(domain.link(&self.parent))?;
        let nil = checked(domain.link(&domain.nil()))?;
        let environment = checked(allocator.allocate(|arena| {
            let pointer = arena.alloc_node(SEXPTYPE::ENVSXP);
            let node = arena.node_token(pointer)?;
            let heap = arena.heap_identity();
            let mut header = heap.node_snapshot(&node)?;
            header.data = NodeBody::Environment(Envsxp {
                frame: frame_link,
                enclos: parent,
                hashtab: nil,
            });
            heap.replace_node(&node, header)?;
            Some(pointer)
        }))?;
        active(access)?;
        Ok(environment)
    }
}

pub(super) struct Symbols {
    pub names: Value,
    pub class: Value,
    pub dim: Value,
    pub dimnames: Value,
    pub assign: Value,
    pub formula: Value,
}

/// Constant-column specialization of original dqrdc2 followed by dqrsl(job=1110).
/// The same Householder operation is used for every retained response length.
struct Fit {
    coefficient: f64,
    residuals: Vec<f64>,
    fitted: Vec<f64>,
    effects: Vec<f64>,
    qr: Vec<f64>,
    qraux: f64,
}
fn fit_column(response: &[f64]) -> Option<Fit> {
    response.first()?;
    if response.len() == 1 {
        return Some(Fit {
            coefficient: response[0],
            residuals: vec![0.],
            fitted: response.to_vec(),
            effects: response.to_vec(),
            qr: vec![1.],
            qraux: 1.,
        });
    }
    let norm = (response.len() as f64).sqrt();
    let scale = 1. / norm;
    let qraux = 1. + scale;
    let reflect = |values: &mut [f64]| {
        let mut dot = qraux * values[0];
        for value in &values[1..] {
            dot += scale * *value;
        }
        let multiplier = -dot / qraux;
        values[0] += multiplier * qraux;
        for value in &mut values[1..] {
            *value += multiplier * scale;
        }
    };
    let mut effects = response.to_vec();
    reflect(&mut effects);
    let coefficient = effects[0] / -norm;
    let mut residuals = effects.clone();
    residuals[0] = 0.;
    reflect(&mut residuals);
    let fitted = response
        .iter()
        .zip(&residuals)
        .map(|(y, e)| y - e)
        .collect();
    let mut qr = vec![scale; response.len()];
    qr[0] = -norm;
    Some(Fit {
        coefficient,
        residuals,
        fitted,
        effects,
        qr,
        qraux,
    })
}

struct Builder<'a> {
    access: &'a RuntimeAccess,
    symbols: &'a Symbols,
}
enum Elements<'a> {
    Real(&'a [f64]),
    Integer(&'a [i32]),
    References(&'a [Value]),
}
impl Builder<'_> {
    fn strings(&self, strings: &[String]) -> Result<Value> {
        let text: Vec<_> = strings.iter().map(String::as_str).collect();
        checked(checked(self.access.allocator(&self.access.domain()))?.strings(&text))
    }
    fn attributes(&self, attributes: &[(Value, Value)]) -> Result<Value> {
        let domain = self.access.domain();
        let allocator = checked(self.access.allocator(&domain))?;
        let mut result = domain.nil();
        for (tag, value) in attributes.iter().rev() {
            result = checked(allocator.pairlist_cell(value, &result, tag))?;
        }
        Ok(result)
    }
    fn vector(
        &self,
        kind: SEXPTYPE,
        elements: Elements<'_>,
        attributes: &[(Value, Value)],
        object: bool,
    ) -> Result<Value> {
        let domain = self.access.domain();
        let references = match &elements {
            Elements::References(values) => values
                .iter()
                .map(|value| checked(domain.link(value)))
                .collect::<Result<Vec<_>>>()?,
            _ => Vec::new(),
        };
        let length = match &elements {
            Elements::Real(values) => values.len(),
            Elements::Integer(values) => values.len(),
            Elements::References(values) => values.len(),
        };
        let length = i64::try_from(length).map_err(|_| "too many model values")?;
        let attributes = self.attributes(attributes)?;
        let attributes_link = checked(domain.link(&attributes))?;
        let allocator = checked(self.access.allocator(&domain))?;
        let result = checked(allocator.allocate(|arena| {
            let pointer = arena.alloc_vector(kind, length);
            let node = arena.node_token(pointer)?;
            let heap = arena.heap_identity();
            if length > 0 {
                let payload = heap.payload_lease(&node)?;
                match elements {
                    Elements::Real(values) => {
                        for (index, value) in values.iter().copied().enumerate() {
                            payload.set_real_elt(index, value)?;
                        }
                    }
                    Elements::Integer(values) => {
                        for (index, value) in values.iter().copied().enumerate() {
                            payload.set_integer_elt(index, value)?;
                        }
                    }
                    Elements::References(_) => {
                        for (index, link) in references.iter().copied().enumerate() {
                            payload.set_reference_elt(index, link)?;
                        }
                    }
                }
            }
            let mut header = heap.node_snapshot(&node)?;
            header.attrib = attributes_link;
            header.sxpinfo.set_obj(object);
            heap.replace_node(&node, header)?;
            Some(pointer)
        }))?;
        active(self.access)?;
        Ok(result)
    }
    fn real(&self, values: &[f64], names: Option<&Value>) -> Result<Value> {
        let attributes: Vec<_> = names
            .into_iter()
            .map(|names| (self.symbols.names.clone(), names.clone()))
            .collect();
        self.vector(
            SEXPTYPE::REALSXP,
            Elements::Real(values),
            &attributes,
            false,
        )
    }
    fn integer(&self, values: &[i32]) -> Result<Value> {
        self.vector(SEXPTYPE::INTSXP, Elements::Integer(values), &[], false)
    }
    fn list(&self, fields: &[(&str, Value)], class: Option<&str>) -> Result<Value> {
        let names = self.strings(
            &fields
                .iter()
                .map(|(name, _)| (*name).to_string())
                .collect::<Vec<_>>(),
        )?;
        let values: Vec<_> = fields.iter().map(|(_, value)| value.clone()).collect();
        let mut attributes = vec![(self.symbols.names.clone(), names)];
        if let Some(class) = class {
            attributes.push((self.symbols.class.clone(), self.strings(&[class.into()])?));
        }
        self.vector(
            SEXPTYPE::VECSXP,
            Elements::References(&values),
            &attributes,
            class.is_some(),
        )
    }
    fn saved_call(&self, call: &Value) -> Result<Value> {
        if call.typeof_() != SEXPTYPE::LANGSXP {
            return Ok(call.clone());
        }
        let head = owned(checked(call.try_car())?)?;
        let mut cell = owned(checked(call.try_cdr())?)?;
        let mut arguments = Vec::new();
        let mut visited = std::collections::HashSet::new();
        while !cell.is_nil() {
            if !visited.insert(checked(cell.allocation())?.link()) {
                return Err("cyclic lm call".into());
            }
            let value = owned(checked(cell.try_car())?)?;
            let tag = if arguments.is_empty() {
                self.symbols.formula.clone()
            } else {
                owned(checked(cell.try_tag())?)?
            };
            arguments.push((value, tag));
            cell = owned(checked(cell.try_cdr())?)?;
        }
        let domain = self.access.domain();
        let allocator = checked(self.access.allocator(&domain))?;
        let mut tail = domain.nil();
        for (value, tag) in arguments.iter().rev() {
            tail = checked(allocator.pairlist_cell(value, &tail, tag))?;
        }
        checked(allocator.call(&head, &tail))
    }
}

pub(super) fn evaluate(
    access: &RuntimeAccess,
    request: &Request,
    response: Value,
    symbols: &Symbols,
) -> Result<Value> {
    active(access)?;
    let labels = request.labels(access, &response)?;
    let mut values = Vec::new();
    let mut retained_names = Vec::new();
    let mut omitted_names = Vec::new();
    let mut omitted = Vec::new();
    for (index, label) in labels.into_iter().enumerate() {
        let value = match response.typeof_() {
            SEXPTYPE::REALSXP => checked(response.try_real_elt(index as i64))?,
            SEXPTYPE::INTSXP => {
                let integer = checked(response.try_integer_elt(index as i64))?;
                if integer == crate::sexp::ffi::NA_INTEGER {
                    crate::sexp::ffi::NA_REAL
                } else {
                    f64::from(integer)
                }
            }
            SEXPTYPE::LGLSXP => {
                let logical = checked(response.try_logical_elt(index as i64))?;
                if logical == crate::sexp::ffi::NA_LOGICAL {
                    crate::sexp::ffi::NA_REAL
                } else {
                    f64::from(logical)
                }
            }
            _ => return Err("'y' must be numeric".into()),
        };
        active(access)?;
        if value.is_nan() {
            omitted.push(i32::try_from(index + 1).map_err(|_| "too many model cases")?);
            omitted_names.push(label);
        } else if !value.is_finite() {
            return Err("NA/NaN/Inf in 'y'".into());
        } else {
            values.push(value);
            retained_names.push(label);
        }
    }
    let fit = fit_column(&values).ok_or("0 (non-NA) cases")?;
    let n = i32::try_from(values.len()).map_err(|_| "too many model cases")?;
    let builder = Builder { access, symbols };
    let names = builder.strings(&retained_names)?;
    let coefficient_names = builder.strings(&["(Intercept)".into()])?;
    let coefficient = builder.real(&[fit.coefficient], Some(&coefficient_names))?;
    let residuals = builder.real(&fit.residuals, Some(&names))?;
    let fitted = builder.real(&fit.fitted, Some(&names))?;
    let mut effect_names = vec![String::new(); values.len()];
    effect_names[0] = "(Intercept)".into();
    let effect_names = builder.strings(&effect_names)?;
    let effects = builder.real(&fit.effects, Some(&effect_names))?;
    let assign = builder.integer(&[0])?;
    let dimensions = builder.integer(&[n, 1])?;
    let dimension_names = builder.vector(
        SEXPTYPE::VECSXP,
        Elements::References(&[names.clone(), coefficient_names]),
        &[],
        false,
    )?;
    let matrix = builder.vector(
        SEXPTYPE::REALSXP,
        Elements::Real(&fit.qr),
        &[
            (symbols.dim.clone(), dimensions),
            (symbols.dimnames.clone(), dimension_names),
            (symbols.assign.clone(), assign.clone()),
        ],
        false,
    )?;
    let rank = builder.integer(&[1])?;
    let qr = builder.list(
        &[
            ("qr", matrix),
            ("qraux", builder.real(&[fit.qraux], None)?),
            ("rank", rank.clone()),
            ("pivot", builder.integer(&[1])?),
        ],
        Some("qr"),
    )?;
    let df = builder.integer(&[n - 1])?;
    let sigma =
        (fit.residuals.iter().map(|value| value * value).sum::<f64>() / f64::from(n - 1)).sqrt();
    let sigma = builder.real(&[sigma], None)?;
    let hat = builder.real(&vec![1. / f64::from(n); values.len()], Some(&names))?;
    let saved_call = builder.saved_call(&request.call)?;
    let mut fields = vec![
        ("coefficients", coefficient),
        ("residuals", residuals),
        ("fitted.values", fitted),
        ("rank", rank),
        ("df.residual", df),
        ("sigma", sigma),
        ("hat", hat),
        ("call", saved_call),
        ("qr", qr),
        ("effects", effects),
        ("assign", assign),
    ];
    if !omitted.is_empty() {
        let names = builder.strings(&omitted_names)?;
        let class = builder.strings(&["omit".into()])?;
        let action = builder.vector(
            SEXPTYPE::INTSXP,
            Elements::Integer(&omitted),
            &[
                (symbols.names.clone(), names),
                (symbols.class.clone(), class),
            ],
            true,
        )?;
        fields.push(("na.action", action));
    }
    let model = builder.list(&fields, Some("lm"))?;
    active(access)?;
    Ok(model)
}
