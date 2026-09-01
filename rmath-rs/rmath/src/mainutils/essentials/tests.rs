// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

use std::ffi::{CStr, CString};
use std::os::raw::c_int;
use std::path::Path;

use crate::sexp::accessors::{
    CHAR, INTEGER, LOGICAL, REAL, SET_STRING_ELT, SET_VECTOR_ELT, STRING_ELT, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::ffi::{FALSE, NA_INTEGER, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::io::*;
use super::mathstats::*;
use super::matrix::*;
use super::runtime::*;

use super::shared::*;

unsafe fn test_pairlist(values: &[SEXP]) -> SEXP {
    unsafe {
        values
            .iter()
            .rev()
            .fold(R_NilValue(), |tail, value| Rf_cons(*value, tail))
    }
}

fn generated_namespace_input(mut seed: u64, len: usize) -> String {
    const ALPHABET: &[u8] =
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.,()#'\"`\\ \n\t";
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        seed = seed
            .wrapping_mul(2862933555777941757)
            .wrapping_add(3037000493);
        out.push(ALPHABET[((seed >> 33) as usize) % ALPHABET.len()] as char);
    }
    out
}

fn adversarial_iterations(default: u64) -> u64 {
    std::env::var("RPORT_ADVERSARIAL_ITERS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[test]
fn description_file_list_parses_collate_entries() {
    assert_eq!(
        description_file_list("'z-producer.R' \"a consumer.R\" helper.R"),
        vec![
            "z-producer.R".to_string(),
            "a consumer.R".to_string(),
            "helper.R".to_string()
        ]
    );
    assert_eq!(
        description_file_list("'one.R'\n  'two.R'\tthree.R"),
        vec![
            "one.R".to_string(),
            "two.R".to_string(),
            "three.R".to_string()
        ]
    );
    assert_eq!(
        description_file_list("'one.R' 'one.R'"),
        vec!["one.R".to_string()]
    );
}

#[test]
fn description_package_list_parses_depends_entries() {
    assert_eq!(
        description_package_list("R (>= 4.0.0), methods, corpbase (>= 0.1.0), corpbase"),
        vec![
            "R".to_string(),
            "methods".to_string(),
            "corpbase".to_string()
        ]
    );
}

#[test]
fn timezone_name_from_zoneinfo_paths() {
    assert_eq!(
        timezone_name_from_zoneinfo_path(Path::new("/var/db/timezone/zoneinfo/America/Sao_Paulo")),
        Some("America/Sao_Paulo".to_string())
    );
    assert_eq!(
        timezone_name_from_zoneinfo_path(Path::new("/usr/share/zoneinfo/Europe/London")),
        Some("Europe/London".to_string())
    );
    assert_eq!(
        timezone_name_from_zoneinfo_path(Path::new("/tmp/localtime")),
        None
    );
}

#[test]
fn skip_olson_metadata_components() {
    assert!(skip_olson_component("zone.tab"));
    assert!(skip_olson_component("posix"));
    assert!(!skip_olson_component("Africa"));
    assert!(!skip_olson_component("Sao_Paulo"));
}

#[test]
fn namespace_parser_handles_strings_comments_and_nested_calls() {
    let directives = parse_namespace_directives(
        r#"
            export(foo, "bar,baz", `quux`)
            exportPattern("^as\\.")
            import(stats)
            importFrom(utils, head, tail)
            S3method(print,myclass)
            S3method(format,myclass,format_myclass)
            useDynLib(nativebits)
            # export(commented_out)
            export("hash#inside")
            export(call_like(default = f(a, b)))
            "#,
    );

    assert_eq!(directives.exports[0], "foo");
    assert!(directives.exports.contains(&"bar,baz".to_string()));
    assert!(directives.exports.contains(&"quux".to_string()));
    assert!(directives.exports.contains(&"hash#inside".to_string()));
    assert!(
        directives
            .exports
            .contains(&"call_like(default = f(a, b))".to_string())
    );
    assert_eq!(directives.export_patterns, vec!["^as\\\\.".to_string()]);
    assert_eq!(directives.imports.len(), 2);
    assert_eq!(directives.s3_methods.len(), 2);
    assert_eq!(directives.native_libraries, vec!["nativebits".to_string()]);
}

#[test]
fn adversarial_namespace_inputs_do_not_panic() {
    let fixed = [
        "export(",
        "export(foo",
        "export(foo, # comment\n bar)",
        "S3method(print,",
        "useDynLib('unterminated)",
        "importFrom(pkg, f(a, b), c)",
        "export(`odd name`, \"comma,name\", 'hash#name')",
    ];

    for input in fixed {
        let result = std::panic::catch_unwind(|| parse_namespace_directives(input));
        assert!(
            result.is_ok(),
            "namespace parser panicked for fixed input: {input:?}"
        );
    }

    for seed in 0..adversarial_iterations(256) {
        let input = generated_namespace_input(seed, (seed as usize % 128) + 1);
        let result = std::panic::catch_unwind(|| parse_namespace_directives(&input));
        assert!(
            result.is_ok(),
            "namespace parser panicked for seed {seed}: {input:?}"
        );
    }
}

#[test]
fn essentials_get_option_delegates_to_options_runtime() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        crate::sexp::init::initialize_r();

        let option_name = Rf_mkString(CString::new("width").unwrap().as_ptr());
        let args = Rf_cons(option_name, R_NilValue());
        let result = do_getOption(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            args,
            std::ptr::null_mut(),
        );

        assert!(!result.is_null());
        assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
        assert_eq!(*INTEGER(result), 80);
    }
}

#[test]
fn essentials_options_delegates_to_options_runtime() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        crate::sexp::init::initialize_r();

        let result = do_options(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            R_NilValue(),
            std::ptr::null_mut(),
        );

        assert!(!result.is_null());
        assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
        assert!(XLENGTH(result) > 0);
    }
}

#[test]
fn test_do_log2_default_base_two() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        crate::sexp::init::initialize_r();

        let args = Rf_cons(Rf_ScalarReal(8.0), R_NilValue());
        let result = do_log2(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            args,
            std::ptr::null_mut(),
        );

        assert!(!result.is_null());
        assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
        assert!(((*REAL(result)).to_owned() - 3.0).abs() < 1e-10);
    }
}

#[test]
fn test_do_log2_explicit_base_is_preserved() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        crate::sexp::init::initialize_r();

        let args = Rf_cons(
            Rf_ScalarReal(8.0),
            Rf_cons(Rf_ScalarReal(8.0), R_NilValue()),
        );
        let result = do_log2(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            args,
            std::ptr::null_mut(),
        );

        assert!(!result.is_null());
        assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
        assert!(((*REAL(result)).to_owned() - 1.0).abs() < 1e-10);
    }
}

#[test]
fn psigamma_recycles_x_and_deriv_to_the_longer_length() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        crate::sexp::init::initialize_r();

        let x = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _x_guard = protect(x);
        *REAL(x) = 1.0;
        *REAL(x).add(1) = 2.0;

        let deriv = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
        let _deriv_guard = protect(deriv);
        *INTEGER(deriv) = 1;
        *INTEGER(deriv).add(1) = 3;
        *INTEGER(deriv).add(2) = 5;

        let args = test_pairlist(&[x, deriv]);
        let result = do_psigamma(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            args,
            std::ptr::null_mut(),
        );

        assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
        assert_eq!(XLENGTH(result), 3);
        let expected = [
            crate::special::polygamma::psigamma(1.0, 1.0),
            crate::special::polygamma::psigamma(2.0, 3.0),
            crate::special::polygamma::psigamma(1.0, 5.0),
        ];
        for (i, expected) in expected.into_iter().enumerate() {
            assert_eq!((*REAL(result).add(i)).to_bits(), expected.to_bits());
        }
    }
}

#[test]
fn psigamma_with_a_zero_length_input_returns_a_zero_length_real_vector() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        crate::sexp::init::initialize_r();

        let x = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        let _x_guard = protect(x);
        let deriv = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
        let _deriv_guard = protect(deriv);
        *INTEGER(deriv) = 1;
        *INTEGER(deriv).add(1) = 3;
        *INTEGER(deriv).add(2) = 5;

        let args = test_pairlist(&[x, deriv]);
        let result = do_psigamma(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            args,
            std::ptr::null_mut(),
        );

        assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
        assert_eq!(XLENGTH(result), 0);
    }
}

#[test]
fn test_gc_reports_session_memory_counters() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        crate::sexp::init::initialize_r();
        let value = Rf_allocVector3(SEXPTYPE::INTSXP, 256);
        let _guard = protect(value);

        let result = do_gc(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            R_NilValue(),
            std::ptr::null_mut(),
        );

        assert!(!result.is_null());
        assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
        let dim = crate::sexp::attrib_core::getAttrib(result, Rf_install(c"dim".as_ptr()));
        assert!(!dim.is_null());
        assert_eq!(*INTEGER(dim), 2);
        // Stock base::gc prints a `limit (Mb)` column: NA for Ncells (no
        // node limit) and the platform vector-pool ceiling for Vcells
        // (macOS startup default: max(physical, 16 Gb) = 32768 Mb here),
        // so the visible table is 2x7. When no ceiling applies the
        // all-NA column is dropped and the table is 2x6.
        let vlimit = crate::mainutils::memory_main::R_GetMaxVSize_memory();
        if vlimit == u64::MAX {
            assert_eq!(*INTEGER(dim).add(1), 6);
        } else {
            // limit (Mb) for Vcells sits at row 2, col 5 (index 4) of the
            // flattened table: Ncells row is cols 0-3, Vcells row cols 4-6.
            let data = REAL(result);
            assert!(*data.add(4) > 0.0, "Vcells limit should be reported");
        }

        let data = REAL(result);
        assert!(*data > 0.0, "Ncells used should reflect active arena nodes");
        assert!(*data.add(1) > 0.0, "Vcells used should reflect arena bytes");
        assert!(*data.add(9) >= *data.add(1), "Vcells max used >= used");
    }
}

#[test]
fn test_memory_size_uses_current_and_peak_arena_bytes() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        crate::sexp::init::initialize_r();
        let before = do_memory_size(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            R_NilValue(),
            std::ptr::null_mut(),
        );

        let value = Rf_allocVector3(SEXPTYPE::REALSXP, 512);
        let _guard = protect(value);
        let current = do_memory_size(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            R_NilValue(),
            std::ptr::null_mut(),
        );
        let peak_args = Rf_cons(Rf_ScalarLogical(TRUE), R_NilValue());
        let peak = do_memory_size(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            peak_args,
            std::ptr::null_mut(),
        );

        assert!(*REAL(current) > *REAL(before));
        assert!(*REAL(peak) >= *REAL(current));
    }
}

#[test]
fn test_gcinfo_is_session_local_and_returns_previous_value() {
    let left = crate::sexp::session::RSession::new();
    let right = crate::sexp::session::RSession::new();

    left.with_protected(|| unsafe {
        let args = Rf_cons(Rf_ScalarLogical(TRUE), R_NilValue());
        let old = do_gcinfo(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            args,
            std::ptr::null_mut(),
        );
        assert_eq!(*LOGICAL(old), FALSE);

        let args = Rf_cons(Rf_ScalarLogical(FALSE), R_NilValue());
        let old = do_gcinfo(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            args,
            std::ptr::null_mut(),
        );
        assert_eq!(*LOGICAL(old), TRUE);
    });

    right.with_protected(|| unsafe {
        let args = Rf_cons(Rf_ScalarLogical(FALSE), R_NilValue());
        let old = do_gcinfo(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            args,
            std::ptr::null_mut(),
        );
        assert_eq!(*LOGICAL(old), FALSE);
    });
}

#[test]
fn test_matrix_byrow_uses_column_major_storage() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        crate::sexp::init::initialize_r();
        let data = Rf_allocVector3(SEXPTYPE::INTSXP, 6);
        let _data_guard = protect(data);
        for i in 0..6 {
            *INTEGER(data).add(i) = (i + 1) as c_int;
        }
        let args = test_pairlist(&[
            data,
            Rf_ScalarInteger(2),
            Rf_ScalarInteger(3),
            Rf_ScalarLogical(TRUE),
        ]);

        let result = do_matrix(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            args,
            std::ptr::null_mut(),
        );

        assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
        assert_eq!(
            (0..6).map(|i| *INTEGER(result).add(i)).collect::<Vec<_>>(),
            vec![1, 4, 2, 5, 3, 6]
        );
    }
}

#[test]
fn test_matrix_zero_length_data_preserves_shape_and_fills_na() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        crate::sexp::init::initialize_r();
        let data = Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        let args = test_pairlist(&[
            data,
            Rf_ScalarInteger(2),
            Rf_ScalarInteger(2),
            Rf_ScalarLogical(FALSE),
        ]);

        let result = do_matrix(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            args,
            std::ptr::null_mut(),
        );

        assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
        assert_eq!(XLENGTH(result), 4);
        for i in 0..4 {
            assert_eq!(*INTEGER(result).add(i), NA_INTEGER);
        }
    }
}

#[test]
fn test_transpose_non_square_matrix_uses_r_column_major_indexing() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        crate::sexp::init::initialize_r();
        let data = Rf_allocVector3(SEXPTYPE::INTSXP, 6);
        let _data_guard = protect(data);
        for i in 0..6 {
            *INTEGER(data).add(i) = (i + 1) as c_int;
        }
        let matrix_args = test_pairlist(&[
            data,
            Rf_ScalarInteger(2),
            Rf_ScalarInteger(3),
            Rf_ScalarLogical(FALSE),
        ]);
        let matrix = do_matrix(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            matrix_args,
            std::ptr::null_mut(),
        );
        let transpose_args = Rf_cons(matrix, R_NilValue());

        let result = do_transpose(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            transpose_args,
            std::ptr::null_mut(),
        );

        let dim = crate::sexp::attrib_core::getAttrib(result, Rf_install(c"dim".as_ptr()));
        assert_eq!(*INTEGER(dim), 3);
        assert_eq!(*INTEGER(dim).add(1), 2);
        assert_eq!(
            (0..6).map(|i| *INTEGER(result).add(i)).collect::<Vec<_>>(),
            vec![1, 3, 5, 2, 4, 6]
        );
    }
}

#[test]
fn test_string_matrix_and_transpose_preserve_elements() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        crate::sexp::init::initialize_r();
        let data = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        let _data_guard = protect(data);
        SET_STRING_ELT(data, 0, Rf_mkChar(c"a".as_ptr()));
        SET_STRING_ELT(data, 1, Rf_mkChar(c"b".as_ptr()));
        SET_STRING_ELT(data, 2, Rf_mkChar(c"c".as_ptr()));
        let args = test_pairlist(&[
            data,
            Rf_ScalarInteger(1),
            Rf_ScalarInteger(3),
            Rf_ScalarLogical(FALSE),
        ]);
        let matrix = do_matrix(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            args,
            std::ptr::null_mut(),
        );
        assert_eq!(CStr::from_ptr(CHAR(STRING_ELT(matrix, 2))).to_bytes(), b"c");

        let transpose = do_transpose(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            Rf_cons(matrix, R_NilValue()),
            std::ptr::null_mut(),
        );
        assert_eq!(
            CStr::from_ptr(CHAR(STRING_ELT(transpose, 1))).to_bytes(),
            b"b"
        );
        let dim = crate::sexp::attrib_core::getAttrib(transpose, Rf_install(c"dim".as_ptr()));
        assert_eq!(*INTEGER(dim), 3);
        assert_eq!(*INTEGER(dim).add(1), 1);
    }
}

#[test]
fn test_system_command_policy_defaults_to_disabled_without_session() {
    assert!(system_commands_disabled_by_runtime_policy());
}

#[test]
fn test_eager_lazy_load_package_db_populates_environment() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let temp_dir = std::env::temp_dir().join(format!("rport-lazyload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("R")).expect("lazyload test dir");

        let filebase = temp_dir.join("R").join("demo");
        let rdb_path = filebase.with_extension("rdb");
        let rdx_path = filebase.with_extension("rdx");

        let value = Rf_ScalarInteger(77);
        let insert_args = Rf_cons(
            value,
            Rf_cons(
                Rf_mkString(
                    CString::new(rdb_path.to_string_lossy().into_owned())
                        .unwrap_or_default()
                        .as_ptr(),
                ),
                Rf_cons(
                    Rf_ScalarLogical(0),
                    Rf_cons(Rf_ScalarInteger(0), Rf_cons(R_NilValue(), R_NilValue())),
                ),
            ),
        );
        let key = crate::mainutils::serialize::do_lazyLoadDBinsertValue(
            R_NilValue(),
            R_NilValue(),
            insert_args,
            R_NilValue(),
        );

        let variables = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        SET_VECTOR_ELT(variables, 0, key);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        SET_STRING_ELT(names, 0, Rf_mkChar(c"hello".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            variables,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );

        let map = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        SET_VECTOR_ELT(map, 0, variables);
        SET_VECTOR_ELT(map, 1, Rf_ScalarInteger(0));
        let map_names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        SET_STRING_ELT(map_names, 0, Rf_mkChar(c"variables".as_ptr()));
        SET_STRING_ELT(map_names, 1, Rf_mkChar(c"compressed".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            map,
            crate::sexp::attrib_core::R_NamesSymbol(),
            map_names,
        );

        let save_args = Rf_cons(
            map,
            Rf_cons(
                Rf_mkString(
                    CString::new(rdx_path.to_string_lossy().into_owned())
                        .unwrap_or_default()
                        .as_ptr(),
                ),
                R_NilValue(),
            ),
        );
        do_saveRDS(R_NilValue(), R_NilValue(), save_args, R_NilValue());

        let package_env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_BaseEnv(),
            R_NilValue(),
        );
        eager_lazy_load_package_db(&filebase, package_env, &[]).expect("lazy load");

        let hello_sym = Rf_install(c"hello".as_ptr());
        let loaded = crate::sexp::envir::R_findVarInFrame(package_env, hello_sym);
        assert_eq!(TYPEOF(loaded), SEXPTYPE::PROMSXP);
        let value = crate::sexp::envir::forcePromise(loaded);
        assert_eq!(TYPEOF(value), SEXPTYPE::INTSXP);
        assert_eq!(*INTEGER(value), 77);

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
