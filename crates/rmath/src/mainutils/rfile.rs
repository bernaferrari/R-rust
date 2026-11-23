//! Open Rust files with the mode strings used by R.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub(crate) fn open_file(path: &Path, mode: &[u8]) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    let plus = mode.contains(&b'+');
    match mode.first() {
        Some(b'r') => {
            opts.read(true).write(plus);
        }
        Some(b'w') => {
            opts.write(true).read(plus).create(true).truncate(true);
        }
        Some(b'a') => {
            opts.append(true).read(plus).create(true);
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid file mode",
            ));
        }
    }
    opts.open(path)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom, Write};

    use super::*;

    #[test]
    fn mode_controls_append_truncate_and_read_access() {
        let path = std::env::temp_dir().join(format!(
            "rport-rfile-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"one").unwrap();

        let mut appended = open_file(&path, b"a+").unwrap();
        appended.write_all(b"two").unwrap();
        appended.seek(SeekFrom::Start(0)).unwrap();
        let mut text = String::new();
        appended.read_to_string(&mut text).unwrap();
        assert_eq!(text, "onetwo");
        drop(appended);

        let mut truncated = open_file(&path, b"wb+").unwrap();
        truncated.write_all(b"three").unwrap();
        truncated.seek(SeekFrom::Start(0)).unwrap();
        let mut text = String::new();
        truncated.read_to_string(&mut text).unwrap();
        assert_eq!(text, "three");
        drop(truncated);

        assert!(open_file(&path, b"q").is_err());
        let missing = path.with_extension("missing");
        assert!(open_file(&missing, b"r").is_err());

        let _ = std::fs::remove_file(&path);
    }
}
