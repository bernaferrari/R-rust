//! Owned buffers and checked admission for the bundled C/Fortran kernels.
//!
//! All pointers are derived only after interface, arity, element types and
//! kernel-specific buffer bounds have been checked. Distinct Rust vectors keep
//! aliased R arguments independent. Foreign loader addresses never enter here.

use std::ffi::{c_char, c_void};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BufferInterface {
    C,
    Fortran,
}

impl std::fmt::Display for BufferInterface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::C => ".C",
            Self::Fortran => ".Fortran",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BufferType {
    Integer,
    Real,
    Character,
}

#[derive(Debug)]
pub(crate) enum NativeBuffer {
    Integer(Vec<i32>),
    Real(Vec<f64>),
    /// Each byte vector owns its terminating NUL, and may be mutated only
    /// through this buffer's invocation. No SEXP storage is exposed to kernels.
    Character(Vec<Vec<u8>>),
}

impl NativeBuffer {
    pub(crate) fn kind(&self) -> BufferType {
        match self {
            Self::Integer(_) => BufferType::Integer,
            Self::Real(_) => BufferType::Real,
            Self::Character(_) => BufferType::Character,
        }
    }
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Integer(x) => x.len(),
            Self::Real(x) => x.len(),
            Self::Character(x) => x.len(),
        }
    }
    pub(crate) fn integers(&self) -> Result<&[i32], BufferError> {
        match self {
            Self::Integer(x) => Ok(x),
            _ => Err(BufferError::new("expected integer buffer")),
        }
    }
    pub(crate) fn reals(&self) -> Result<&[f64], BufferError> {
        match self {
            Self::Real(x) => Ok(x),
            _ => Err(BufferError::new("expected double buffer")),
        }
    }
    fn pointer(&mut self, string_arrays: &mut Vec<Vec<*mut c_char>>) -> *mut c_void {
        match self {
            Self::Integer(x) => x.as_mut_ptr().cast(),
            Self::Real(x) => x.as_mut_ptr().cast(),
            Self::Character(x) => {
                string_arrays.push(x.iter_mut().map(|s| s.as_mut_ptr().cast()).collect());
                string_arrays
                    .last_mut()
                    .expect("just inserted")
                    .as_mut_ptr()
                    .cast()
            }
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BufferError(String);
impl BufferError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
impl std::fmt::Display for BufferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

macro_rules! void_kernels {
    ($($variant:ident($n:literal; $($arg:ident),*)),* $(,)?) => {
        #[derive(Clone, Copy)]
        pub(crate) enum VoidKernel {
            $($variant(unsafe extern "C-unwind" fn($(void_kernels!(@type $arg)),*))),*
        }
        impl VoidKernel {
            fn arity(self) -> usize { match self { $(Self::$variant(_) => $n),* } }
            unsafe fn invoke(self, pointers: &[*mut c_void]) {
                match self { $(Self::$variant(function) => {
                    let [$($arg),*] = pointers else { unreachable!("admitted exact kernel arity") };
                    unsafe { function($(*$arg),*) };
                }),* }
            }
        }
    };
    (@type $arg:ident) => { *mut c_void };
}
void_kernels! {
    Args1(1; a), Args2(2; a,b), Args4(4; a,b,c,d), Args6(6; a,b,c,d,e,f),
    Args7(7; a,b,c,d,e,f,g), Args9(9; a,b,c,d,e,f,g,h,i),
    Args10(10; a,b,c,d,e,f,g,h,i,j), Args17(17; a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p,q),
    Args20(20; a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p,q,r,s,t),
}

macro_rules! numeric_kernels {
    ($($variant:ident($($arg:ident:$kind:ident),*)),* $(,)?) => {
        #[derive(Clone, Copy)]
        pub(crate) enum LoessKernel {
            $($variant(unsafe extern "C-unwind" fn($(numeric_kernels!(@type $kind)),*))),*
        }
        impl LoessKernel {
            fn types(self) -> &'static [BufferType] {
                match self { $(Self::$variant(_) => &[$(numeric_kernels!(@kind $kind)),*]),* }
            }
            unsafe fn invoke(self, pointers: &[*mut c_void]) {
                match self { $(Self::$variant(function) => {
                    let [$($arg),*] = pointers else { unreachable!("admitted exact kernel arity") };
                    unsafe { function($((*$arg).cast()),*) };
                }),* }
            }
        }
    };
    (@type I) => { *mut i32 }; (@kind I) => { BufferType::Integer };
    (@type R) => { *mut f64 }; (@kind R) => { BufferType::Real };
    (@type C) => { *mut *mut c_char }; (@kind C) => { BufferType::Character };
}
numeric_kernels! {
    Raw(a:R,b:R,c:R,d:R,e:I,f:I,g:R,h:I,i:I,j:I,k:I,l:R,m:C,n:R,o:I,p:I,q:R,r:R,s:R,t:R,u:R,v:R,w:R,x:I),
    Dfit(a:R,b:R,c:R,d:R,e:R,f:I,g:I,h:I,i:I,j:I,k:I,l:I,m:R),
    Ifit(a:I,b:I,c:R,d:R,e:R,f:I,g:R,h:R),
    Ise(a:R,b:R,c:R,d:R,e:R,f:I,g:I,h:I,i:I,j:R,k:I,l:I,m:I,n:R,o:R),
    Dfitse(a:R,b:R,c:R,d:R,e:R,f:I,g:R,h:I,i:I,j:I,k:I,l:I,m:I,n:I,o:R,p:R),
}

#[derive(Clone, Copy)]
enum Kernel {
    Void(VoidKernel),
    Numeric(LoessKernel),
    Owned(fn(&mut [NativeBuffer]) -> Result<(), BufferError>),
}

#[derive(Clone, Copy)]
pub(crate) struct BufferRoutine {
    package: &'static str,
    interface: BufferInterface,
    types: &'static [BufferType],
    shape: fn(&[NativeBuffer]) -> Result<(), BufferError>,
    kernel: Kernel,
}

impl BufferRoutine {
    /// # Safety
    /// `types` describes the void-pointer kernel's real element types; `shape`
    /// proves every dereference/index/workspace bound and scalar constraint.
    pub(crate) const unsafe fn void(
        package: &'static str,
        interface: BufferInterface,
        types: &'static [BufferType],
        shape: fn(&[NativeBuffer]) -> Result<(), BufferError>,
        kernel: VoidKernel,
    ) -> Self {
        Self {
            package,
            interface,
            types,
            shape,
            kernel: Kernel::Void(kernel),
        }
    }
    /// # Safety
    /// `shape` proves every scalar/array/workspace precondition of the actual
    /// typed kernel. Element types and pointer count are derived from its ABI.
    pub(crate) unsafe fn numeric(
        package: &'static str,
        interface: BufferInterface,
        shape: fn(&[NativeBuffer]) -> Result<(), BufferError>,
        kernel: LoessKernel,
    ) -> Self {
        Self {
            package,
            interface,
            types: kernel.types(),
            shape,
            kernel: Kernel::Numeric(kernel),
        }
    }
    pub(crate) const fn owned(
        package: &'static str,
        interface: BufferInterface,
        types: &'static [BufferType],
        shape: fn(&[NativeBuffer]) -> Result<(), BufferError>,
        kernel: fn(&mut [NativeBuffer]) -> Result<(), BufferError>,
    ) -> Self {
        Self {
            package,
            interface,
            types,
            shape,
            kernel: Kernel::Owned(kernel),
        }
    }

    pub(crate) fn package(self) -> &'static str {
        self.package
    }
    pub(crate) fn interface(self) -> BufferInterface {
        self.interface
    }
    pub(crate) fn types(self) -> &'static [BufferType] {
        self.types
    }
    pub(crate) fn arity(self) -> usize {
        match self.kernel {
            Kernel::Void(k) => k.arity(),
            Kernel::Numeric(k) => k.types().len(),
            Kernel::Owned(_) => self.types.len(),
        }
    }
    pub(crate) fn validate_request(
        self,
        interface: BufferInterface,
        count: usize,
    ) -> Result<(), BufferError> {
        if interface != self.interface {
            return Err(BufferError::new(format!(
                "{} routine called through {interface}",
                self.interface
            )));
        }
        if count != self.arity() {
            return Err(BufferError::new(format!(
                "incorrect number of arguments: expected {}, received {count}",
                self.arity()
            )));
        }
        if self.types.len() != self.arity() {
            return Err(BufferError::new("inconsistent bundled native descriptor"));
        }
        Ok(())
    }
    pub(crate) fn validate_buffers(
        self,
        interface: BufferInterface,
        buffers: &[NativeBuffer],
    ) -> Result<(), BufferError> {
        self.validate_request(interface, buffers.len())?;
        for (i, (buffer, kind)) in buffers.iter().zip(self.types).enumerate() {
            if buffer.kind() != *kind {
                return Err(BufferError::new(format!(
                    "argument {} must have type {kind:?}",
                    i + 1
                )));
            }
            if let NativeBuffer::Character(strings) = buffer {
                if strings
                    .iter()
                    .any(|s| s.last() != Some(&0) || s[..s.len() - 1].contains(&0))
                {
                    return Err(BufferError::new("invalid native character buffer"));
                }
            }
        }
        (self.shape)(buffers)
    }
    pub(crate) fn invoke(
        self,
        interface: BufferInterface,
        buffers: &mut [NativeBuffer],
    ) -> Result<(), BufferError> {
        self.validate_buffers(interface, buffers)?;
        #[cfg(test)]
        INVOCATIONS.with(|count| count.set(count.get() + 1));
        if let Kernel::Owned(function) = self.kernel {
            return function(buffers);
        }
        let mut string_arrays = Vec::new();
        let pointers: Vec<_> = buffers
            .iter_mut()
            .map(|x| x.pointer(&mut string_arrays))
            .collect();
        // SAFETY: exact ABI is retained, buffers are independently owned and
        // live, and the authoritative descriptor just proved all access bounds.
        unsafe {
            match self.kernel {
                Kernel::Void(k) => k.invoke(&pointers),
                Kernel::Numeric(k) => k.invoke(&pointers),
                Kernel::Owned(_) => unreachable!("owned kernels dispatched without pointers"),
            }
        }
        Ok(())
    }
}

fn minimum(buffers: &[NativeBuffer], index: usize, length: usize) -> Result<(), BufferError> {
    if buffers[index].len() < length {
        return Err(BufferError::new(format!(
            "argument {} needs at least {length} elements, received {}",
            index + 1,
            buffers[index].len()
        )));
    }
    Ok(())
}
fn scalar(buffers: &[NativeBuffer], index: usize) -> Result<i32, BufferError> {
    minimum(buffers, index, 1)?;
    Ok(buffers[index].integers()?[0])
}
fn dimension(buffers: &[NativeBuffer], index: usize, lower: i32) -> Result<usize, BufferError> {
    let x = scalar(buffers, index)?;
    if x < lower {
        return Err(BufferError::new(format!(
            "argument {} dimension must be at least {lower}",
            index + 1
        )));
    }
    Ok(x as usize)
}
fn product(values: &[usize]) -> Result<usize, BufferError> {
    values.iter().try_fold(1usize, |n, v| {
        n.checked_mul(*v)
            .filter(|n| i32::try_from(*n).is_ok())
            .ok_or_else(|| BufferError::new("native dimension product overflow"))
    })
}
fn scalar_real(buffers: &[NativeBuffer], index: usize) -> Result<f64, BufferError> {
    minimum(buffers, index, 1)?;
    Ok(buffers[index].reals()?[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! { static CALLS: Cell<usize> = const { Cell::new(0) }; }
    unsafe extern "C-unwind" fn increment(value: *mut c_void) {
        CALLS.with(|n| n.set(n.get() + 1));
        // Test descriptor requires one independently owned integer element.
        unsafe {
            *value.cast::<i32>() += 1;
        }
    }
    fn one_integer(buffers: &[NativeBuffer]) -> Result<(), BufferError> {
        minimum(buffers, 0, 1)
    }
    fn routine() -> BufferRoutine {
        // The exact pointer type, element type and one-element access agree.
        unsafe {
            BufferRoutine::void(
                "test",
                BufferInterface::C,
                &[BufferType::Integer],
                one_integer,
                VoidKernel::Args1(increment),
            )
        }
    }

    #[test]
    fn typed_buffer_admission_rejects_interface_and_count_before_invocation() {
        CALLS.with(|n| n.set(0));
        let descriptor = routine();
        let mut empty = [];
        assert!(
            descriptor
                .invoke(BufferInterface::Fortran, &mut empty)
                .unwrap_err()
                .to_string()
                .contains(".C routine called through .Fortran")
        );
        assert!(
            descriptor
                .invoke(BufferInterface::C, &mut empty)
                .unwrap_err()
                .to_string()
                .contains("expected 1, received 0")
        );
        let mut too_many = [
            NativeBuffer::Integer(vec![1]),
            NativeBuffer::Integer(vec![2]),
        ];
        assert!(
            descriptor
                .invoke(BufferInterface::C, &mut too_many)
                .is_err()
        );
        assert_eq!(CALLS.with(Cell::get), 0);
    }

    #[test]
    fn typed_buffer_admission_rejects_wrong_type_and_short_buffer_before_invocation() {
        CALLS.with(|n| n.set(0));
        let descriptor = routine();
        assert!(
            descriptor
                .invoke(BufferInterface::C, &mut [NativeBuffer::Real(vec![1.0])])
                .is_err()
        );
        assert!(
            descriptor
                .invoke(BufferInterface::C, &mut [NativeBuffer::Integer(Vec::new())])
                .is_err()
        );
        assert_eq!(CALLS.with(Cell::get), 0);
        let mut valid = [NativeBuffer::Integer(vec![7])];
        descriptor.invoke(BufferInterface::C, &mut valid).unwrap();
        assert_eq!(valid[0].integers().unwrap(), &[8]);
        assert_eq!(CALLS.with(Cell::get), 1);
    }
}
fn finite(buffers: &[NativeBuffer], index: usize) -> Result<(), BufferError> {
    if buffers[index].reals()?.iter().any(|x| !x.is_finite()) {
        return Err(BufferError::new(format!(
            "argument {} needs finite values for this native kernel",
            index + 1
        )));
    }
    Ok(())
}
pub(crate) fn renctest(b: &[NativeBuffer]) -> Result<(), BufferError> {
    minimum(b, 0, 1)
}

pub(crate) fn kmeans(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let n = dimension(b, 1, 0)?;
    let p = dimension(b, 2, 1)?;
    let k = dimension(b, 4, 1)?;
    minimum(b, 0, product(&[n, p])?)?;
    minimum(b, 3, product(&[k, p])?)?;
    minimum(b, 5, n)?;
    dimension(b, 6, 1)?;
    minimum(b, 7, k)?;
    minimum(b, 8, k)?;
    finite(b, 0)?;
    finite(b, 3)?;
    Ok(())
}
pub(crate) fn kmns(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let n = dimension(b, 1, 0)?;
    let p = dimension(b, 2, 1)?;
    let k = dimension(b, 4, 1)?;
    minimum(b, 0, product(&[n, p])?)?;
    minimum(b, 3, product(&[k, p])?)?;
    for i in [5, 6, 11] {
        minimum(b, i, n)?;
    }
    for i in [7, 8, 9, 10, 13, 15] {
        minimum(b, i, k)?;
    }
    minimum(
        b,
        12,
        k.checked_add(1)
            .filter(|x| i32::try_from(*x).is_ok())
            .ok_or_else(|| BufferError::new("native workspace dimension overflow"))?,
    )?;
    dimension(b, 14, 0)?;
    minimum(b, 16, 1)?;
    finite(b, 0)?;
    finite(b, 3)?;
    Ok(())
}
pub(crate) fn eureka(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let n = dimension(b, 0, 0)?;
    if n == 0 {
        return Ok(());
    }
    let plus = n
        .checked_add(1)
        .filter(|x| i32::try_from(*x).is_ok())
        .ok_or_else(|| BufferError::new("native correlation dimension overflow"))?;
    minimum(b, 1, plus)?;
    minimum(b, 2, plus)?;
    minimum(b, 3, product(&[n, n])?)?;
    minimum(b, 4, n)?;
    minimum(b, 5, n)?;
    Ok(())
}
pub(crate) fn multi_yw(b: &[NativeBuffer]) -> Result<(), BufferError> {
    dimension(b, 1, 1)?;
    let order_max = dimension(b, 2, 0)?;
    let series = dimension(b, 3, 1)?;
    let lags = order_max
        .checked_add(1)
        .filter(|x| i32::try_from(*x).is_ok())
        .ok_or_else(|| BufferError::new("native lag dimension overflow"))?;
    let total = product(&[lags, series, series])?;
    for i in [0, 4, 5, 6] {
        minimum(b, i, total)?;
    }
    minimum(b, 7, lags)?;
    if dimension(b, 8, 0)? > order_max {
        return Err(BufferError::new("requested order exceeds maximum order"));
    }
    minimum(b, 9, 1)?;
    Ok(())
}
pub(crate) fn hclust(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let n = dimension(b, 0, 2)?;
    let len = dimension(b, 1, 1)?;
    if !(1..=8).contains(&scalar(b, 2)?) {
        return Err(BufferError::new("invalid hierarchical clustering method"));
    }
    let distances = n
        .checked_mul(n - 1)
        .map(|x| x / 2)
        .filter(|x| i32::try_from(*x).is_ok())
        .ok_or_else(|| BufferError::new("native triangular dimension overflow"))?;
    if len < distances {
        return Err(BufferError::new(
            "distance dimension is shorter than n*(n-1)/2",
        ));
    }
    for i in 3..=8 {
        minimum(b, i, n)?;
    }
    minimum(b, 9, len)?;
    finite(b, 6)?;
    finite(b, 9)?;
    if b[6].reals()?.iter().take(n).any(|x| *x <= 0.0) {
        return Err(BufferError::new("cluster sizes must be positive"));
    }
    Ok(())
}
pub(crate) fn hcass2(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let n = dimension(b, 0, 2)?;
    for i in 1..=5 {
        minimum(b, i, n)?;
    }
    // The native body negates the merge IDs. Exclude the one overflowing i32.
    for i in [1, 2] {
        if b[i].integers()?.iter().take(n).any(|x| *x == i32::MIN) {
            return Err(BufferError::new("invalid merge identifier"));
        }
    }
    Ok(())
}
fn knots(b: &[NativeBuffer], index: usize, nk: usize) -> Result<(), BufferError> {
    let len = nk
        .checked_add(4)
        .filter(|x| i32::try_from(*x).is_ok())
        .ok_or_else(|| BufferError::new("native knot dimension overflow"))?;
    minimum(b, index, len)?;
    let x = &b[index].reals()?[..len];
    if x.iter().any(|v| !v.is_finite())
        || x.windows(2).any(|v| v[0] > v[1])
        || x[..4].iter().any(|v| *v != x[0])
        || x[nk..].iter().any(|v| *v != x[nk])
        || x[3] >= x[nk]
    {
        return Err(BufferError::new(
            "cubic knots need finite ordered values and clamped endpoints",
        ));
    }
    Ok(())
}
pub(crate) fn bvalus(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let n = dimension(b, 0, 0)?;
    let nk = dimension(b, 3, 4)?;
    knots(b, 1, nk)?;
    minimum(b, 2, nk)?;
    minimum(b, 4, n)?;
    minimum(b, 5, n)?;
    dimension(b, 6, 0)?;
    finite(b, 4)?;
    Ok(())
}
pub(crate) fn rbart(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let n = dimension(b, 6, 1)?;
    let nk = dimension(b, 8, 4)?;
    let ld4 = dimension(b, 17, 4)?;
    let ldnk = dimension(b, 18, 1)?;
    for i in [0, 1, 5, 12, 14, 19] {
        minimum(b, i, 1)?;
    }
    for i in [2, 3, 4, 10, 11] {
        minimum(b, i, n)?;
    }
    knots(b, 7, nk)?;
    minimum(b, 9, nk)?;
    minimum(b, 13, 4)?;
    minimum(b, 15, 5)?;
    let stride = 9usize
        .checked_add(product(&[2, ld4])?)
        .and_then(|x| x.checked_add(ldnk))
        .ok_or_else(|| BufferError::new("native spline workspace overflow"))?;
    minimum(b, 16, product(&[stride, nk])?)?;
    finite(b, 2)?;
    let x = &b[2].reals()?[..n];
    let k = b[7].reals()?;
    if x.iter().any(|v| *v < k[3] || *v > k[nk]) {
        return Err(BufferError::new(
            "spline observations outside the supported knot interval",
        ));
    }
    Ok(())
}
pub(crate) fn lowesw(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let n = dimension(b, 1, 0)?;
    for i in [0, 2, 3] {
        minimum(b, i, n)?;
    }
    Ok(())
}
pub(crate) fn lowesp(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let n = dimension(b, 0, 0)?;
    for i in 1..=6 {
        minimum(b, i, n)?;
    }
    Ok(())
}
fn loess_fit(
    b: &[NativeBuffer],
    d_index: usize,
    n_index: usize,
    span_index: usize,
    degree_index: usize,
    np_index: usize,
    drop_index: usize,
) -> Result<(usize, usize), BufferError> {
    let d = dimension(b, d_index, 1)?;
    let n = dimension(b, n_index, 1)?;
    let span = scalar_real(b, span_index)?;
    if !span.is_finite() || span <= 0.0 {
        return Err(BufferError::new("LOESS span must be finite and positive"));
    }
    let degree = scalar(b, degree_index)?;
    if !(0..=2).contains(&degree) {
        return Err(BufferError::new(
            "LOESS degree must be between zero and two",
        ));
    }
    let np = dimension(b, np_index, 0)?;
    if np > d {
        return Err(BufferError::new(
            "LOESS nonparametric count exceeds predictor count",
        ));
    }
    minimum(b, drop_index, d)?;
    Ok((d, n))
}
pub(crate) fn loess_raw(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let (d, n) = loess_fit(b, 4, 5, 6, 7, 8, 9)?;
    minimum(b, 0, n)?;
    minimum(b, 1, product(&[n, d])?)?;
    minimum(b, 2, n)?;
    minimum(b, 3, n)?;
    for i in [10, 11, 12, 20, 21, 22, 23] {
        minimum(b, i, 1)?;
    }
    minimum(b, 13, n)?;
    minimum(b, 14, 7)?;
    Ok(())
}
pub(crate) fn loess_dfit(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let (d, n) = loess_fit(b, 9, 10, 4, 5, 6, 7)?;
    let m = dimension(b, 11, 0)?;
    minimum(b, 0, n)?;
    minimum(b, 1, product(&[n, d])?)?;
    minimum(b, 2, product(&[m, d])?)?;
    minimum(b, 3, n)?;
    minimum(b, 8, 1)?;
    minimum(b, 12, m)?;
    Ok(())
}
pub(crate) fn loess_ise(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let (d, n) = loess_fit(b, 10, 11, 4, 5, 6, 7)?;
    let m = dimension(b, 12, 0)?;
    minimum(b, 0, n)?;
    minimum(b, 1, product(&[n, d])?)?;
    minimum(b, 2, product(&[m, d])?)?;
    minimum(b, 3, n)?;
    minimum(b, 8, 1)?;
    minimum(b, 9, 1)?;
    minimum(b, 13, m)?;
    minimum(b, 14, product(&[m, n])?)?;
    Ok(())
}
pub(crate) fn loess_dfitse(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let (d, n) = loess_fit(b, 11, 12, 6, 7, 8, 9)?;
    let m = dimension(b, 13, 0)?;
    minimum(b, 0, n)?;
    minimum(b, 1, product(&[n, d])?)?;
    minimum(b, 2, product(&[m, d])?)?;
    minimum(b, 3, n)?;
    minimum(b, 4, n)?;
    minimum(b, 5, 1)?;
    minimum(b, 10, 1)?;
    minimum(b, 14, m)?;
    minimum(b, 15, product(&[m, n])?)?;
    Ok(())
}
pub(crate) fn loess_ifit(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let m = dimension(b, 5, 0)?;
    // The actual kernel derives width from its retained model, not parameter.
    let width =
        crate::library::stats::loessc::checked_predictor_width().map_err(BufferError::new)?;
    minimum(b, 6, product(&[m, width])?)?;
    minimum(b, 7, m)?;
    Ok(())
}

#[cfg(test)]
thread_local! { static INVOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(crate) fn invocation_count() -> usize {
    INVOCATIONS.with(std::cell::Cell::get)
}

pub(crate) mod holtwinters;

/// Port-only base dtrco compatibility, entirely over owned checked buffers.
pub(crate) fn dtrco_shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let ldt = dimension(b, 1, 1)?;
    let n = dimension(b, 2, 0)?;
    if ldt < n {
        return Err(BufferError::new(
            "dtrco leading dimension smaller than matrix",
        ));
    }
    minimum(b, 0, product(&[ldt, n])?)?;
    minimum(b, 3, 1)?;
    minimum(b, 4, n)?;
    scalar(b, 5)?;
    finite(b, 0)
}
pub(crate) fn dtrco_owned(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    let n = scalar(b, 2)? as usize;
    let ldt = scalar(b, 1)? as usize;
    let lower = scalar(b, 5)? == 0;
    let t = b[0].reals()?;
    let at = |i: usize, j: usize| t[j * ldt + i];
    let mut work = vec![0.0f64; n.max(1)];
    let mut tnorm: f64 = 0.0;
    for j in 0..n {
        let l = if lower { n - j } else { j + 1 };
        let i1 = if lower { j } else { 0 };
        let mut s: f64 = 0.0;
        for i in 0..l {
            s += at(i1 + i, j).abs();
        }
        tnorm = tnorm.max(s);
    }
    let mut ek: f64 = 1.0;
    for kk in 0..n {
        let k = if lower { n - 1 - kk } else { kk };
        if work[k] != 0.0 {
            ek = ek.copysign(-work[k]);
        }
        if (ek - work[k]).abs() > at(k, k).abs() {
            let s = at(k, k).abs() / (ek - work[k]).abs();
            for v in &mut work {
                *v *= s;
            }
            ek *= s;
        }
        let mut wk = ek - work[k];
        let mut wkm = -ek - work[k];
        let mut s = wk.abs();
        let mut sm = wkm.abs();
        let diag = at(k, k);
        if diag != 0.0 {
            wk /= diag;
            wkm /= diag;
        } else {
            wk = 1.0;
            wkm = 1.0;
        }
        if kk + 1 != n {
            let (j1, j2) = if lower { (0, k) } else { (k + 1, n) };
            for j in j1..j2 {
                sm += (work[j] + wkm * at(k, j)).abs();
                work[j] += wk * at(k, j);
                s += work[j].abs();
            }
            if s < sm {
                let w = wkm - wk;
                wk = wkm;
                for j in j1..j2 {
                    work[j] += w * at(k, j);
                }
            }
        }
        work[k] = wk;
    }
    let mut asum = work.iter().map(|v| v.abs()).sum::<f64>();
    if asum != 0.0 {
        let s = 1.0 / asum;
        for v in &mut work {
            *v *= s;
        }
    }
    let mut ynorm = 1.0;
    for kk in 0..n {
        let k = if lower { kk } else { n - 1 - kk };
        if work[k].abs() > at(k, k).abs() && at(k, k) != 0.0 {
            let s = at(k, k).abs() / work[k].abs();
            for v in &mut work {
                *v *= s;
            }
            ynorm *= s;
        }
        if at(k, k) != 0.0 {
            work[k] /= at(k, k);
        } else {
            work[k] = 1.0;
        }
        if kk + 1 < n {
            let w = -work[k];
            let i1 = if lower { k + 1 } else { 0 };
            let count = n - kk - 1;
            for i in 0..count {
                work[i1 + i] += w * at(i1 + i, k);
            }
        }
    }
    asum = work.iter().map(|v| v.abs()).sum::<f64>();
    if asum != 0.0 {
        let s = 1.0 / asum;
        ynorm *= s;
    }
    let rcond = if tnorm != 0.0 { ynorm / tnorm } else { 0.0 };

    let NativeBuffer::Real(output) = &mut b[3] else {
        unreachable!("validated real result")
    };
    output[0] = rcond;
    let NativeBuffer::Real(output) = &mut b[4] else {
        unreachable!("validated real workspace")
    };
    output[..n].copy_from_slice(&work[..n]);
    Ok(())
}

#[cfg(test)]
mod registration_tests {
    use super::*;
    #[test]
    fn typed_buffer_registry_matches_independent_pinned_interfaces_counts_and_rejects_empty_storage()
     {
        // GNU R r90451 registration query, independent of port lookup arms.
        // Exact type metadata was not published by GNU; types here derive from
        // actual Rust declarations. This is coverage evidence, not full parity.
        const REGISTERED: &str = r"stats	.C	loess_raw	24
stats	.C	loess_dfit	13
stats	.C	loess_dfitse	16
stats	.C	loess_ifit	8
stats	.C	loess_ise	15
stats	.C	multi_burg	11
stats	.C	multi_yw	10
stats	.C	HoltWinters	17
stats	.C	kmeans_Lloyd	9
stats	.C	kmeans_MacQueen	9
stats	.C	rcont2	8
stats	.Fortran	lowesw	4
stats	.Fortran	lowesp	7
stats	.Fortran	setppr	6
stats	.Fortran	smart	16
stats	.Fortran	pppred	5
stats	.Fortran	setsmu	1
stats	.Fortran	rbart	20
stats	.Fortran	bvalus	7
stats	.Fortran	supsmu	10
stats	.Fortran	hclust	10
stats	.Fortran	hcass2	6
stats	.Fortran	kmns	17
stats	.Fortran	eureka	6
stats	.Fortran	stl	17
tools	.C	Renctest	1";
        assert_eq!(REGISTERED.lines().count(), 26);
        let before = invocation_count();
        let mut covered = 0;
        for row in REGISTERED.lines() {
            let fields: Vec<_> = row.split('\t').collect();
            let Some(routine) = crate::library::tools::native_calls::lookup_buffer(fields[2])
            else {
                continue;
            };
            let interface = if fields[1] == ".C" {
                BufferInterface::C
            } else {
                BufferInterface::Fortran
            };
            assert_eq!(routine.package(), fields[0]);
            assert_eq!(routine.interface(), interface);
            assert_eq!(routine.arity(), fields[3].parse::<usize>().unwrap());
            let wrong = if interface == BufferInterface::C {
                BufferInterface::Fortran
            } else {
                BufferInterface::C
            };
            assert!(routine.validate_request(wrong, routine.arity()).is_err());
            assert!(
                routine
                    .validate_request(interface, routine.arity() - 1)
                    .is_err()
            );
            assert!(
                routine
                    .validate_request(interface, routine.arity() + 1)
                    .is_err()
            );
            let mut empty: Vec<_> = routine
                .types()
                .iter()
                .map(|kind| match kind {
                    BufferType::Integer => NativeBuffer::Integer(vec![]),
                    BufferType::Real => NativeBuffer::Real(vec![]),
                    BufferType::Character => NativeBuffer::Character(vec![]),
                })
                .collect();
            assert!(
                routine.invoke(interface, &mut empty).is_err(),
                "{} cannot enter a kernel with missing scalar/workspace storage",
                fields[2]
            );
            covered += 1;
        }
        assert_eq!(
            covered, 18,
            "eight unsupported registrations are explicit inventory gaps"
        );
        assert_eq!(
            invocation_count(),
            before,
            "all registered invalid shapes reject before leaf invocation"
        );
    }
}

#[cfg(test)]
mod holtwinters_tests;
