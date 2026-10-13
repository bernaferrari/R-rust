//! Namespace and environment-search builtin entries.

use super::EvaluatedBuiltin;

pub(super) const NAMESPACE: &[EvaluatedBuiltin] = &[
    EvaluatedBuiltin {
        name: "getNamespaceValue",
        handler: crate::mainutils::essentials::do_get_namespace_value,
    },
    EvaluatedBuiltin {
        name: "La_library",
        handler: crate::mainutils::lapack::do_La_library,
    },
    EvaluatedBuiltin {
        name: "library.dynam",
        handler: crate::mainutils::essentials::do_library_dynam,
    },
    EvaluatedBuiltin {
        name: ".rport_library",
        handler: crate::mainutils::essentials::do_library,
    },
    EvaluatedBuiltin {
        name: ".rport_require",
        handler: crate::mainutils::essentials::do_require,
    },
    EvaluatedBuiltin {
        name: "loadNamespace",
        handler: crate::mainutils::essentials::do_load_namespace,
    },
    EvaluatedBuiltin {
        name: "requireNamespace",
        handler: crate::mainutils::essentials::do_require_namespace,
    },
    EvaluatedBuiltin {
        name: "getNamespace",
        handler: crate::mainutils::essentials::do_get_namespace,
    },
    EvaluatedBuiltin {
        name: "asNamespace",
        handler: crate::mainutils::essentials::do_as_namespace,
    },
    EvaluatedBuiltin {
        name: "loadedNamespaces",
        handler: crate::mainutils::essentials::do_loaded_namespaces,
    },
    EvaluatedBuiltin {
        name: "unloadNamespace",
        handler: crate::mainutils::essentials::do_unload_namespace,
    },
    EvaluatedBuiltin {
        name: "getRegisteredNamespace",
        handler: crate::mainutils::essentials::do_get_registered_namespace,
    },
    EvaluatedBuiltin {
        name: "search",
        handler: crate::mainutils::envir::do_search,
    },
    EvaluatedBuiltin {
        name: "searchpaths",
        handler: crate::mainutils::essentials::do_searchpaths,
    },
];
