use std::fs::{self, File, FileTimes, Metadata, OpenOptions};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy)]
pub(super) struct CopyOptions {
    pub(super) overwrite: bool,
    pub(super) recursive: bool,
    pub(super) copy_mode: bool,
    pub(super) copy_date: bool,
}

pub(super) fn copy_into_directory(
    from: &Path,
    destination_dir: &Path,
    options: CopyOptions,
) -> io::Result<()> {
    #[cfg(unix)]
    let mode_mask = 0o777 & !super::current_file_creation_umask();
    #[cfg(not(unix))]
    let mode_mask = 0o777;
    copy_with_mask(from, destination_dir, options, mode_mask)
}

fn copy_with_mask(
    from: &Path,
    destination_dir: &Path,
    options: CopyOptions,
    mode_mask: u32,
) -> io::Result<()> {
    let last = from
        .as_os_str()
        .as_encoded_bytes()
        .rsplit(|&b| b == b'/' || cfg!(windows) && b == b'\\')
        .find(|part| !part.is_empty());
    let name = match last {
        Some(b".") => Path::new(".").as_os_str(),
        Some(b"..") => return Err(invalid("copy source ends with '..'")),
        _ => from
            .file_name()
            .ok_or_else(|| invalid("invalid copy source"))?,
    };
    let destination_dir = fs::canonicalize(destination_dir)?;
    if !fs::metadata(&destination_dir)?.is_dir() {
        return Err(io::Error::new(
            ErrorKind::NotADirectory,
            "copy destination is not a directory",
        ));
    }
    CopyTree {
        options,
        mode_mask,
        ancestors: Vec::new(),
    }
    .copy_entry(from, &destination_dir.join(name))
}

struct CopyTree {
    options: CopyOptions,
    mode_mask: u32,
    ancestors: Vec<PathBuf>,
}

impl CopyTree {
    fn copy_entry(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        if self.ancestors.len() >= 100 {
            return Err(invalid("too deep nesting"));
        }
        let from = fs::canonicalize(from)?;
        let source = fs::metadata(&from)?;
        let destination = match fs::symlink_metadata(to) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(conflict("copy destination is a symbolic link"));
            }
            Ok(meta) => Some(meta),
            Err(err) if err.kind() == ErrorKind::NotFound => None,
            Err(err) => return Err(err),
        };
        if let Some(existing) = &destination
            && (same_inode(&source, existing) || fs::canonicalize(to)? == from)
        {
            return Err(invalid("attempt to copy a path to itself"));
        }

        if source.is_dir() {
            self.copy_directory(&from, to, &source, destination.as_ref())
        } else if source.is_file() {
            self.copy_file(&from, to, destination.as_ref())
        } else {
            Err(invalid("copy source is not a regular file or directory"))
        }
    }

    fn copy_directory(
        &mut self,
        from: &Path,
        to: &Path,
        source: &Metadata,
        destination: Option<&Metadata>,
    ) -> io::Result<()> {
        if !self.options.recursive {
            return Err(invalid("directories require recursive copying"));
        }
        if to.starts_with(from) || self.ancestors.iter().any(|ancestor| ancestor == from) {
            return Err(invalid("attempt to copy a directory into itself"));
        }
        let entries = fs::read_dir(from)?;
        match destination {
            Some(meta) if !meta.is_dir() => {
                return Err(conflict(
                    "cannot overwrite a non-directory with a directory",
                ));
            }
            Some(_) => (),
            None => {
                let mut builder = fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(to)?;
            }
        }

        self.ancestors.push(from.to_owned());
        let mut failure = None;
        for entry in entries {
            let result =
                entry.and_then(|entry| self.copy_entry(&entry.path(), &to.join(entry.file_name())));
            if let Err(err) = result {
                failure.get_or_insert(err);
            }
        }
        self.ancestors.pop();

        if self.options.copy_date {
            open_directory(to)?.set_times(copy_times(source)?)?;
        }
        #[cfg(unix)]
        {
            let mode = if self.options.copy_mode {
                permission_bits(source)
            } else {
                0o777
            };
            fs::set_permissions(to, unix_permissions(mode & self.mode_mask))?;
        }
        failure.map_or(Ok(()), Err)
    }

    fn copy_file(&self, from: &Path, to: &Path, destination: Option<&Metadata>) -> io::Result<()> {
        if let Some(existing) = destination {
            if !self.options.overwrite || !existing.is_file() {
                return Err(conflict("copy destination already exists"));
            }
            // Rename tests directory permissions. GNU also requires that the
            // existing file be writable, so open it without truncating it.
            let mut options = OpenOptions::new();
            options.write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
            }
            if !options.open(to)?.metadata()?.is_file() {
                return Err(conflict("copy destination is not a regular file"));
            }
        }

        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // A concurrent replacement of a regular file with a FIFO must
            // not make the copy block waiting for another process.
            options.custom_flags(libc::O_NONBLOCK);
        }
        let mut input = options.open(from)?;
        let source = input.metadata()?;
        if !source.is_file() {
            return Err(invalid("copy source is not a regular file"));
        }
        let parent = to
            .parent()
            .ok_or_else(|| invalid("invalid copy destination"))?;
        let (mut output, staged) = StagedFile::create(parent)?;
        if io::copy(&mut input, &mut output)? < source.len() {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "copy source shrank while reading",
            ));
        }
        if self.options.copy_date {
            output.set_times(copy_times(&source)?)?;
        }
        #[cfg(unix)]
        {
            let mode = if self.options.copy_mode {
                permission_bits(&source) & self.mode_mask
            } else {
                destination.map_or(0o666 & self.mode_mask, permission_bits)
            };
            output.set_permissions(unix_permissions(mode))?;
        }
        output.sync_all()?;
        drop(output);
        staged.publish(to, destination.is_some())?;
        #[cfg(not(unix))]
        if let Some(permissions) = self
            .options
            .copy_mode
            .then(|| source.permissions())
            .or_else(|| destination.map(Metadata::permissions))
        {
            fs::set_permissions(to, permissions)?;
        }
        Ok(())
    }
}

fn copy_times(source: &Metadata) -> io::Result<FileTimes> {
    let time = source.modified()?;
    Ok(FileTimes::new().set_accessed(time).set_modified(time))
}

fn open_directory(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Directory handles and FILE_WRITE_ATTRIBUTES for File::set_times.
        options.custom_flags(0x0200_0000).access_mode(0x100);
    }
    options.open(path)
}

#[cfg(unix)]
fn permission_bits(meta: &Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o777
}

#[cfg(unix)]
fn unix_permissions(mode: u32) -> fs::Permissions {
    use std::os::unix::fs::PermissionsExt;
    fs::Permissions::from_mode(mode)
}

#[cfg(unix)]
fn same_inode(a: &Metadata, b: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(not(unix))]
fn same_inode(_a: &Metadata, _b: &Metadata) -> bool {
    false
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, message)
}

fn conflict(message: &'static str) -> io::Error {
    io::Error::new(ErrorKind::AlreadyExists, message)
}

struct StagedFile(Option<PathBuf>);

impl StagedFile {
    fn create(parent: &Path) -> io::Result<(File, Self)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        for _ in 0..100 {
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(".rport-copy-{}-{serial}", std::process::id()));
            match options.open(&path) {
                Ok(file) => return Ok((file, Self(Some(path)))),
                Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err),
            }
        }
        Err(conflict("cannot create temporary copy file"))
    }

    fn publish(mut self, destination: &Path, replace: bool) -> io::Result<()> {
        let staged = self.0.as_ref().expect("staged copy path");
        if replace {
            fs::rename(staged, destination)?;
            self.0 = None;
            Ok(())
        } else {
            // An exclusive link publishes complete contents without replacing
            // a destination created by another writer during the copy.
            fs::hard_link(staged, destination)
        }
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const TREE: CopyOptions = CopyOptions {
        overwrite: true,
        recursive: true,
        copy_mode: true,
        copy_date: false,
    };

    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir();
            loop {
                let serial = NEXT.fetch_add(1, Ordering::Relaxed);
                let path = root.join(format!("rport-copy-test-{}-{serial}", std::process::id()));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(err) if err.kind() == ErrorKind::AlreadyExists => (),
                    Err(err) => panic!("{err}"),
                }
            }
        }

        fn directory(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(&path).unwrap();
            path
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn merges_trees_and_keeps_conflicting_entries() {
        let sandbox = Sandbox::new();
        let source = sandbox.directory("source");
        let destination = sandbox.directory("destination");
        let existing = sandbox.directory("destination/source");
        fs::write(source.join("data"), b"new").unwrap();
        fs::write(source.join("fresh"), b"fresh").unwrap();
        fs::write(existing.join("data"), b"old").unwrap();
        fs::write(existing.join("unrelated"), b"independent").unwrap();
        let keep = CopyOptions {
            overwrite: false,
            ..TREE
        };
        assert_eq!(
            copy_with_mask(&source, &destination, keep, 0o755)
                .unwrap_err()
                .kind(),
            ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(existing.join("data")).unwrap(), b"old");
        assert_eq!(fs::read(existing.join("fresh")).unwrap(), b"fresh");
        copy_with_mask(&source, &destination, TREE, 0o755).unwrap();
        assert_eq!(fs::read(existing.join("data")).unwrap(), b"new");
        assert_eq!(
            fs::read(existing.join("unrelated")).unwrap(),
            b"independent"
        );

        sandbox.directory("source/dir");
        fs::write(existing.join("dir"), b"keep file").unwrap();
        fs::write(source.join("file"), b"keep directory").unwrap();
        sandbox.directory("destination/source/file");
        assert!(copy_with_mask(&source, &destination, TREE, 0o755).is_err());
        assert_eq!(fs::read(existing.join("dir")).unwrap(), b"keep file");
        assert!(existing.join("file").is_dir());
    }

    #[test]
    fn copies_tree_and_dot_contents_but_rejects_descendants_and_self() {
        let sandbox = Sandbox::new();
        let source = sandbox.directory("source");
        let child = sandbox.directory("source/child");
        let destination = sandbox.directory("destination");
        let data = child.join("file");
        fs::write(&data, b"content").unwrap();
        let flat = CopyOptions {
            recursive: false,
            ..TREE
        };
        assert!(copy_with_mask(&source, &destination, flat, 0o755).is_err());
        assert!(!destination.join("source").exists());
        assert!(copy_with_mask(&source, &child, TREE, 0o755).is_err());
        assert!(!child.join("source").exists());
        assert!(copy_with_mask(&data, &child, TREE, 0o755).is_err());
        assert_eq!(fs::read(&data).unwrap(), b"content");
        copy_with_mask(&source, &destination, TREE, 0o755).unwrap();
        assert_eq!(
            fs::read(destination.join("source/child/file")).unwrap(),
            b"content"
        );
        copy_with_mask(&source.join("."), &destination, TREE, 0o755).unwrap();
        assert_eq!(
            fs::read(destination.join("child/file")).unwrap(),
            b"content"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dereferences_source_links_and_rejects_cycles_and_destination_links() {
        use std::os::unix::fs::symlink;

        let sandbox = Sandbox::new();
        let source = sandbox.directory("source");
        let destination = sandbox.directory("destination");
        let copied = sandbox.directory("destination/source");
        fs::write(sandbox.0.join("original"), b"original").unwrap();
        fs::write(sandbox.0.join("outside"), b"outside").unwrap();
        fs::write(source.join("protected"), b"replacement").unwrap();
        symlink(sandbox.0.join("original"), source.join("alias")).unwrap();
        symlink(&source, source.join("loop")).unwrap();
        symlink(sandbox.0.join("outside"), copied.join("protected")).unwrap();
        assert!(copy_with_mask(&source, &destination, TREE, 0o755).is_err());
        assert_eq!(fs::read(copied.join("alias")).unwrap(), b"original");
        assert!(
            !fs::symlink_metadata(copied.join("alias"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!copied.join("loop").exists());
        assert_eq!(fs::read(sandbox.0.join("outside")).unwrap(), b"outside");

        fs::hard_link(source.join("protected"), destination.join("protected")).unwrap();
        assert!(copy_with_mask(&source.join("protected"), &destination, TREE, 0o755).is_err());
        assert_eq!(
            fs::read(destination.join("protected")).unwrap(),
            b"replacement"
        );
    }

    #[cfg(unix)]
    #[test]
    fn copies_dates_and_umask_limited_permissions() {
        use std::time::{Duration, UNIX_EPOCH};

        let sandbox = Sandbox::new();
        let source = sandbox.directory("source");
        let destination = sandbox.directory("destination");
        let file = File::create(source.join("content")).unwrap();
        let time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let times = FileTimes::new().set_modified(time);
        file.set_times(times).unwrap();
        file.set_permissions(unix_permissions(0o777)).unwrap();
        File::open(&source).unwrap().set_times(times).unwrap();
        let dates = CopyOptions {
            copy_date: true,
            ..TREE
        };
        copy_with_mask(&source, &destination, dates, 0o750).unwrap();
        let path = destination.join("source/content");
        let target = fs::metadata(&path).unwrap();
        assert_eq!(permission_bits(&target), 0o750);
        assert_eq!(target.modified().unwrap(), time);
        assert_eq!(
            fs::metadata(destination.join("source"))
                .unwrap()
                .modified()
                .unwrap(),
            time
        );

        fs::set_permissions(&path, unix_permissions(0o640)).unwrap();
        let keep_mode = CopyOptions {
            copy_mode: false,
            ..TREE
        };
        copy_with_mask(&source, &destination, keep_mode, 0o755).unwrap();
        assert_eq!(permission_bits(&fs::metadata(&path).unwrap()), 0o640);
    }

    #[test]
    fn no_replace_publication_keeps_file_created_by_another_writer() {
        let sandbox = Sandbox::new();
        let (mut file, staged) = StagedFile::create(&sandbox.0).unwrap();
        file.write_all(b"copy").unwrap();
        drop(file);
        let dest = sandbox.0.join("data");
        fs::write(&dest, b"arrived first").unwrap();
        assert_eq!(
            staged.publish(&dest, false).unwrap_err().kind(),
            ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&dest).unwrap(), b"arrived first");
        assert_eq!(fs::read_dir(&sandbox.0).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_source_preserves_existing_destination() {
        use std::os::unix::net::UnixListener;

        let sandbox = Sandbox::new();
        let source = sandbox.0.join("source");
        let destination = sandbox.directory("destination");
        let _socket = UnixListener::bind(&source).unwrap();
        let existing = destination.join("source");
        fs::write(&existing, b"before copy").unwrap();
        assert_eq!(
            copy_with_mask(&source, &destination, TREE, 0o755)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(fs::read(existing).unwrap(), b"before copy");
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
    }
}
