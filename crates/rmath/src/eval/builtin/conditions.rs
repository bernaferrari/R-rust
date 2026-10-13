//! Condition and error-signalling builtin entries.

use super::EvaluatedBuiltin;

pub(super) const CONDITIONS: &[EvaluatedBuiltin] = &[
    EvaluatedBuiltin {
        name: "geterrmessage",
        handler: crate::mainutils::errors::do_geterrmessage,
    },
    EvaluatedBuiltin {
        name: "conditionMessage",
        handler: crate::mainutils::essentials::do_conditionMessage,
    },
    EvaluatedBuiltin {
        name: "conditionCall",
        handler: crate::mainutils::essentials::do_conditionCall,
    },
    EvaluatedBuiltin {
        name: "errorCondition",
        handler: crate::mainutils::essentials::do_errorCondition,
    },
    EvaluatedBuiltin {
        name: "warningCondition",
        handler: crate::mainutils::essentials::do_warningCondition,
    },
    EvaluatedBuiltin {
        name: "signalCondition",
        handler: crate::mainutils::essentials::do_signalCondition_r,
    },
    EvaluatedBuiltin {
        name: "suppressWarnings",
        handler: crate::mainutils::essentials::do_suppress_warnings,
    },
    EvaluatedBuiltin {
        name: "restartDescription",
        handler: crate::mainutils::essentials::do_restartDescription,
    },
    EvaluatedBuiltin {
        name: "warning",
        handler: crate::mainutils::essentials::do_warning,
    },
    EvaluatedBuiltin {
        name: "warnings",
        handler: crate::mainutils::essentials::do_warnings,
    },
    EvaluatedBuiltin {
        name: "tryCatch",
        handler: crate::mainutils::essentials::do_tryCatch,
    },
];
