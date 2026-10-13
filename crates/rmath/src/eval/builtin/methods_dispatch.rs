//! S4/methods-dispatch builtin entries.

use super::EvaluatedBuiltin;

pub(super) const METHODS_DISPATCH: &[EvaluatedBuiltin] = &[
    EvaluatedBuiltin {
        name: "standardGeneric",
        handler: crate::mainutils::objects::do_standardGeneric,
    },
    EvaluatedBuiltin {
        name: "NextMethod",
        handler: crate::mainutils::objects::do_nextmethod,
    },
    EvaluatedBuiltin {
        name: "isS4",
        handler: crate::mainutils::essentials::do_isS4,
    },
    EvaluatedBuiltin {
        name: "setS4Object",
        handler: crate::mainutils::objects::do_setS4Object,
    },
    EvaluatedBuiltin {
        name: "asS4",
        handler: crate::mainutils::objects::do_asS4,
    },
    EvaluatedBuiltin {
        name: ".asS4",
        handler: crate::mainutils::objects::do_asS4,
    },
];
