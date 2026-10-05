//! Original pinned package images share one namespace and lazy-record lifecycle.
use crate::sexp::object::{Sexp, SexpResult};
mod bridge;
mod owned;

pub(crate) struct PackageImage {
    pub(crate) name: &'static str,
    version: &'static str,
    directory: &'static str,
    database: LazyDatabase,
    sysdata: Option<LazyDatabase>,
    namespace_source: &'static str,
    exports: &'static [u8],
    s3_methods: &'static [u8],
    envhook: &'static str,
}

macro_rules! image {
    ($name:literal, $assets:literal, $sysdata:expr) => {
        PackageImage {
            name: $name,
            version: "4.7.0",
            directory: concat!("<builtin:", $name, ">"),
            database: LazyDatabase {
                path: concat!("<builtin:", $name, ">/R/", $name, ".rdb"),
                bytes: include_bytes!(concat!($assets, "/", $name, ".rdb")),
                index: include_bytes!(concat!($assets, "/index.rds")),
            },
            sysdata: $sysdata,
            namespace_source: include_str!(concat!($assets, "/NAMESPACE")),
            exports: include_bytes!(concat!($assets, "/exports.rds")),
            s3_methods: include_bytes!(concat!($assets, "/S3methods.rds")),
            envhook: include_str!(concat!($assets, "/envhook.R")),
        }
    };
}

static METHODS: PackageImage = image!("methods", "../methods/portable/assets", None);
static UTILS: PackageImage = image!(
    "utils",
    "assets/utils",
    Some(LazyDatabase {
        path: "<builtin:utils>/R/sysdata.rdb",
        bytes: include_bytes!("assets/utils/sysdata.rdb"),
        index: include_bytes!("assets/utils/sysdata-index.rds"),
    })
);

static TOOLS: PackageImage = image!(
    "tools",
    "assets/tools",
    Some(LazyDatabase {
        path: "<builtin:tools>/R/sysdata.rdb",
        bytes: include_bytes!("assets/tools/sysdata.rdb"),
        index: include_bytes!("assets/tools/sysdata-index.rds"),
    })
);

pub(crate) fn image(name: &str) -> Option<&'static PackageImage> {
    match name {
        "methods" => Some(&METHODS),
        "utils" => Some(&UTILS),
        "tools" => Some(&TOOLS),
        _ => None,
    }
}

pub(crate) struct LazyDatabase {
    pub(crate) path: &'static str,
    bytes: &'static [u8],
    index: &'static [u8],
}

fn databases() -> impl Iterator<Item = &'static LazyDatabase> {
    [&METHODS, &UTILS, &TOOLS]
        .into_iter()
        .flat_map(|image| std::iter::once(&image.database).chain(image.sysdata.as_ref()))
}

pub(crate) fn database_path(path: &std::path::Path) -> Option<&'static LazyDatabase> {
    databases().find(|database| path == std::path::Path::new(database.path))
}

pub(crate) fn database(file: &Sexp<'_>) -> SexpResult<Option<&'static LazyDatabase>> {
    for database in databases() {
        if database.is_database(file)? {
            return Ok(Some(database));
        }
    }
    Ok(None)
}

impl LazyDatabase {
    fn is_database(&self, file: &Sexp<'_>) -> SexpResult<bool> {
        if file.typeof_() != crate::sexp::SEXPTYPE::STRSXP
            || file.header().sxpinfo.alt()
            || file.len() != 1
        {
            return Ok(false);
        }
        file.try_string_elt(0)?.try_char_eq(self.path.as_bytes())
    }
}

impl PackageImage {
    pub(crate) fn namespace(&'static self) -> Result<Sexp<'static>, String> {
        owned::namespace(self, bridge::base().map_err(|error| error.to_string())?)
    }

    pub(crate) fn attach(&'static self) -> Result<(), String> {
        owned::attach(self, bridge::base().map_err(|error| error.to_string())?)
    }
}

impl LazyDatabase {
    pub(crate) fn read_database(
        &'static self,
        file: Sexp<'static>,
        key: Sexp<'static>,
    ) -> Result<Option<Sexp<'static>>, String> {
        owned::read_database(self, file, key)
    }

    pub(crate) fn fetch(
        &'static self,
        key: Sexp<'static>,
        file: Sexp<'static>,
        compressed: Sexp<'static>,
        hook: Sexp<'static>,
    ) -> Result<Sexp<'static>, String> {
        owned::fetch(self, key, file, compressed, hook)
    }
}
