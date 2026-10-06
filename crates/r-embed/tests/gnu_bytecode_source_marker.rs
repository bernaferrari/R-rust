use r_embed::{RSession, RuntimePathPolicy};

const FIXTURE: &[u8] = include_bytes!("fixtures/gnu-bytecode-source-marker/branch.rds");
const WORDS: [i32; 14] = [12, 17, 4, 20, 2, 3, 3, 11, 16, 4, 1, 16, 5, 1];

fn replace_word(index: usize, value: i32) -> Vec<u8> {
    let stream: Vec<_> = WORDS.iter().flat_map(|word| word.to_be_bytes()).collect();
    let offsets: Vec<_> = FIXTURE
        .windows(stream.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == stream).then_some(offset))
        .collect();
    assert_eq!(
        offsets.len(),
        1,
        "fixture must contain one instruction stream"
    );
    let mut bytes = FIXTURE.to_vec();
    let start = offsets[0] + index * 4;
    bytes[start..start + 4].copy_from_slice(&value.to_be_bytes());
    bytes
}

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
fn source_symbol_cannot_override_gnu_instructions_or_validation() {
    let mut session = RSession::new().unwrap();
    load(&mut session, FIXTURE);
    assert_eq!(session.eval("f(TRUE)").unwrap().trim(), "[1] 41");

    // The retained source still returns 41 for TRUE and mentions C_modelframe.
    // Redirect LDCONST to the false branch's constant; instructions must win.
    load(&mut session, &replace_word(9, 5));
    assert_eq!(session.eval("f(TRUE)").unwrap().trim(), "[1] 42");
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));g(TRUE)")
            .unwrap()
            .trim(),
        "[1] 42"
    );

    // An out-of-range constant is invalid even with that symbol in the source.
    let malformed = replace_word(9, 9999);
    let raw = malformed
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    assert!(
        session
            .eval(&format!("f<-unserialize(as.raw(c({raw})));f(TRUE)"))
            .is_err()
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");

    // Removing a compiler workaround must preserve the ordinary statistics
    // path that originally motivated it.
    assert_eq!(
        session
            .eval("nrow(model.frame(y~x,data.frame(x=1:3,y=2:4)))")
            .unwrap()
            .trim(),
        "[1] 3"
    );
}

#[test]
fn imported_source_edits_preserve_compiled_constants_and_closure_aliases() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!("fixtures/gnu-compiled-source-edit-contract.R"))
                .unwrap(),
            include_str!("fixtures/gnu-compiled-source-edit-contract.out")
        );
    }
}
