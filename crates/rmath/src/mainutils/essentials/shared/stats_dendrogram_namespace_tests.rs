use crate::sexp::session::RSession;

#[test]
fn dendrogram_helpers_live_in_stats_namespace_after_base_only_startup() {
    let session = RSession::new_without_default_packages();
    session.with_active(|| unsafe {
        let base = crate::sexp::globals::R_BaseEnv();
        let generic = crate::sexp::symbol::Rf_install(c"as.dendrogram".as_ptr());
        let method = crate::sexp::symbol::Rf_install(c"as.dendrogram.hclust".as_ptr());
        assert_eq!(
            crate::sexp::envir::R_findVarInFrame(base, generic),
            crate::sexp::globals::R_UnboundValue(),
            "base bootstrap must not install the stats generic"
        );
        assert_eq!(
            crate::sexp::envir::R_findVarInFrame(base, method),
            crate::sexp::globals::R_UnboundValue(),
            "base bootstrap must not install the stats method"
        );

        let stats = crate::mainutils::essentials::load_package_namespace_by_name("stats")
            .expect("the stats namespace should load after base-only bootstrap");
        for symbol in [generic, method] {
            let mut value = crate::sexp::envir::R_findVarInFrame(stats, symbol);
            if crate::sexp::accessors::TYPEOF(value)
                == crate::sexp::SEXPTYPE::PROMSXP.as_c_int()
            {
                value = crate::eval::eval::Rf_eval(value, stats);
            }
            assert_eq!(
                crate::sexp::accessors::TYPEOF(value),
                crate::sexp::SEXPTYPE::CLOSXP.as_c_int(),
                "the stats package definition should be installed as a closure"
            );
            assert_eq!(
                crate::sexp::accessors::CLOENV(value),
                stats,
                "stats definitions must retain the stats lexical environment"
            );
        }
    });
}
