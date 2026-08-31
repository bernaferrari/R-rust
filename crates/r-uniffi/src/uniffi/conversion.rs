//! Owned value records crossing the UniFFI boundary plus the conversions from
//! the `r_embed` facade types. Every type here is plain owned data: no raw
//! interpreter pointers ever cross this layer.

use r_embed::AndroidRuntimePaths as EmbedAndroidRuntimePaths;

use super::error::RError;

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProgressUpdate {
    pub progress: f64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum RValueKind {
    Null,
    Logical,
    Integer,
    Real,
    LogicalVector,
    IntegerVector,
    RealVector,
    StringVector,
    RawVector,
    ComplexVector,
    List,
    Unsupported,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct RComplexValue {
    pub real: f64,
    pub imaginary: f64,
}

impl From<r_embed::RComplexValue> for RComplexValue {
    fn from(value: r_embed::RComplexValue) -> Self {
        RComplexValue {
            real: value.real,
            imaginary: value.imaginary,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RValue {
    pub kind: RValueKind,
    pub logical_scalar: Option<bool>,
    pub integer_scalar: Option<i32>,
    pub real_scalar: Option<f64>,
    pub logical_values: Vec<Option<bool>>,
    pub integer_values: Vec<Option<i32>>,
    pub real_values: Vec<Option<f64>>,
    pub string_values: Vec<Option<String>>,
    pub raw_values: Vec<u8>,
    pub complex_values: Vec<Option<RComplexValue>>,
    pub list_values: Vec<RValue>,
    pub type_name: String,
    pub error: String,
    pub metadata: RMetadata,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RAttribute {
    pub name: String,
    pub value: RValue,
}

#[derive(Debug, Clone, PartialEq, Default, uniffi::Record)]
pub struct RMetadata {
    pub names: Option<Vec<Option<String>>>,
    pub dim: Option<Vec<i32>>,
    pub class: Option<Vec<Option<String>>>,
    pub levels: Option<Vec<Option<String>>>,
    pub attributes: Vec<RAttribute>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct EvalResult {
    pub output: String,
    pub value: RValue,
}

/// A bounded table slice. Only `value` crosses the FFI boundary; `total_rows`
/// describes the source object without serializing its unloaded rows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DataFramePage {
    pub value: RValue,
    pub total_rows: u64,
    pub offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeInfo {
    pub is_active: bool,
    pub library_paths: Vec<String>,
    pub temp_dir: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ResourceLimits {
    pub max_eval_depth: u64,
    pub max_execution_time_ms: u64,
    pub max_alloc_bytes: u64,
    pub max_arena_nodes: u64,
}

impl From<r_embed::RResourceLimits> for ResourceLimits {
    fn from(limits: r_embed::RResourceLimits) -> Self {
        ResourceLimits {
            max_eval_depth: limits.max_eval_depth,
            max_execution_time_ms: limits.max_execution_time_ms,
            max_alloc_bytes: limits.max_alloc_bytes,
            max_arena_nodes: limits.max_arena_nodes,
        }
    }
}

impl From<ResourceLimits> for r_embed::RResourceLimits {
    fn from(limits: ResourceLimits) -> Self {
        r_embed::RResourceLimits {
            max_eval_depth: limits.max_eval_depth,
            max_execution_time_ms: limits.max_execution_time_ms,
            max_alloc_bytes: limits.max_alloc_bytes,
            max_arena_nodes: limits.max_arena_nodes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AndroidRuntimePaths {
    pub app_files_dir: String,
    pub cache_dir: String,
    pub bundled_library_dir: Option<String>,
    pub user_library_dir: String,
    pub temp_dir: String,
    pub library_paths: Vec<String>,
}

impl From<EmbedAndroidRuntimePaths> for AndroidRuntimePaths {
    fn from(paths: EmbedAndroidRuntimePaths) -> Self {
        AndroidRuntimePaths {
            app_files_dir: paths.app_files_dir.clone(),
            cache_dir: paths.cache_dir.clone(),
            bundled_library_dir: paths.bundled_library_dir.clone(),
            user_library_dir: paths.user_library_dir(),
            temp_dir: paths.temp_dir(),
            library_paths: paths.library_paths(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub title: String,
    pub description: String,
    pub license: String,
    pub depends: String,
    pub imports: String,
    pub suggests: String,
    pub needs_compilation: bool,
    pub path: String,
    pub library_path: String,
}

impl From<r_embed::RPackageInfo> for PackageInfo {
    fn from(info: r_embed::RPackageInfo) -> Self {
        PackageInfo {
            name: info.name,
            version: info.version,
            title: info.title,
            description: info.description,
            license: info.license,
            depends: info.depends,
            imports: info.imports,
            suggests: info.suggests,
            needs_compilation: info.needs_compilation,
            path: info.path,
            library_path: info.library_path,
        }
    }
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

pub(crate) fn empty_value(kind: RValueKind) -> RValue {
    RValue {
        kind,
        logical_scalar: None,
        integer_scalar: None,
        real_scalar: None,
        logical_values: Vec::new(),
        integer_values: Vec::new(),
        real_values: Vec::new(),
        string_values: Vec::new(),
        raw_values: Vec::new(),
        complex_values: Vec::new(),
        list_values: Vec::new(),
        type_name: String::new(),
        error: String::new(),
        metadata: RMetadata::default(),
    }
}

/// Sentinel [`EvalResult`] for outcomes that carry no evaluated R value
/// (e.g. retained async render results report `"render complete"`).
pub(crate) fn null_eval_result(output: &str) -> EvalResult {
    EvalResult {
        output: output.to_string(),
        value: empty_value(RValueKind::Null),
    }
}

impl From<r_embed::RValue> for RValue {
    fn from(value: r_embed::RValue) -> Self {
        match value {
            r_embed::RValue::Null => empty_value(RValueKind::Null),
            r_embed::RValue::Logical(value) => RValue {
                logical_scalar: value,
                ..empty_value(RValueKind::Logical)
            },
            r_embed::RValue::Integer(value) => RValue {
                integer_scalar: value,
                ..empty_value(RValueKind::Integer)
            },
            r_embed::RValue::Real(value) => RValue {
                real_scalar: value,
                ..empty_value(RValueKind::Real)
            },
            r_embed::RValue::LogicalVector(values) => RValue {
                logical_values: values,
                ..empty_value(RValueKind::LogicalVector)
            },
            r_embed::RValue::IntegerVector(values) => RValue {
                integer_values: values,
                ..empty_value(RValueKind::IntegerVector)
            },
            r_embed::RValue::RealVector(values) => RValue {
                real_values: values,
                ..empty_value(RValueKind::RealVector)
            },
            r_embed::RValue::StringVector(values) => RValue {
                string_values: values,
                ..empty_value(RValueKind::StringVector)
            },
            r_embed::RValue::RawVector(values) => RValue {
                raw_values: values,
                ..empty_value(RValueKind::RawVector)
            },
            r_embed::RValue::ComplexVector(values) => RValue {
                complex_values: values
                    .into_iter()
                    .map(|value| value.map(RComplexValue::from))
                    .collect(),
                ..empty_value(RValueKind::ComplexVector)
            },
            r_embed::RValue::List(values) => RValue {
                list_values: values.into_iter().map(RValue::from).collect(),
                ..empty_value(RValueKind::List)
            },
            r_embed::RValue::Attributed { value, metadata } => {
                let mut value = RValue::from(*value);
                value.metadata = RMetadata::from(metadata);
                value
            }
            r_embed::RValue::Unsupported { type_name } => RValue {
                type_name,
                ..empty_value(RValueKind::Unsupported)
            },
            r_embed::RValue::Error(message) => RValue {
                error: message,
                ..empty_value(RValueKind::Error)
            },
        }
    }
}

impl From<r_embed::RAttribute> for RAttribute {
    fn from(attribute: r_embed::RAttribute) -> Self {
        RAttribute {
            name: attribute.name,
            value: RValue::from(attribute.value),
        }
    }
}

impl From<r_embed::RMetadata> for RMetadata {
    fn from(metadata: r_embed::RMetadata) -> Self {
        RMetadata {
            names: metadata.names,
            dim: metadata.dim,
            class: metadata.class,
            levels: metadata.levels,
            attributes: metadata
                .attributes
                .into_iter()
                .map(RAttribute::from)
                .collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// Exported helpers
// ---------------------------------------------------------------------------

/// Derive Android app-private runtime paths (user library, temp dir, search
/// path) from the app's file and cache directories.
#[uniffi::export]
pub fn android_runtime_paths(
    app_files_dir: String,
    cache_dir: String,
    bundled_library_dir: Option<String>,
) -> AndroidRuntimePaths {
    r_embed::AndroidRuntimePaths::new(app_files_dir, cache_dir, bundled_library_dir).into()
}

/// Reject package names that cannot denote an installed package.
pub(crate) fn validate_package_name(package: &str) -> Result<(), RError> {
    if package.trim().is_empty() {
        return Err(RError::InvalidInput("package name is empty".to_string()));
    }
    Ok(())
}
