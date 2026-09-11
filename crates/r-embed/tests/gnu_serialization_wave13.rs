//! GNU serialization wave 13: public `refhook` dispatch.
//!
//! GNU R invokes `serialize()`'s `refhook` for an environment before writing
//! it. The current entry point accepts the argument but drops it, so this
//! side-effect probe exposes the missing callback without relying on bytes.

use r_embed::RSession;

#[test]
fn serialize_invokes_refhook_for_environment() {
    let mut session = RSession::new().unwrap();
    let value = session
        .eval(
            "local({e<-new.env();hits<-0L;raw<-serialize(e,NULL,refhook=function(x){hits<<-hits+1L;\"x\"});identical(hits,1L)&&is.raw(raw)})",
        )
        .unwrap();
    assert_eq!(value.trim(), "[1] TRUE");
}

#[test]
fn serialize_explicit_null_roundtrips_and_missing_arguments_still_error() {
    let mut session = RSession::new().unwrap();
    let value = session
        .eval(
            "local({ok<-identical(unserialize(serialize(NULL,NULL)),NULL);mo<-tryCatch(serialize(connection=NULL),error=function(e)grepl('object',conditionMessage(e),fixed=TRUE));mc<-tryCatch(serialize(object=NULL),error=function(e)grepl('connection',conditionMessage(e),fixed=TRUE));ok&&mo&&mc})",
        )
        .unwrap();
    assert_eq!(value.trim(), "[1] TRUE");
}
