//! Virtual-file interception — the browser store semantics exactly as
//! the legacy facade specified them. When the store is enabled it IS
//! the filesystem: reads hit virtual bytes or fail NotFound without
//! host fallthrough, writes land in virtual memory rather than escaping
//! to the host. When disabled the store is write-through only for paths
//! it already holds, and everything else reaches the host through the
//! path-policy gate. One helper pair means file primitives never
//! choose between three concerns at every call site.

use crate::value::heap::Heap;
use std::io;

/// Read one path exactly as the facade's contract: virtual bytes when
/// the store holds them, NotFound when the store is enabled but misses,
/// the gated host filesystem otherwise.
pub fn read_file(heap: &Heap, path: &str) -> io::Result<Vec<u8>> {
    if let Some(bytes) = heap.browser_files.get(path) {
        return Ok(bytes.clone());
    }
    if heap.browser_files_enabled {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "browser file not found",
        ));
    }
    match crate::value::path_policy::resolve_host_path(heap.path_policy.as_ref(), path) {
        Some(host) => std::fs::read(&host),
        None => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path refused",
        )),
    }
}

/// Write one path exactly as the facade's contract: the virtual store
/// whenever it holds the path or is enabled wholesale, the gated host
/// filesystem otherwise.
pub fn write_file(heap: &mut Heap, path: &str, bytes: &[u8]) -> io::Result<()> {
    if heap.browser_files.contains_key(path) || heap.browser_files_enabled {
        heap.browser_files.insert(path.to_string(), bytes.to_vec());
        return Ok(());
    }
    match crate::value::path_policy::resolve_host_path(heap.path_policy.as_ref(), path) {
        Some(host) => std::fs::write(&host, bytes),
        None => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path refused",
        )),
    }
}

/// Existence exactly as the read contract — virtual hits exist, and
/// enabled-mode misses do not regardless of what the host holds.
pub fn file_exists(heap: &Heap, path: &str) -> bool {
    if heap.browser_files.contains_key(path) {
        return true;
    }
    if heap.browser_files_enabled {
        return false;
    }
    match crate::value::path_policy::resolve_host_path(heap.path_policy.as_ref(), path) {
        Some(host) => std::path::Path::new(&host).exists(),
        None => false,
    }
}

/// Removal exactly as the write contract — virtual entries drop, and
/// enabled mode never reaches the host.
pub fn remove_file(heap: &mut Heap, path: &str) -> io::Result<()> {
    if heap.browser_files.remove(path).is_some() {
        return Ok(());
    }
    if heap.browser_files_enabled {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "browser file not found",
        ));
    }
    match crate::value::path_policy::resolve_host_path(heap.path_policy.as_ref(), path) {
        Some(host) => std::fs::remove_file(&host),
        None => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path refused",
        )),
    }
}
