use r_embed::RSession;

#[test]
fn exists_method_is_exact_while_has_method_includes_inheritance() {
    // Pinned GNU R: existsMethod checks the registered signature exactly;
    // hasMethod also finds an inherited method through B -> A.
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                setClass('A'); setClass('B', contains='A');
                setGeneric('f', function(x) standardGeneric('f'));
                setMethod('f', 'A', function(x) 'A');
                paste(existsMethod('f','A'), existsMethod('f','B'),
                      existsMethod('f','C'), hasMethod('f','A'),
                      hasMethod('f','B'), hasMethod('f','C'),
                      existsMethod('missingGeneric','A'),
                      hasMethod('missingGeneric','A'), sep='|')
            })",
        )
        .unwrap();
    assert!(
        result.contains("TRUE|FALSE|FALSE|TRUE|TRUE|FALSE|FALSE|FALSE"),
        "GNU method-existence contract expected: {result}"
    );
}

#[test]
fn method_existence_respects_argument_names_registry_and_any_defaults() {
    let mut session = RSession::new().unwrap();
    let result = session.eval("local({
        e<-new.env(); eval(quote({setGeneric('registryProbe',function(x,y)standardGeneric('registryProbe'));setMethod('registryProbe',c('numeric','character'),function(x,y)1)}),e);
        setGeneric('anyProbe',function(x)standardGeneric('anyProbe'));
        setMethod('anyProbe','ANY',function(x)1);
        c(existsMethod(signature=c(y='character',x='numeric'),f='registryProbe',where=e),
          !hasMethod('registryProbe',c(y='character',x='numeric'),where=e),
          !existsMethod('registryProbe',c('numeric','character'),where=.GlobalEnv),
          existsMethod('anyProbe'),hasMethod('anyProbe'))
    })").unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE TRUE TRUE");
}

#[test]
fn exists_method_accepts_symbol_and_closure_generic_specs() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("local({setClass('SymbolA');setGeneric('symbolProbe',function(x)standardGeneric('symbolProbe'));setMethod('symbolProbe','SymbolA',function(x)1);c(existsMethod(as.name('symbolProbe'),'SymbolA'),existsMethod(symbolProbe,'SymbolA'),hasMethod(symbolProbe,'SymbolA'))})").unwrap().trim(), "[1] TRUE TRUE TRUE");
}
