#![forbid(unsafe_code)]
//! Built-in classes and checked compact-vector constructors.
use super::*;

pub(crate) fn builtin_sequence<'s>(
    owner: OwnerToken<'s>,
    kind: SEXPTYPE,
) -> SexpResult<AltrepClassHandle<'s>> {
    let name = match kind {
        SEXPTYPE::INTSXP => ".builtin.compact_intseq",
        SEXPTYPE::REALSXP => ".builtin.compact_realseq",
        _ => return Err(failure("sequence vector type")),
    };
    let symbol = CString::new(format!(".AltrepClass.{name}")).unwrap();
    let raw = storage::intern(owner, &symbol)?.as_raw();
    if lookup(owner, raw).is_some() {
        return class_handle(owner, raw);
    }
    register(owner, name, Rc::new(SequenceClass(kind)))
}

pub(crate) fn new_sequence<'s>(
    owner: OwnerToken<'s>,
    kind: SEXPTYPE,
    origin: f64,
    step: f64,
    length: i64,
) -> SexpResult<Sexp<'s>> {
    if length < 0 || length > (1_i64 << 52) {
        return Err(failure("invalid sequence length"));
    }
    if length <= 1 {
        let value = allocate(owner, kind, length)?;
        if length == 1 {
            let mut value = SexpMut::try_from_checked(value)?;
            match kind {
                SEXPTYPE::INTSXP => value.try_set_integer_elt(0, origin as i32)?,
                SEXPTYPE::REALSXP => value.try_set_real_elt(0, origin)?,
                _ => return Err(failure("sequence vector type")),
            }
            return Ok(value.freeze());
        }
        return Ok(value);
    }
    let class = builtin_sequence(owner, kind)?;
    let state = allocate(owner, SEXPTYPE::REALSXP, 3)?;
    let mut state = SexpMut::try_from_checked(state)?;
    state.try_set_real_elt(0, length as f64)?;
    state.try_set_real_elt(1, origin)?;
    state.try_set_real_elt(2, step)?;
    AltrepBuilder::new(class).data1(state.freeze()).build()
}

/// Formula in data1: GNU's `[length, origin, step]` real triple.
/// Both classes retain that state after expanding, independently of values.
pub struct SequenceClass(pub SEXPTYPE);
impl AltrepClass for SequenceClass {
    fn vector_type(&self) -> SEXPTYPE {
        self.0
    }
    fn cache_in_data2(&self) -> bool {
        true
    }
    fn length(&self, c: &AltrepContext<'_>) -> SexpResult<i64> {
        let state = c.data1()?;
        if state.len() != 3 {
            return Err(failure("sequence state must contain three scalars"));
        }
        let length = state.try_real_elt(0)?;
        if !length.is_finite()
            || length < 0.0
            || length > (1_u64 << 52) as f64
            || length.fract() != 0.0
        {
            return Err(failure("invalid sequence length"));
        }
        if self.0 == SEXPTYPE::INTSXP {
            let first = state.try_real_elt(1)?;
            let step = state.try_real_elt(2)?;
            let last = first + (length - 1.0).max(0.0) * step;
            if [first, step, last].iter().any(|v| {
                !v.is_finite() || v.fract() != 0.0 || *v < i32::MIN as f64 || *v > i32::MAX as f64
            }) {
                return Err(failure("integer sequence out of range"));
            }
        }
        Ok(length as i64)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        let state = c.data1()?;
        let value = state.try_real_elt(1)? + i as f64 * state.try_real_elt(2)?;
        match self.0 {
            SEXPTYPE::INTSXP
                if value.is_finite()
                    && value.fract() == 0.0
                    && value >= i32::MIN as f64
                    && value <= i32::MAX as f64 =>
            {
                Ok(AltrepElement::Integer(value as i32))
            }
            SEXPTYPE::REALSXP => Ok(AltrepElement::Real(value)),
            _ => Err(failure("invalid sequence element")),
        }
    }
}

/// Deferred evaluation with a traced result cache. data1 is a list containing
/// `[expression, environment, length]`; data2 starts at NULL. Evaluation failure
/// leaves the cache empty, so retry is possible. Cached results must have the
/// declared vector type and length.
pub struct DeferredClass(pub SEXPTYPE);
impl AltrepClass for DeferredClass {
    fn vector_type(&self) -> SEXPTYPE {
        self.0
    }
    fn length(&self, c: &AltrepContext<'_>) -> SexpResult<i64> {
        let len = c.data1()?.try_vector_elt(2)?.try_real_elt(0)?;
        if !len.is_finite() || len < 0.0 || len > (1_u64 << 52) as f64 || len.fract() != 0.0 {
            return Err(failure("invalid deferred vector length"));
        }
        Ok(len as i64)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        let cached = c.data2()?;
        let cached = if cached.typeof_() == SEXPTYPE::NILSXP {
            let data = c.data1()?;
            let result = c.eval(data.try_vector_elt(0)?, data.try_vector_elt(1)?)?;
            if result.typeof_() != self.0 || result.len() != c.object().len() {
                return Err(failure("deferred result type or length mismatch"));
            }
            c.set_data2(result.clone())?;
            result
        } else {
            cached
        };
        if cached.typeof_() != self.0 || cached.len() != c.object().len() {
            return Err(failure("invalid deferred result cache"));
        }
        dense_element(&cached, i)
    }
}

impl RSession {
    /// Safe built-in compact vectors using the production formula representation.
    pub fn compact_integer_sequence(
        &self,
        origin: i32,
        step: i32,
        length: usize,
    ) -> SexpResult<Sexp<'_>> {
        if length > 0 {
            let last = i128::from(origin) + (length - 1) as i128 * i128::from(step);
            if last < i32::MIN as i128 || last > i32::MAX as i128 {
                return Err(failure("integer sequence out of range"));
            }
        }
        let owner = self.owner_token().ok_or(SexpError::OwnerNotActive)?;
        bridge::compact_integer_sequence(owner, origin, step, length)
    }
    pub fn compact_real_sequence(
        &self,
        origin: f64,
        step: f64,
        length: usize,
    ) -> SexpResult<Sexp<'_>> {
        let owner = self.owner_token().ok_or(SexpError::OwnerNotActive)?;
        bridge::compact_real_sequence(owner, origin, step, length)
    }
}

/// Rust built-in repeated atomic or list elements. Data1 is the scalar source,
/// data2 an integer/real length. GC traces both independently of the payload.
pub struct RepeatClass(pub SEXPTYPE);
impl AltrepClass for RepeatClass {
    fn vector_type(&self) -> SEXPTYPE {
        self.0
    }
    fn length(&self, c: &AltrepContext<'_>) -> SexpResult<R_xlen_t> {
        let scalar = c.data1()?;
        if scalar.len() != 1 || scalar.typeof_() != self.0 {
            return Err(failure("ALTREP repeat scalar"));
        }
        let len = c.data2()?.try_real_elt(0)?;
        if !len.is_finite() || len < 0.0 || len.fract() != 0.0 || len >= i64::MAX as f64 {
            return Err(failure("ALTREP repeat length"));
        }
        Ok(len as i64)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, _: R_xlen_t) -> SexpResult<AltrepElement<'s>> {
        dense_element(&c.data1()?, 0)
    }
}
