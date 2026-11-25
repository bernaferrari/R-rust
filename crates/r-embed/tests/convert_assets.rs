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

fn hex_encode(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

/// Convert one portable package lazy database end to end: every record in
/// variables and references, the index keys, and the metadata rds files.
/// `package` is the asset directory under portable_package assets or the
/// methods portable assets, given as absolute paths by the caller.
fn convert_package_db(
    session: &mut RSession,
    rdb_path: &std::path::Path,
    index_path: &std::path::Path,
    mode: u8,
    extra_rds: &[std::path::PathBuf],
) {
    // Parse the index in-session: shuttle entries as hex-encoded name + key.
    std::fs::copy(index_path, "/tmp/conv-idx-in.rds").unwrap();
    let probe = session.eval(
        "x<-readRDS('/tmp/conv-idx-in.rds');\
         enc<-function(nm)paste(as.character(charToRaw(nm)),collapse='');\
         vlines<-vapply(names(x$variables),function(nm)paste(enc(nm),x$variables[[nm]][1],x$variables[[nm]][2]),'');\
         rlines<-vapply(names(x$references),function(nm)paste(enc(nm),x$references[[nm]][1],x$references[[nm]][2]),'');\
         paste('V',length(vlines),'R',length(rlines));\
         writeLines(c(vlines,'---REFS---',rlines),'/tmp/conv-keys.txt');\
         'keys-extracted'",
    );
    let rendered = format!("{:?}", probe.typed);
    assert!(rendered.contains("keys-extracted"), "key extraction failed: {} {:?}", probe.output, probe.typed);

    let keys_text = std::fs::read_to_string("/tmp/conv-keys.txt").unwrap();
    let blob = std::fs::read(rdb_path).unwrap();
    let mut new_blob: Vec<u8> = Vec::new();
    let mut keymap = String::new();
    let mut section_variables = true;
    let (mut nv, mut nr) = (0usize, 0usize);
    for line in keys_text.lines() {
        if line == "---REFS---" {
            section_variables = false;
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let hexname = parts.next().unwrap_or("");
        if hexname.is_empty() { continue; }
        let off: usize = parts.next().and_then(|v| v.parse().ok()).expect("offset");
        let len: usize = parts.next().and_then(|v| v.parse().ok()).expect("len");
        let record = &blob[off..off + len];
        let record_vec = record.to_vec();
        let converted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.convert_lazy_record_to_rbg1(&record_vec, mode)
        }))
        .map_err(|payload| {
            if let Some(e) = payload.downcast_ref::<rmath::RError>() { e.message.clone() }
            else if let Some(s) = payload.downcast_ref::<String>() { s.clone() }
            else if let Some(s) = payload.downcast_ref::<&str>() { s.to_string() }
            else { "non-string panic payload".to_string() }
        })
        .and_then(|r| r)
        .unwrap_or_else(|message| panic!("conversion of {hexname} failed: {message}"));
        let new_offset = new_blob.len();
        new_blob.extend_from_slice(&converted);
        keymap.push_str(&format!("{} {} {} {}\n", hexname, new_offset, converted.len(), if section_variables { "v" } else { "r" }));
        if section_variables { nv += 1; } else { nr += 1; }
    }
    eprintln!("converted {nv} variables + {nr} references records");
    std::fs::write(rdb_path, &new_blob).unwrap();
    std::fs::write("/tmp/conv-keymap.txt", &keymap).unwrap();

    // Rewrite the index keys in-session and save as RBG1.
    let rewrite = session.eval(
        "km<-read.table('/tmp/conv-keymap.txt',header=FALSE,colClasses=c('character','integer','integer','character'));\
         idx<-readRDS('/tmp/conv-idx-in.rds');\
         iv<-which(km[[4]]=='v');ir<-which(km[[4]]=='r');\
         kv<-lapply(iv,function(i)as.integer(c(km[i,2],km[i,3])));\
         kr<-lapply(ir,function(i)as.integer(c(km[i,2],km[i,3])));\
         idx$variables[]<-kv;\
         if(length(ir))idx$references[]<-kr;\
         saveRDS(idx,'/tmp/conv-idx-out.rds');\
         paste('index-rewritten',length(kv),length(kr))",
    );
    let rendered = format!("{:?}", rewrite.typed);
    assert!(rendered.contains("index-rewritten"), "index rewrite failed: {} {:?}", rewrite.output, rewrite.typed);
    std::fs::copy("/tmp/conv-idx-out.rds", index_path).unwrap();
    eprintln!("index rewritten");

    // Format-convert metadata rds files.
    for path in extra_rds {
        if !path.exists() { continue; }
        let name = path.file_name().unwrap().to_string_lossy();
        std::fs::copy(path.as_path(), format!("/tmp/conv-meta-{name}")).unwrap();
        let script = format!(
            "x<-readRDS('/tmp/conv-meta-{name}');saveRDS(x,'/tmp/conv-meta-{name}-out');'ok-{name}'"
        );
        let r = session.eval(&script);
        let rendered = format!("{:?}", r.typed);
        assert!(rendered.contains(&format!("ok-{name}")), "conversion of {name} failed: {} {:?}", r.output, r.typed);
        std::fs::copy(format!("/tmp/conv-meta-{name}-out"), path).unwrap();
        eprintln!("converted metadata {name}");
    }
}

#[test]
fn convert_methods() {
    if std::env::var("RBG1_CONVERT").is_err() {
        eprintln!("opt-in conversion harness: set RBG1_CONVERT=1 to run");
        return;
    }
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"));
    let boot = session.eval(
        "rbg1_convert_read_hook<-function(n){e<-new.env();attr(e,'rbg1pk')<-n;e};\
         rbg1_convert_write_hook<-function(e){k<-attr(e,'rbg1pk');if(is.character(k))k else NULL};\
         'hooks-ready'",
    );
    assert!(format!("{:?}", boot.typed).contains("hooks-ready"), "hook install failed: {:?}", boot.typed);

    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..").join("rmath").join("src").join("library")
        .join("methods").join("portable").join("assets");
    convert_package_db(
        &mut session,
        &assets.join("methods.rdb"),
        &assets.join("index.rds"),
        1,
        &[assets.join("exports.rds"), assets.join("S3methods.rds")],
    );
    eprintln!("methods conversion complete");
}
