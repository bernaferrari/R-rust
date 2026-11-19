//! Stream generated output into a staged sibling before replacing a host file.
use std::io;

#[cfg(target_arch = "wasm32")]
pub(crate) fn publish_host_file(
    _path: &str,
    _write: impl FnOnce(&mut std::fs::File) -> io::Result<()>,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "host files are unavailable",
    ))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn publish_host_file(
    path: &str,
    write: impl FnOnce(&mut std::fs::File) -> io::Result<()>,
) -> io::Result<()> {
    use std::{
        fs::{self, OpenOptions},
        path::{Path, PathBuf},
    };
    struct StagedFile(Option<PathBuf>);
    impl Drop for StagedFile {
        fn drop(&mut self) {
            if let Some(path) = &self.0 {
                let _ = fs::remove_file(path);
            }
        }
    }

    let destination = Path::new(path);
    // Keep an existing symlink and atomically replace the regular file it
    // names, as direct R writes do. A dangling symlink is recoverable.
    let resolved;
    let destination = match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            resolved = fs::canonicalize(destination)?;
            resolved.as_path()
        }
        Ok(_) => destination,
        Err(error) if error.kind() == io::ErrorKind::NotFound => destination,
        Err(error) => return Err(error),
    };
    let permissions = match fs::metadata(destination) {
        Ok(metadata) if metadata.is_file() => {
            // Opening without truncation checks the original file's write
            // permission. Renaming alone checks only its directory.
            OpenOptions::new().write(true).open(destination)?;
            Some(metadata.permissions())
        }
        Ok(_) => {
            return Err(io::Error::other("destination is not a regular file"));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = parent
        .to_str()
        .ok_or_else(|| io::Error::other("invalid destination directory"))?;
    for _ in 0..100 {
        let temporary = crate::mainutils::sysutils::R_tmpnam2(".rport-output-", parent, ".tmp")
            .ok_or_else(|| io::Error::other("cannot choose temporary output name"))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = match options.open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let mut cleanup = StagedFile(Some(PathBuf::from(temporary)));
        write(&mut file)?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.sync_all()?;
        drop(file);
        fs::rename(cleanup.0.as_ref().expect("staged path"), destination)?;
        cleanup.0 = None;
        return Ok(());
    }
    Err(io::Error::other("cannot create temporary output file"))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::{fs, io::Write, path::PathBuf};

    #[test]
    fn a_failed_producer_leaves_the_previous_file_and_reclaims_staging() {
        let scratch = std::env::temp_dir();
        let name = crate::mainutils::sysutils::R_tmpnam2(
            "rport-generated-file-",
            scratch.to_str().unwrap(),
            "",
        )
        .unwrap();
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let directory = Scratch(PathBuf::from(name));
        fs::create_dir(&directory.0).unwrap();
        let target = directory.0.join("result");
        fs::write(&target, b"existing result").unwrap();
        let path = target.to_str().unwrap();
        let failed = publish_host_file(path, |file| {
            file.write_all(b"incomplete result")?;
            Err(io::Error::other("producer error"))
        });
        assert_eq!(failed.unwrap_err().to_string(), "producer error");
        assert_eq!(fs::read(&target).unwrap(), b"existing result");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
        publish_host_file(path, |file| file.write_all(b"finished result")).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"finished result");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }
}
