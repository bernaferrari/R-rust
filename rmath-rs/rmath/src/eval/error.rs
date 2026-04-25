//! Evaluator error kinds.

use std::os::raw::c_int;

/// Errors that can occur during evaluation.
#[derive(Debug)]
pub enum EvalError {
    TooDeeplyNested,
    TimeLimitExceeded,
    IncorrectDotsContext,
    ObjectNotFound(String),
    MissingArgument,
    FunctionNotFound(String),
    NonFunction,
    UnimplementedType(c_int),
    BytecodeNotImplemented,
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::TooDeeplyNested => write!(f, "evaluation nested too deeply"),
            EvalError::TimeLimitExceeded => write!(f, "evaluation time limit exceeded"),
            EvalError::IncorrectDotsContext => write!(f, "'...' used in an incorrect context"),
            EvalError::ObjectNotFound(name) => write!(f, "object '{}' not found", name),
            EvalError::MissingArgument => write!(f, "missing argument"),
            EvalError::FunctionNotFound(name) => write!(f, "could not find function \"{}\"", name),
            EvalError::NonFunction => write!(f, "attempt to apply non-function"),
            EvalError::UnimplementedType(t) => write!(f, "unimplemented type in eval: {}", t),
            EvalError::BytecodeNotImplemented => {
                write!(f, "bytecode evaluation not yet implemented")
            }
        }
    }
}
