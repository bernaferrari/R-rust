use r_embed::{RSession, RuntimePathPolicy};
#[test]
fn perl_character_classes_match_gnu_default_and_ucp_profiles() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/perl-character-class-contract.R"))
            .unwrap(),
        include_str!("fixtures/perl-character-class-contract.out")
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}

#[test]
fn perl_character_classes_ucp_survives_ascii_coordinate_optimization() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    let result = session.eval(r#"p<-"(*UCP)(?<letter>\\w)"; r<-regexpr(p,"a",perl=TRUE); g<-gregexpr(p,"a",perl=TRUE)[[1L]]; e<-regexec(p,"a",perl=TRUE)[[1L]]; identical(as.integer(r),1L) && identical(as.integer(attr(r,"capture.start")),1L) && identical(attr(r,"capture.names"),"letter") && identical(as.integer(g),1L) && identical(as.integer(e),c(1L,1L))"#).unwrap();
    assert_eq!(result, "[1] TRUE\n");
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}

#[test]
fn perl_unicode_classes_preserve_public_matching_replacement_and_capture_contracts() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/perl-class-family-contract.R"))
            .unwrap(),
        "[1] TRUE\n"
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}

#[test]
fn perl_gregexpr_empty_boundary_does_not_search_again_at_terminal_offset() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    let positions = session
        .eval(r#"as.integer(gregexpr("\\b",enc2utf8("éa"),perl=TRUE)[[1L]])"#)
        .unwrap();
    assert_eq!(positions, "[1] 2\n");
}
