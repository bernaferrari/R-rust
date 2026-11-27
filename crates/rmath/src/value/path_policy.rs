//! Host filesystem confinement — the pure world's runtime path policy.
//!
//! Embedded contexts (browser, mobile sandboxes) must not reach the host
//! filesystem outside their virtual root. The policy is a cheap pure-data
//! decision: normalize the candidate path lexically, refuse absolute paths
//! and any escape above the root, and map the survivor onto the virtual
//! root for the host operation. No filesystem access happens here — the
//! gate is deterministic exactly as the architecture's ownership rules
//! need, testable without fixtures.

/// Filesystem access policy for one session.
///
/// `allowed_roots` prefixes pass through untouched (system library paths
/// the host explicitly shares); every other path resolves inside
/// `virtual_root`. Lexical normalization resolves `.` and `..` without
/// touching the filesystem exactly as GNU's path normalization does —
/// symlinks are the host's concern, not the policy's.
#[derive(Debug, Clone, Default)]
pub struct PathPolicy {
    pub allowed_roots: Vec<String>,
    pub virtual_root: String,
}

impl PathPolicy {
    pub fn new(allowed_roots: Vec<String>, virtual_root: impl Into<String>) -> PathPolicy {
        PathPolicy {
            allowed_roots,
            virtual_root: virtual_root.into(),
        }
    }

    /// Normalize one path lexically: `.` segments drop, `..` pops the
    /// trailing segment, root-anchored paths keep their anchor. Never
    /// allocates host knowledge — purely the string's own structure.
    fn normalize(path: &str) -> (bool, Vec<String>) {
        let absolute = path.starts_with('/');
        let mut segments: Vec<String> = Vec::new();
        for segment in path.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    segments.pop();
                }
                other => segments.push(other.to_string()),
            }
        }
        (absolute, segments)
    }

    /// Whether one normalized path escapes upward past its own root —
    /// `../../etc` carries more `..` than segments at some prefix.
    fn escapes(path: &str) -> bool {
        let mut depth: i64 = 0;
        for segment in path.split('/') {
            match segment {
                "" | "." => {}
                ".." => depth -= 1,
                _ => depth += 1,
            }
            if depth < 0 {
                return true;
            }
        }
        false
    }

    /// Gate one candidate path: the host location the operation may use,
    /// or the refusal reason. Absolute paths pass only inside an allowed
    /// root; relative paths and escapes resolve inside the virtual root.
    pub fn resolve(&self, path: &str) -> Result<String, String> {
        let (absolute, segments) = PathPolicy::normalize(path);
        let normalized = segments.join("/");
        if absolute {
            let full = format!("/{normalized}");
            for root in &self.allowed_roots {
                let root_norm = {
                    let (_, root_segments) = PathPolicy::normalize(root);
                    format!("/{}", root_segments.join("/"))
                };
                if full == root_norm || full.starts_with(&format!("{root_norm}/")) {
                    return Ok(full);
                }
            }
            return Err(format!(
                "absolute path '{path}' is outside the session's permitted roots"
            ));
        }
        if PathPolicy::escapes(path) {
            return Err(format!("path '{path}' escapes the session's virtual root"));
        }
        let root = self.virtual_root.trim_end_matches('/');
        if normalized.is_empty() {
            Ok(root.to_string())
        } else {
            Ok(format!("{root}/{normalized}"))
        }
    }
}

/// The enforcement primitive for iterator-shaped file operations: the
/// resolved host path, or None when the policy refuses exactly as the
/// operation's own failure channel needs — denied removals report false,
/// denied reads error, denied writes fail, each in GNU's shape.
pub fn resolve_host_path(policy: Option<&PathPolicy>, path: &str) -> Option<String> {
    gate_host_path(policy, path).ok()
}

/// The heap-facing gate: permissive when no policy is configured exactly
/// as the native default, enforcing otherwise. Returns the path the host
/// operation may use.
pub fn gate_host_path(
    policy: Option<&PathPolicy>,
    path: &str,
) -> std::io::Result<String> {
    match policy {
        None => Ok(path.to_string()),
        Some(policy) => policy
            .resolve(path)
            .map_err(|reason| std::io::Error::new(std::io::ErrorKind::PermissionDenied, reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> PathPolicy {
        PathPolicy::new(vec!["/system/lib".to_string()], "tmp")
    }

    #[test]
    fn relative_paths_resolve_inside_virtual_root() {
        assert_eq!(policy().resolve("data/x.csv").unwrap(), "tmp/data/x.csv");
        assert_eq!(policy().resolve("x.csv").unwrap(), "tmp/x.csv");
    }

    #[test]
    fn dot_segments_normalize_lexically() {
        assert_eq!(policy().resolve("./a/../b").unwrap(), "tmp/b");
        assert_eq!(policy().resolve("a/./b/./c").unwrap(), "tmp/a/b/c");
    }

    #[test]
    fn escapes_are_refused() {
        assert!(policy().resolve("../outside").is_err());
        assert!(policy().resolve("a/../../outside").is_err());
        // An escape attempt that never leaves depth-negative ground stays.
        assert_eq!(policy().resolve("a/../b").unwrap(), "tmp/b");
    }

    #[test]
    fn absolute_paths_pass_only_inside_allowed_roots() {
        assert_eq!(
            policy().resolve("/system/lib/stats/parts").unwrap(),
            "/system/lib/stats/parts"
        );
        assert!(policy().resolve("/etc/passwd").is_err());
        assert!(policy().resolve("/system/libevil/x").is_err());
    }

    #[test]
    fn no_policy_is_permissive() {
        assert_eq!(
            gate_host_path(None, "/anywhere/at/all").unwrap(),
            "/anywhere/at/all"
        );
    }

    #[test]
    fn gated_violations_are_permission_denied() {
        let policy = policy();
        let failure = gate_host_path(Some(&policy), "/etc/passwd").unwrap_err();
        assert_eq!(failure.kind(), std::io::ErrorKind::PermissionDenied);
    }
}
