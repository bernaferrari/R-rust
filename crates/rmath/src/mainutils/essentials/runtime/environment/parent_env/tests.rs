use crate::sexp::RSession;

#[test]
fn owned_parent_environment_admission_matches_independent_gnu() {
    let mut session = RSession::new_without_default_packages();
    for line in include_str!("gnu-r90451.tsv").lines().skip(1) {
        let (expression, expected) = line.split_once('\t').unwrap();
        let code = format!(
            "tryCatch({{value <- {expression}; if(is.logical(value)) as.character(value) else 'OK'}}, error=function(e) conditionMessage(e))"
        );
        let (value, _, _) = session.eval_code_with_output_capture(&code);
        let actual = value
            .unwrap_or_else(|error| panic!("{expression}: {}", error.message))
            .try_string_elt(0)
            .unwrap()
            .try_as_string()
            .unwrap();
        assert_eq!(actual, expected, "{expression}");
    }
}

#[test]
fn owned_base_bootstrap_seals_original_shared_bindings() {
    let mut session = RSession::new_without_default_packages();
    let (value, _, _) = session.eval_code_with_output_capture("environmentIsLocked(baseenv()) && environmentIsLocked(asNamespace('base')) && bindingIsLocked('sum', baseenv()) && bindingIsLocked('sum', asNamespace('base')) && !bindingIsLocked('.Device', baseenv()) && !bindingIsLocked('.Devices', baseenv())");
    assert_eq!(value.unwrap().try_logical_elt(0).unwrap(), 1);
}
