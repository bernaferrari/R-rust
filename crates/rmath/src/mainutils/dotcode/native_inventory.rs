//! Explicit registration export through the production package-scoped resolver.
//! This inventories descriptors without invoking a native handler or R program.

use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};

use crate::mainutils::native_routines::PayloadArity;
use crate::mainutils::native_routines::buffers::BufferInterface;

#[test]
#[ignore = "requires authenticated census input and a new resolver output path"]
fn export_native_registration_inventory() {
    let input_path = std::env::var_os("RPORT_NATIVE_CENSUS_INPUT")
        .expect("RPORT_NATIVE_CENSUS_INPUT is required");
    let output_path = std::env::var_os("RPORT_NATIVE_RESOLVER_OUTPUT")
        .expect("RPORT_NATIVE_RESOLVER_OUTPUT is required");
    let input = std::fs::read_to_string(input_path).expect("read census probe input");
    let mut rows = input.lines();
    assert_eq!(rows.next(), Some("dll\tinterface\tname\tnum_parameters"));
    let mut seen = BTreeSet::new();
    let mut records = Vec::new();
    for row in rows {
        let fields: Vec<_> = row.split('\t').collect();
        let [package, requested, name, expected] = fields.as_slice() else {
            panic!("census row must contain four fields");
        };
        assert!(!package.is_empty() && !name.is_empty());
        let expected_count: i32 = expected.parse().expect("GNU registered payload count");
        assert!(expected_count >= -1);
        assert!(
            seen.insert((*package, *requested, *name)),
            "duplicate census key"
        );
        let metadata = match *requested {
            ".Call" | ".External" => {
                super::lookup_bundled_native(name, Some(package)).map(|routine| {
                    let (kind, count) = match routine.payload_arity() {
                        PayloadArity::Fixed(count) => ("fixed", count.to_string()),
                        PayloadArity::Variadic => ("variadic", "-1".to_string()),
                    };
                    (routine.interface().to_string(), kind, count)
                })
            }
            ".C" | ".Fortran" => crate::library::tools::native_calls::lookup_buffer(name)
                .filter(|routine| routine.package() == *package)
                .map(|routine| {
                    let interface = match routine.interface() {
                        BufferInterface::C => ".C",
                        BufferInterface::Fortran => ".Fortran",
                    };
                    (interface.to_string(), "fixed", routine.arity().to_string())
                }),
            _ => panic!("unknown GNU native registration interface"),
        };
        let (status, interface, kind, count) = metadata.map_or_else(
            || ("unsupported", String::new(), "", String::new()),
            |(interface, kind, count)| ("resolved", interface, kind, count),
        );
        records.push(format!("{row}\t{status}\t{interface}\t{kind}\t{count}\n"));
    }
    assert!(
        !records.is_empty(),
        "empty native census cannot establish resolver coverage"
    );
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)
        .expect("resolver output path must be new");
    let mut output = BufWriter::new(file);
    output
        .write_all(b"dll\tinterface\tname\tnum_parameters\tresolver_status\tactual_interface\tactual_arity_kind\tactual_num_parameters\n")
        .expect("write resolver header");
    for row in &records {
        output
            .write_all(row.as_bytes())
            .expect("write resolver row");
    }
    output.flush().expect("finish complete resolver export");
    println!(
        "Exported {} native registration descriptors without invocation",
        records.len()
    );
}
