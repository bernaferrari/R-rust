use r_embed::RSession;

#[test]
fn multidispatch_uses_componentwise_inheritance_distance() {
    // GNU R reports both methods as valid for C#D and deterministically picks
    // C#B.  The candidates trade distance between arguments, so a scalar sum
    // must not be used as the dispatch relation.
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                setClass('A'); setClass('B');
                setClass('C', contains='A'); setClass('D', contains='B');
                setGeneric('f', function(x,y) standardGeneric('f'));
                setMethod('f', c('A','D'), function(x,y) 'AD');
                setMethod('f', c('C','B'), function(x,y) 'CB');
                f(new('C'), new('D'))
            })",
        )
        .unwrap();
    assert!(
        result.contains("CB"),
        "GNU-compatible choice expected: {result}"
    );
}

#[test]
fn multidispatch_prefers_non_dominated_inherited_method() {
    // The (X,B) and (A,Y2) methods are incomparable in the dispatch order.
    // GNU R chooses the nearer aggregate match A#Y2 for X#Y3.
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                setClass('A'); setClass('B'); setClass('X', contains='A');
                setClass('Y1', contains='B'); setClass('Y2', contains='Y1');
                setClass('Y3', contains='Y2');
                setGeneric('g', function(x,y) standardGeneric('g'));
                setMethod('g', c('X','B'), function(x,y) 'XB');
                setMethod('g', c('A','Y2'), function(x,y) 'AY2');
                g(new('X'), new('Y3'))
            })",
        )
        .unwrap();
    assert!(
        result.contains("AY2"),
        "GNU-compatible choice expected: {result}"
    );
}

#[test]
fn subclass_method_wins_despite_shortcut_to_ancestor() {
    let mut session = RSession::new().unwrap();
    let result = session.eval("local({setClass('A');setClass('B',contains='A');setClass('C',contains='B');setClass('D',contains=c('A','C'));setGeneric('f',function(x)standardGeneric('f'));setMethod('f','C',function(x)'C');setMethod('f','A',function(x)'A');f(new('D'))})").unwrap();
    assert_eq!(result.trim(), "[1] \"C\"");
}
