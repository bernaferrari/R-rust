//! One-shot asset migration harness: converts GNU-format lazy-load records in
//! the bundled package databases to RBG1 frames. Opt-in: run with
//! `RBG1_CONVERT=1 cargo test -p r-embed --test convert_assets`.
//! Writes converted blobs and regenerated inventories into the source tree.

use rmath::android::{RSession, RuntimePathPolicy};

fn field<'a>(entry: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\": \"");
    let start = entry.find(&needle)? + needle.len();
    let rest = &entry[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn parse_inventory(json: &str) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    // Entries are flat objects inside the "objects" array.
    let objects_pos = json.find("\"objects\"").expect("objects key");
    let array_start = json[objects_pos..].find('[').expect("objects array") + objects_pos;
    let mut rest = &json[array_start..];
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}').expect("entry end") + open;
        let entry = &rest[open..=close];
        let name = field(entry, "name").expect("name").to_string();
        let offset: usize = field(entry, "offset").expect("offset").parse().expect("offset num");
        let bytes: usize = field(entry, "bytes").expect("bytes").parse().expect("bytes num");
        out.push((name, offset, bytes));
        rest = &rest[close + 1..];
        if rest.trim_start().starts_with(']') {
            break;
        }
    }
    out
}

#[test]
fn convert_datasets() {
    if std::env::var("RBG1_CONVERT").is_err() {
        eprintln!("opt-in conversion harness: set RBG1_CONVERT=1 to run");
        return;
    }
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"));
    // Force the runtime fully live before conversion.
    eprintln!("booting");
    let _ = session.eval("invisible(1+1)");
    eprintln!("booted");

    let blob = include_bytes!("../../rmath/src/library/datasets/assets/Rdata.rdb");
    let inventory_json = std::str::from_utf8(include_bytes!(
        "../../rmath/src/library/datasets/assets/inventory.json"
    ))
    .unwrap();
    let entries = parse_inventory(inventory_json);
    assert_eq!(entries.len(), 108, "pinned datasets inventory");
    eprintln!("inventory parsed");

    let mut new_blob: Vec<u8> = Vec::new();
    let mut new_inventory: Vec<(String, usize, usize)> = Vec::new();
    for (name, offset, len) in &entries {
        let record = &blob[*offset..*offset + *len];
        if new_inventory.is_empty() { eprintln!("first record: {} bytes", record.len()); }
        let record_vec = record.to_vec();
        let converted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.convert_lazy_record_to_rbg1(&record_vec, 3)
        }))
        .map_err(|payload| {
            if let Some(e) = payload.downcast_ref::<rmath::RError>() { e.message.clone() }
            else if let Some(s) = payload.downcast_ref::<String>() { s.clone() }
            else if let Some(s) = payload.downcast_ref::<&str>() { s.to_string() }
            else { "non-string panic payload".to_string() }
        })
        .and_then(|r| r)
        .unwrap_or_else(|message| panic!("conversion of {name} failed: {message}"));
        let new_offset = new_blob.len();
        new_blob.extend_from_slice(&converted);
        new_inventory.push((name.clone(), new_offset, converted.len()));
        eprintln!("converted {name}: {len} -> {}", converted.len());
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("rmath").join("src")
        .join("library").join("datasets");
    std::fs::write(root.join("assets").join("Rdata.rdb"), &new_blob).unwrap();

    // Preserve the data-dependent TOPICS/INDEX tables verbatim; only OBJECTS
    // offsets change with conversion.
    let prior = std::fs::read_to_string(root.join("inventory.rs")).unwrap();
    let topics_pos = prior.find("pub(super) const TOPICS").expect("TOPICS table");
    let tail = &prior[topics_pos..];
    let mut source = String::from(
        "//! Literal key/topic inventory from the authenticated pinned datasets export.\n\
         //! Regenerated for the RBG1 migration: offsets into the RBG1-frame database.\n\
         pub(super) const OBJECTS: &[(&str, i32, i32)] = &[\n",
    );
    for (name, offset, len) in &new_inventory {
        source.push_str(&format!("    (\"{name}\", {offset}, {len}),\n"));
    }
    source.push_str("];\n");
    source.push_str(tail);
    std::fs::write(root.join("inventory.rs"), source).unwrap();
    // ---------------------------------------------------------------
    // Index conversion: the .rdx lazy index embeds per-record offsets and
    // must be rewritten; the other metadata files are format-converted.
    // All reading goes through the legacy GNU reader, writing through RBG1.
    // ---------------------------------------------------------------
    let assets = root.join("assets");
    for f in ["Rdata.rdx", "Rdata.rds", "data-index.rds"] {
        let bytes = std::fs::read(assets.join(f)).unwrap();
        std::fs::write(format!("/tmp/conv-in-{f}"), &bytes).unwrap();
    }
    let mut mapping = String::from("newkeys<-list(");
    for (i, (name, offset, len)) in new_inventory.iter().enumerate() {
        if i > 0 { mapping.push(','); }
        let safe = name.replace('\'', "");
        mapping.push_str(&format!("`{safe}`=c({offset}L,{len}L)"));
    }
    mapping.push(')');
    let script = format!(
        "{mapping};         idx<-readRDS('/tmp/conv-in-Rdata.rdx');         for(nm in names(idx$variables))idx$variables[[nm]]<-as.integer(newkeys[[nm]]);         saveRDS(idx,'/tmp/conv-out-Rdata.rdx');         x<-readRDS('/tmp/conv-in-Rdata.rds');saveRDS(x,'/tmp/conv-out-Rdata.rds');         d<-readRDS('/tmp/conv-in-data-index.rds');saveRDS(d,'/tmp/conv-out-data-index.rds');         'indexes-converted'"
    );
    let result = session.eval(&script);
    let rendered = format!("{:?}", result.typed);
    assert!(rendered.contains("indexes-converted"), "index conversion failed: {} {:?}", result.output, result.typed);
    for f in ["Rdata.rdx", "Rdata.rds", "data-index.rds"] {
        let converted = std::fs::read(format!("/tmp/conv-out-{f}"))
            .unwrap_or_else(|e| panic!("missing converted {f}: {e}"));
        std::fs::write(assets.join(f), &converted).unwrap();
        eprintln!("converted index {f}: {} bytes", converted.len());
    }
    eprintln!("datasets conversion complete: {} objects", new_inventory.len());
}
