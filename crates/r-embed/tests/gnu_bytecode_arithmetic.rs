use r_embed::RSession;

const FIXTURES: [(&str, &[u8]); 10] = [
    (
        "+",
        include_bytes!("fixtures/gnu-bytecode-arithmetic/add.rds"),
    ),
    (
        "-",
        include_bytes!("fixtures/gnu-bytecode-arithmetic/subtract.rds"),
    ),
    (
        "*",
        include_bytes!("fixtures/gnu-bytecode-arithmetic/multiply.rds"),
    ),
    (
        "/",
        include_bytes!("fixtures/gnu-bytecode-arithmetic/divide.rds"),
    ),
    (
        "==",
        include_bytes!("fixtures/gnu-bytecode-arithmetic/equal.rds"),
    ),
    (
        "!=",
        include_bytes!("fixtures/gnu-bytecode-arithmetic/unequal.rds"),
    ),
    (
        "<",
        include_bytes!("fixtures/gnu-bytecode-arithmetic/less.rds"),
    ),
    (
        "<=",
        include_bytes!("fixtures/gnu-bytecode-arithmetic/less_equal.rds"),
    ),
    (
        ">=",
        include_bytes!("fixtures/gnu-bytecode-arithmetic/greater_equal.rds"),
    ),
    (
        ">",
        include_bytes!("fixtures/gnu-bytecode-arithmetic/greater.rds"),
    ),
];

fn load(session: &mut RSession, bytes: &[u8]) {
    let raw = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    session
        .eval(&format!("f <- unserialize(as.raw(c({raw})))"))
        .unwrap();
}

#[test]
fn compiled_operators_preserve_vectors_missing_values_and_attributes() {
    let mut session = RSession::new().unwrap();
    for (op, bytes) in FIXTURES {
        load(&mut session, bytes);
        for (x, y) in [
            ("c(1L, NA_integer_, 3L)", "2L"),
            ("c(NA_real_, NaN, Inf, -Inf, 0)", "c(1, 2, Inf, 0, 0)"),
            ("setNames(c(1,2), c('a','b'))", "2"),
            ("numeric(0)", "2"),
            ("matrix(1:4, 2)", "2L"),
        ] {
            let code = format!("x<-{x};y<-{y};identical(f(x,y), x {op} y)");
            assert_eq!(
                session.eval(&code).unwrap().trim(),
                "[1] TRUE",
                "{op}: {code}"
            );
        }
    }
    load(&mut session, FIXTURES[0].1);
    assert_eq!(
        session
            .eval("suppressWarnings(identical(f(2147483647L, 1L), NA_integer_))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval(
                "Ops.foo<-function(e1,e2) {gc(); 42L};identical(f(structure(1,class='foo'),2),42L)"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    load(&mut session, FIXTURES[6].1);
    assert_eq!(session.eval("Ops.foo<-function(e1,e2) {gc(); TRUE};identical(f(structure(1,class='foo'),2),TRUE)").unwrap().trim(), "[1] TRUE");
}

#[test]
fn arithmetic_instruction_overrides_retained_source_and_survives_serialization() {
    let mut bytes = FIXTURES[0].1.to_vec();
    let stream = [12_i32, 20, 1, 20, 2, 44, 0, 1];
    let encoded = stream
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect::<Vec<_>>();
    let offsets = bytes
        .windows(encoded.len())
        .enumerate()
        .filter_map(|(i, data)| (data == encoded).then_some(i))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 1);
    // ADD -> SUB, while the retained expression still says x + y.
    let offset = offsets[0] + 5 * 4;
    bytes[offset..offset + 4].copy_from_slice(&45_i32.to_be_bytes());
    let mut session = RSession::new().unwrap();
    load(&mut session, &bytes);
    assert_eq!(
        session
            .eval("identical(f(c(8L,10L),3L),c(5L,7L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));identical(g(8L,3L),5L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    // Point the operator's call operand at the symbol x instead of a call.
    bytes[offset + 4..offset + 8].copy_from_slice(&1_i32.to_be_bytes());
    load(&mut session, &bytes);
    assert!(session.eval("f(8L,3L)").is_err());
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
