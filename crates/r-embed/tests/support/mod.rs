//! Isolated, reproducible real-package fixtures; never reuse a developer's /tmp tree.

pub struct PackageCorpus {
    _scratch: tempfile::TempDir,
    pub bundled: String,
    pub app: String,
    pub cache: String,
}

impl PackageCorpus {
    pub fn new() -> Self {
        let scratch = tempfile::tempdir().expect("package fixture directory");
        let bundled = std::env::var("RPORT_REAL_PKG_BUNDLED").unwrap_or_else(|_| {
            let bundled = scratch.path().join("bundled");
            std::fs::create_dir(&bundled).unwrap();
            let vendor = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/real-packages/vendor");
            let mut count = 0;
            for entry in std::fs::read_dir(vendor).expect("vendored package tarballs") {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|extension| extension == "gz") {
                    let status = std::process::Command::new("tar")
                        .arg("-xzf")
                        .arg(&path)
                        .arg("-C")
                        .arg(&bundled)
                        .status()
                        .expect("tar executable");
                    assert!(status.success(), "unpacking {} failed", path.display());
                    count += 1;
                }
            }
            assert!(count > 0, "no vendored packages");
            bundled.to_string_lossy().into_owned()
        });
        let app = std::env::var("RPORT_REAL_PKG_APP")
            .unwrap_or_else(|_| scratch.path().join("app").to_string_lossy().into_owned());
        let cache = std::env::var("RPORT_REAL_PKG_CACHE")
            .unwrap_or_else(|_| scratch.path().join("cache").to_string_lossy().into_owned());
        Self {
            _scratch: scratch,
            bundled,
            app,
            cache,
        }
    }
}
