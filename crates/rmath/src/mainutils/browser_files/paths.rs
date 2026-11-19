use super::{BrowserFileStore, MAX_FILE_COUNT, MAX_FILE_PATH_BYTES, MAX_SESSION_BYTES, StoredFile};
use std::borrow::Cow;
use std::rc::Rc;

const MAX_DIRECTORY_COUNT: usize = 512;

fn descendant<'a>(path: &'a str, directory: &str) -> Option<&'a str> {
    if directory.is_empty() {
        return (!path.is_empty()).then_some(path);
    }
    path.strip_prefix(directory)?.strip_prefix('/')
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

impl BrowserFileStore {
    pub(super) fn resolve<'a>(&self, path: &'a str) -> Result<Cow<'a, str>, String> {
        let absolute = path.starts_with('/');
        let trimmed = path.strip_prefix('/').unwrap_or(path);
        if (absolute || self.cwd.is_empty()) && Self::validate_path(trimmed).is_ok() {
            return Ok(Cow::Borrowed(trimmed));
        }
        if path.is_empty()
            || trimmed.len() > MAX_FILE_PATH_BYTES
            || path
                .bytes()
                .any(|byte| byte < 0x20 || matches!(byte, b'\\' | b':'))
        {
            return Err("invalid browser file path".into());
        }
        let mut parts = Vec::new();
        if !absolute && !self.cwd.is_empty() {
            parts.extend(self.cwd.split('/'));
        }
        for part in trimmed.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if !self.directory(&parts.join("/")) {
                        return Err("browser parent path is not a directory".into());
                    }
                    parts
                        .pop()
                        .ok_or("browser path goes above the session root")?;
                }
                _ => parts.push(part),
            }
        }
        let key = parts.join("/");
        if !key.is_empty() {
            Self::validate_path(&key)?;
        }
        Ok(Cow::Owned(key))
    }

    pub(super) fn directory(&self, key: &str) -> bool {
        key.is_empty() || self.directories.contains(key)
    }

    pub(super) fn working_directory(&self) -> String {
        format!("/{}", self.cwd)
    }

    pub(super) fn change_directory(&mut self, path: &str) -> Result<String, String> {
        let key = self.resolve(path)?.into_owned();
        if !self.directory(&key) {
            return Err(format!("cannot change working directory to '{path}'"));
        }
        let old = self.working_directory();
        self.cwd = key;
        Ok(old)
    }

    pub(super) fn missing_directories(&self, directory: &str) -> Result<Vec<String>, String> {
        let mut missing = Vec::new();
        let mut key = directory;
        while !self.directory(key) {
            if self.contains(key) {
                return Err(format!("'{key}' is a browser file"));
            }
            missing.push(key.to_owned());
            key = parent(key);
        }
        Ok(missing)
    }

    pub(super) fn reserve_directories(&mut self, additional: usize) -> Result<(), String> {
        if self.directories.len() + additional > MAX_DIRECTORY_COUNT {
            return Err(format!(
                "browser file store exceeds {MAX_DIRECTORY_COUNT} directory limit"
            ));
        }
        self.directories
            .try_reserve(additional)
            .map_err(|_| "could not allocate browser directory entries".into())
    }

    pub(super) fn create_directory(&mut self, path: &str, recursive: bool) -> Result<(), String> {
        let key = self.resolve(path)?.into_owned();
        let missing = self.missing_directories(&key)?;
        if missing.is_empty() {
            return Err(format!("'{path}' already exists"));
        }
        if !recursive && missing.len() != 1 {
            return Err(format!("parent directory of '{path}' does not exist"));
        }
        self.reserve_directories(missing.len())?;
        self.directories.extend(missing);
        Ok(())
    }

    pub(super) fn require_parent(&self, key: &str) -> Result<(), String> {
        if !self.directory(parent(key)) {
            return Err("browser parent directory does not exist".into());
        }
        Ok(())
    }

    pub(super) fn entries(&self, path: &str) -> Option<Vec<(String, bool)>> {
        let key = self.resolve(path).ok()?;
        if !self.directory(&key) {
            return None;
        }
        Some(
            self.directories
                .iter()
                .map(|name| (name, true))
                .chain(self.files.keys().map(|name| (name, false)))
                .filter_map(|(name, directory)| {
                    descendant(name, &key).map(|relative| (relative.to_owned(), directory))
                })
                .collect(),
        )
    }

    pub(super) fn unlink(&mut self, path: &str, recursive: bool) -> bool {
        let Ok(key) = self.resolve(path).map(Cow::into_owned) else {
            return false;
        };
        if !self.directory(&key) {
            self.remove(&key);
            return true;
        }
        if key.is_empty() || key == self.cwd || descendant(&self.cwd, &key).is_some() {
            return false;
        }
        if !recursive {
            return false;
        }

        self.directories
            .retain(|name| name != &key && descendant(name, &key).is_none());
        let mut removed = 0;
        self.files.retain(|name, file| {
            if descendant(name, &key).is_some() {
                removed += file.bytes.len();
                return false;
            }
            true
        });
        self.total_bytes -= removed;
        true
    }

    pub(super) fn copy_into_directory(
        &mut self,
        from: &str,
        directory: &str,
        overwrite: bool,
        recursive: bool,
        copy_mode: bool,
        copy_date: bool,
    ) -> Result<(), String> {
        let source = self.resolve(from)?.into_owned();
        let directory = self.resolve(directory)?.into_owned();
        if !self.directory(&directory) {
            return Err("destination is not a browser directory".into());
        }
        let basename = source
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .ok_or("cannot copy the browser root directory")?;
        let target = if directory.is_empty() {
            basename.to_owned()
        } else {
            format!("{directory}/{basename}")
        };
        if target == source || descendant(&target, &source).is_some() {
            return Err("cannot copy a browser path onto or inside itself".into());
        }

        let mut directories = Vec::new();
        let mut files = Vec::new();
        if self.directory(&source) {
            if !recursive {
                return Err("directories are omitted unless 'recursive = TRUE'".into());
            }
            directories.push(target.clone());
            for name in &self.directories {
                if let Some(relative) = descendant(name, &source) {
                    directories.push(format!("{target}/{relative}"));
                }
            }
            for (name, file) in &self.files {
                if let Some(relative) = descendant(name, &source) {
                    files.push((format!("{target}/{relative}"), file));
                }
            }
        } else {
            files.push((
                target,
                self.files.get(&source).ok_or("browser file not found")?,
            ));
        }

        let mut conflicts = Vec::new();
        for path in &directories {
            Self::validate_path(path)?;
            if self.contains(path) {
                conflicts.push(path.clone());
            }
        }
        let mut incomplete = !conflicts.is_empty();
        let blocked = |path: &str| {
            conflicts
                .iter()
                .any(|name| path == name || descendant(path, name).is_some())
        };
        directories.retain(|path| !self.directory(path) && !blocked(path));

        let now = self.now();
        let mut total = self.total_bytes;
        let mut additional = 0;
        let mut staged = Vec::with_capacity(files.len());
        for (path, file) in files {
            Self::validate_path(&path)?;
            if self.directory(&path) || blocked(&path) {
                incomplete = true;
                continue;
            }
            let ctime = if let Some(existing) = self.files.get(&path) {
                if !overwrite {
                    incomplete = true;
                    continue;
                }
                total -= existing.bytes.len();
                existing.ctime
            } else {
                additional += 1;
                now
            };
            total += file.bytes.len();
            let mtime = if copy_date { file.mtime } else { now };
            // Both paths refer to an immutable admitted file version. A later
            // write replaces the version of just the path that was written.
            let mode = if copy_mode {
                file.mode
            } else {
                self.files
                    .get(&path)
                    .map_or(0o644, |existing| existing.mode)
            };
            staged.push((
                path,
                StoredFile {
                    bytes: Rc::clone(&file.bytes),
                    mtime,
                    atime: mtime,
                    ctime,
                    mode,
                },
            ));
        }
        if total > MAX_SESSION_BYTES || self.files.len() + additional > MAX_FILE_COUNT {
            return Err("copied files exceed browser file limits".into());
        }
        self.reserve_directories(directories.len())?;
        self.files
            .try_reserve(additional)
            .map_err(|_| "could not allocate browser file entries")?;
        self.directories.extend(directories);
        self.files.extend(staged);
        self.total_bytes = total;
        if incomplete {
            Err("one or more copied browser paths already exist".into())
        } else {
            Ok(())
        }
    }
}
