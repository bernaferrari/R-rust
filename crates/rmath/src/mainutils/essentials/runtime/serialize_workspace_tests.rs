use super::*;
use crate::sexp::{RSession, memory::ArenaBudget, object::PairlistBuilder};

#[test]
#[cfg(not(miri))]
fn serialization_workspace_save_rds_compressors_deny_before_file_publication() {
    for codec in ["gzip", "bzip2", "xz"] {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let object = factory.strings(&["small payload"]).unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../target/serialization-workspace-evidence/denied-{codec}.rds"
        ));
        let filename = factory.strings(&[path.to_str().unwrap()]).unwrap();
        let compression = factory.strings(&[codec]).unwrap();
        let mut args = PairlistBuilder::from_factory(factory.clone());
        for value in [object, filename, factory.nil(), factory.nil(), compression] {
            args.push(value, None).unwrap();
        }
        let args = args.finish().unwrap();
        let limit = owner
            .with_arena(|a| a.total_bytes_allocated() + 128 * 1024)
            .unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
            .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            do_saveRDS(
                factory.nil().as_raw(),
                factory.nil().as_raw(),
                args.as_raw(),
                factory.nil().as_raw(),
            )
        }));
        let published = path.exists();
        if published {
            std::fs::remove_file(&path).unwrap();
        }
        assert!(
            result.is_err(),
            "{codec} must admit compressor workspace before construction"
        );
        assert!(
            !published,
            "denied {codec} must not create a successful-looking file"
        );
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
}
