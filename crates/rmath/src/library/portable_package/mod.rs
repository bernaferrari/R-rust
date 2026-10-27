//! Original pinned package images share one namespace and lazy-record lifecycle.
use crate::sexp::object::{Sexp, SexpResult};
use std::collections::HashSet;
use std::sync::OnceLock;
mod bridge;
mod owned;

pub(crate) struct PackageImage {
    pub(crate) name: &'static str,
    version: &'static str,
    directory: &'static str,
    database: LazyDatabase,
    sysdata: Option<LazyDatabase>,
    pub(crate) namespace_source: &'static str,
    exports: &'static [u8],
    imports: Option<&'static [u8]>,
    s3_methods: &'static [u8],
    envhook: &'static str,
    examples_index: &'static [u8],
    examples_data: &'static [u8],
}

macro_rules! image {
    ($name:literal, $assets:literal, $sysdata:expr) => {
        image!($name, $assets, $sysdata, None)
    };
    ($name:literal, $assets:literal, $sysdata:expr, $imports:expr) => {
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
            imports: $imports,
            s3_methods: include_bytes!(concat!($assets, "/S3methods.rds")),
            envhook: include_str!(concat!($assets, "/envhook.R")),
            examples_index: include_bytes!(concat!($assets, "/examples.index")),
            examples_data: include_bytes!(concat!($assets, "/examples.data")),
        }
    };
}

static METHODS: PackageImage = image!("methods", "../methods/portable/assets", None);
static GRDEVICES: PackageImage = image!("grDevices", "assets/grDevices", None);
static GRAPHICS: PackageImage = image!(
    "graphics",
    "assets/graphics",
    None,
    Some(include_bytes!("assets/graphics/imports.rds"))
);
static STATS: PackageImage = image!(
    "stats",
    "assets/stats",
    None,
    Some(include_bytes!("assets/stats/imports.rds"))
);
static STATS4: PackageImage = image!(
    "stats4",
    "assets/stats4",
    None,
    Some(include_bytes!("assets/stats4/imports.rds"))
);
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
        "grDevices" => Some(&GRDEVICES),
        "graphics" => Some(&GRAPHICS),
        "stats" => Some(&STATS),
        "stats4" => Some(&STATS4),
        _ => None,
    }
}

/// Return the pinned GNU `tools::Rd2ex` source for a package help alias.
/// The index stores byte offsets into immutable embedded source data, so
/// browser runtimes can run examples through text connections without files.
pub(crate) fn example_source(package: &str, topic: &str) -> Option<&'static str> {
    let image = image(package)?;
    for record in image.examples_index.split(|byte| *byte == b'\n') {
        let mut fields = record.split(|byte| *byte == b'\t');
        let Some(name) = fields.next() else {
            continue;
        };
        if name != topic.as_bytes() {
            continue;
        }
        let offset = std::str::from_utf8(fields.next()?)
            .ok()?
            .parse::<usize>()
            .ok()?;
        let length = std::str::from_utf8(fields.next()?)
            .ok()?
            .parse::<usize>()
            .ok()?;
        let end = offset.checked_add(length)?;
        return std::str::from_utf8(image.examples_data.get(offset..end)?).ok();
    }
    None
}

/// Whether a registered primitive name is also supplied by an embedded
/// package's S3method declaration. Such native implementations remain
/// available to S3 dispatch, but must not shadow the package method through
/// inherited symbol lookup before that package's namespace is consulted.
pub(crate) fn is_namespace_s3_method_name(name: &str) -> bool {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            [
                &METHODS, &UTILS, &TOOLS, &GRDEVICES, &GRAPHICS, &STATS, &STATS4,
            ]
            .into_iter()
            .flat_map(|image| {
                crate::mainutils::essentials::s3_method_function_names(image.namespace_source)
            })
            .collect()
        })
        .contains(name)
}

pub(crate) struct LazyDatabase {
    pub(crate) path: &'static str,
    bytes: &'static [u8],
    index: &'static [u8],
}

fn databases() -> impl Iterator<Item = &'static LazyDatabase> {
    [
        &METHODS, &UTILS, &TOOLS, &GRDEVICES, &GRAPHICS, &STATS, &STATS4,
    ]
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

#[cfg(test)]
mod example_asset_tests {
    use super::example_source;

    #[test]
    fn pinned_examples_cover_aliases_and_multiple_package_images() {
        let new = example_source("methods", "new").expect("methods::new example");
        let initialize =
            example_source("methods", "initialize").expect("methods::initialize alias");
        assert_eq!(new, initialize);
        assert!(new.contains("setClass(\"trackCurve\""));

        let filter = example_source("stats", "filter").expect("stats::filter example");
        assert!(filter.contains("filter(x, rep(1, 3))"));
        assert!(example_source("methods", "__missing_topic__").is_none());
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
