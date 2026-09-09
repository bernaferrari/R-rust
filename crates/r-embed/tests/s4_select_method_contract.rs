use r_embed::RSession;

#[test]
fn select_method_returns_inherited_method_definition_metadata() {
    // Pinned GNU R returns a MethodDefinition whose defined signature is A,
    // target signature is B, and whose body still dispatches successfully.
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                setClass('A'); setClass('B', contains='A');
                setGeneric('f', function(x) standardGeneric('f'));
                setMethod('f', 'A', function(x) 'A');
                m <- selectMethod('f', 'B');
                paste(class(m), as.character(m@defined),
                      as.character(m@target), m@.Data(new('B')), sep='|')
            })",
        )
        .unwrap();
    assert!(
        result.contains("MethodDefinition|A|B|A"),
        "GNU selectMethod contract expected: {result}"
    );
}

#[test]
fn select_method_respects_inheritance_and_named_multidispatch() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
        setClass('A'); setClass('B', contains='A');
        setGeneric('f', function(x,y) standardGeneric('f'));
        setMethod('f', c('A','numeric'), function(x,y) y+1);
        m <- selectMethod('f', c(y='B',x='numeric'));
        c(m(new('B'), 41)==42,
          is.null(selectMethod('f', c('B','numeric'), optional=TRUE, useInherited=FALSE)),
          is.function(selectMethod('f', c('A','numeric'), useInherited=FALSE)))
    })",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE");
}
