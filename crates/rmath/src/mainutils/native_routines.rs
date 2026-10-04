//! Exact native signatures for the bundled Rust implementations.
//!
//! Registration retains the callable type. Admission checks the requested R
//! interface and payload arity before dispatch; dispatch never reconstructs a
//! callable signature from an erased address or from the caller's argument count.
//! Foreign libraries remain a separate, explicitly unsafe ABI boundary.

use crate::sexp::ffi::SEXP;

pub(crate) mod buffers;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeInterface {
    Call,
    External,
    External2,
}

impl std::fmt::Display for NativeInterface {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Call => ".Call",
            Self::External => ".External",
            Self::External2 => ".External2",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeAdmissionError {
    InterfaceMismatch {
        registered: NativeInterface,
        requested: NativeInterface,
    },
    ArgumentCount {
        expected: usize,
        actual: usize,
    },
}

impl std::fmt::Display for NativeAdmissionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InterfaceMismatch {
                registered,
                requested,
            } => {
                write!(formatter, "{registered} routine called through {requested}")
            }
            Self::ArgumentCount { expected, actual } => write!(
                formatter,
                "incorrect number of arguments: expected {expected}, received {actual}"
            ),
        }
    }
}

macro_rules! call_routines {
    ($($variant:ident($arity:literal; $($argument:ident),*)),* $(,)?) => {
        #[derive(Clone, Copy)]
        pub(crate) enum CallRoutine {
            $($variant(unsafe extern "C-unwind" fn($(call_routines!(@type $argument)),*) -> SEXP)),*
        }

        impl CallRoutine {
            pub(crate) fn arity(self) -> usize {
                match self { $(Self::$variant(_) => $arity),* }
            }

            /// The caller retains every payload allocation and the original
            /// runtime across invocation. Callback code may evaluate or collect.
            unsafe fn invoke(self, arguments: &[SEXP]) -> SEXP {
                // NativeRoutine::invoke_call checks this before any invocation.
                debug_assert_eq!(arguments.len(), self.arity());
                match self {
                    $(Self::$variant(function) => {
                        let [$($argument),*] = arguments else {
                            unreachable!("admitted payload has its registered arity")
                        };
                        unsafe { function($(*$argument),*) }
                    }),*
                }
            }
        }
    };
    (@type $argument:ident) => { SEXP };
}

call_routines! {
    Args0(0;),
    Args1(1; a),
    Args2(2; a, b),
    Args3(3; a, b, c),
    Args4(4; a, b, c, d),
    Args5(5; a, b, c, d, e),
    Args6(6; a, b, c, d, e, f),
    Args7(7; a, b, c, d, e, f, g),
    Args8(8; a, b, c, d, e, f, g, h),
    Args9(9; a, b, c, d, e, f, g, h, i),
    Args10(10; a, b, c, d, e, f, g, h, i, j),
}

/// Registered R payload count, excluding the routine name and PACKAGE controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PayloadArity {
    Fixed(usize),
    Variadic,
}

#[derive(Clone, Copy)]
pub(crate) enum NativeRoutine {
    Call(CallRoutine),
    External1(unsafe extern "C-unwind" fn(SEXP) -> SEXP, PayloadArity),
    External2(
        unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP,
        PayloadArity,
    ),
}

impl NativeRoutine {
    pub(crate) fn interface(self) -> NativeInterface {
        match self {
            Self::Call(_) => NativeInterface::Call,
            Self::External1(_, _) => NativeInterface::External,
            Self::External2(_, _) => NativeInterface::External2,
        }
    }

    pub(crate) fn payload_arity(self) -> PayloadArity {
        match self {
            Self::Call(function) => PayloadArity::Fixed(function.arity()),
            Self::External1(_, arity) | Self::External2(_, arity) => arity,
        }
    }

    /// Interface admission comes first. Fixed payload counts are carried by the
    /// registered descriptor; a Call also derives its ABI arity from its type.
    /// Variadic routines remain responsible for their individual input contract.
    pub(crate) fn validate_request(
        self,
        requested: NativeInterface,
        payload_count: usize,
    ) -> Result<(), NativeAdmissionError> {
        let registered = self.interface();
        if registered != requested {
            return Err(NativeAdmissionError::InterfaceMismatch {
                registered,
                requested,
            });
        }
        if let PayloadArity::Fixed(expected) = self.payload_arity() {
            if payload_count != expected {
                return Err(NativeAdmissionError::ArgumentCount {
                    expected,
                    actual: payload_count,
                });
            }
        }
        Ok(())
    }

    /// The caller owns the original runtime and all argument allocations for
    /// the entire invocation and validates the result before publishing it.
    pub(crate) unsafe fn invoke_call(
        self,
        arguments: &[SEXP],
    ) -> Result<SEXP, NativeAdmissionError> {
        self.validate_request(NativeInterface::Call, arguments.len())?;
        let Self::Call(function) = self else {
            unreachable!("admission established the Call interface")
        };
        Ok(unsafe { function.invoke(arguments) })
    }

    /// The caller retains the original runtime and argument-list graph across
    /// callbacks and collection, then validates the returned allocation. The
    /// supplied count matches that retained graph's actual payload (no name or
    /// controls); admission is repeated here before calling the typed pointer.
    pub(crate) unsafe fn invoke_external1(
        self,
        arguments: SEXP,
        payload_count: usize,
    ) -> Result<SEXP, NativeAdmissionError> {
        self.validate_request(NativeInterface::External, payload_count)?;
        let Self::External1(function, _) = self else {
            unreachable!("admission established the External interface")
        };
        Ok(unsafe { function(arguments) })
    }

    /// The caller retains the original runtime and the complete call, operator,
    /// argument-list and environment graphs across callbacks and collection.
    /// The supplied count matches the retained list's actual payload excluding
    /// its name and controls; admission is repeated before invocation.
    pub(crate) unsafe fn invoke_external2(
        self,
        call: SEXP,
        operator: SEXP,
        arguments: SEXP,
        environment: SEXP,
        payload_count: usize,
    ) -> Result<SEXP, NativeAdmissionError> {
        self.validate_request(NativeInterface::External2, payload_count)?;
        let Self::External2(function, _) = self else {
            unreachable!("admission established the External2 interface")
        };
        Ok(unsafe { function(call, operator, arguments, environment) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    unsafe extern "C-unwind" fn call_two(_a: SEXP, _b: SEXP) -> SEXP {
        INVOCATIONS.with(|count| count.set(count.get() + 1));
        std::ptr::null_mut()
    }

    unsafe extern "C-unwind" fn external_one(_arguments: SEXP) -> SEXP {
        INVOCATIONS.with(|count| count.set(count.get() + 1));
        std::ptr::null_mut()
    }

    unsafe extern "C-unwind" fn external_two(
        _call: SEXP,
        _operator: SEXP,
        _arguments: SEXP,
        _environment: SEXP,
    ) -> SEXP {
        INVOCATIONS.with(|count| count.set(count.get() + 1));
        std::ptr::null_mut()
    }

    #[test]
    fn typed_call_rejects_short_long_and_external_requests_before_invocation() {
        INVOCATIONS.with(|count| count.set(0));
        let routine = NativeRoutine::Call(CallRoutine::Args2(call_two));
        let nil = std::ptr::null_mut();
        // These sentinels neither inspect raw values nor access a runtime.
        unsafe {
            for count in [0, 1, 3, 10] {
                assert_eq!(
                    routine.invoke_call(&vec![nil; count]),
                    Err(NativeAdmissionError::ArgumentCount {
                        expected: 2,
                        actual: count
                    })
                );
            }
            for requested in [NativeInterface::External, NativeInterface::External2] {
                let result = match requested {
                    NativeInterface::External => routine.invoke_external1(nil, 0),
                    NativeInterface::External2 => routine.invoke_external2(nil, nil, nil, nil, 0),
                    NativeInterface::Call => unreachable!(),
                };
                assert_eq!(
                    result,
                    Err(NativeAdmissionError::InterfaceMismatch {
                        registered: NativeInterface::Call,
                        requested,
                    })
                );
            }
            assert_eq!(INVOCATIONS.with(Cell::get), 0);
            assert_eq!(routine.invoke_call(&[nil, nil]), Ok(nil));
            assert_eq!(INVOCATIONS.with(Cell::get), 1);
        }
    }

    #[test]
    fn typed_external_rejects_other_interfaces_before_invocation() {
        INVOCATIONS.with(|count| count.set(0));
        let nil = std::ptr::null_mut();
        for routine in [
            NativeRoutine::External1(external_one, PayloadArity::Fixed(0)),
            NativeRoutine::External2(external_two, PayloadArity::Fixed(0)),
        ] {
            let registered = routine.interface();
            unsafe {
                assert_eq!(
                    routine.invoke_call(&[nil]),
                    Err(NativeAdmissionError::InterfaceMismatch {
                        registered,
                        requested: NativeInterface::Call,
                    })
                );
                let requested = if registered == NativeInterface::External {
                    NativeInterface::External2
                } else {
                    NativeInterface::External
                };
                let result = match requested {
                    NativeInterface::External => routine.invoke_external1(nil, 0),
                    NativeInterface::External2 => routine.invoke_external2(nil, nil, nil, nil, 0),
                    NativeInterface::Call => unreachable!(),
                };
                assert_eq!(
                    result,
                    Err(NativeAdmissionError::InterfaceMismatch {
                        registered,
                        requested
                    })
                );
            }
        }
        assert_eq!(INVOCATIONS.with(Cell::get), 0);
        unsafe {
            assert_eq!(
                NativeRoutine::External1(external_one, PayloadArity::Fixed(0))
                    .invoke_external1(nil, 0),
                Ok(nil)
            );
            assert_eq!(
                NativeRoutine::External2(external_two, PayloadArity::Fixed(0))
                    .invoke_external2(nil, nil, nil, nil, 0),
                Ok(nil)
            );
        }
        assert_eq!(INVOCATIONS.with(Cell::get), 2);
    }

    #[test]
    fn typed_external_fixed_payload_counts_reject_before_invocation() {
        INVOCATIONS.with(|count| count.set(0));
        let nil = std::ptr::null_mut();
        for expected in [0, 2] {
            for routine in [
                NativeRoutine::External1(external_one, PayloadArity::Fixed(expected)),
                NativeRoutine::External2(external_two, PayloadArity::Fixed(expected)),
            ] {
                let interface = routine.interface();
                // A simultaneous interface/count mismatch always reports the
                // interface; an invalid request cannot reach the sentinel body.
                assert_eq!(
                    routine.validate_request(NativeInterface::Call, 999),
                    Err(NativeAdmissionError::InterfaceMismatch {
                        registered: interface,
                        requested: NativeInterface::Call,
                    })
                );
                for actual in [0, 1, 3, 65] {
                    if actual == expected {
                        continue;
                    }
                    let result = unsafe {
                        match interface {
                            NativeInterface::External => routine.invoke_external1(nil, actual),
                            NativeInterface::External2 => {
                                routine.invoke_external2(nil, nil, nil, nil, actual)
                            }
                            NativeInterface::Call => unreachable!(),
                        }
                    };
                    assert_eq!(
                        result,
                        Err(NativeAdmissionError::ArgumentCount { expected, actual })
                    );
                }
            }
        }
        assert_eq!(INVOCATIONS.with(Cell::get), 0);
        unsafe {
            NativeRoutine::External1(external_one, PayloadArity::Fixed(0))
                .invoke_external1(nil, 0)
                .unwrap();
            NativeRoutine::External2(external_two, PayloadArity::Fixed(2))
                .invoke_external2(nil, nil, nil, nil, 2)
                .unwrap();
        }
        assert_eq!(INVOCATIONS.with(Cell::get), 2);
    }

    #[test]
    fn typed_external_variadic_payload_counts_retain_interface_admission() {
        INVOCATIONS.with(|count| count.set(0));
        let nil = std::ptr::null_mut();
        for routine in [
            NativeRoutine::External1(external_one, PayloadArity::Variadic),
            NativeRoutine::External2(external_two, PayloadArity::Variadic),
        ] {
            assert!(matches!(
                routine.validate_request(NativeInterface::Call, 1),
                Err(NativeAdmissionError::InterfaceMismatch { .. })
            ));
            for actual in [0, 1, 65, 66] {
                unsafe {
                    match routine.interface() {
                        NativeInterface::External => routine.invoke_external1(nil, actual),
                        NativeInterface::External2 => {
                            routine.invoke_external2(nil, nil, nil, nil, actual)
                        }
                        NativeInterface::Call => unreachable!(),
                    }
                    .unwrap();
                }
            }
        }
        assert_eq!(INVOCATIONS.with(Cell::get), 8);
    }
}
