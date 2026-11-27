//! The pinned baked-package assets exactly as the pure world carries
//! them — the R-level namespace sources, the export manifests, and the
//! package DESCRIPTION inventory. The bootstrap logic in the runtime
//! crate consumes these tables; the bytes live here because include_str
//! is compile-time crate-local.

/// The R-level namespace sources evaluated into their package
/// environments at bootstrap — GNU's base/utils R-layer functions
/// exactly as the package sources define them.
pub const RLEVEL_SOURCES: &[(&str, &str)] = &[
    ("base", include_str!("assets/rlevel/base.R")),
    ("utils", include_str!("assets/rlevel/utils.R")),
];

/// The pinned baked-package export manifests, gzipped rds exactly as
/// the pinned metadata declares them.
pub const PACKAGE_EXPORTS: &[(&str, &[u8])] = &[
    ("stats", include_bytes!("assets/packages/stats/exports.rds")),
    ("utils", include_bytes!("assets/packages/utils/exports.rds")),
    ("graphics", include_bytes!("assets/packages/graphics/exports.rds")),
    ("grDevices", include_bytes!("assets/packages/grDevices/exports.rds")),
    ("stats4", include_bytes!("assets/packages/stats4/exports.rds")),
    ("tools", include_bytes!("assets/packages/tools/exports.rds")),
    ("methods", include_bytes!("assets/methods/exports.rds")),
];

/// The baked package set exactly as the pinned assets carry it — the
/// base-package inventory GNU R installs, minus the internal compiler
/// assets that never surface as a user package.
pub const BAKED_PACKAGES: &[(&str, &str)] = &[
    ("datasets", include_str!("assets/datasets/DESCRIPTION")),
    ("methods", include_str!("assets/methods/DESCRIPTION")),
    ("stats", include_str!("assets/packages/stats/DESCRIPTION")),
    ("utils", include_str!("assets/packages/utils/DESCRIPTION")),
    ("graphics", include_str!("assets/packages/graphics/DESCRIPTION")),
    ("grDevices", include_str!("assets/packages/grDevices/DESCRIPTION")),
    ("stats4", include_str!("assets/packages/stats4/DESCRIPTION")),
    ("tools", include_str!("assets/packages/tools/DESCRIPTION")),
];
