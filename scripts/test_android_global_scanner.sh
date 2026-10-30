#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/crates/rmath/src"
manifest="$fixture/manifest.tsv"
cat > "$fixture/crates/rmath/src/atomics.rs" <<'RUST'
static HEAP: AtomicU32 = AtomicU32::new(1);
static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static FLAG: AtomicBool = AtomicBool::new(false);
use std::sync::atomic::AtomicUsize as Counter;
type Alias = Counter;
static ALIASED: Alias = Counter::new(0);
unsafe extern "C" { static FOREIGN: u32; }
#[cfg(test)] mod arbitrary { static ONLY_TEST: AtomicU32 = AtomicU32::new(0); }
RUST
cd "$root"
check() {
    scripts/cargo_dev.sh run -q -p rport-android-global-audit -- --check "$fixture" "$manifest"
}
: > "$manifest"
if check > "$fixture/output" 2>&1; then
    echo 'Scanner failed to reject unclassified declarations' >&2; exit 1
fi
for declaration in HEAP ID FLAG ALIASED FOREIGN; do
    if ! rg -q "unclassified declaration: .*${declaration}" "$fixture/output"; then
        echo "Scanner missed $declaration" >&2; exit 1
    fi
done
if rg -q 'ONLY_TEST' "$fixture/output"; then
    echo 'Scanner included a test-only declaration' >&2; exit 1
fi
printf 'crates/rmath/src/atomics.rs\tsync-static\tHEAP\tfixture\tallow-process\ttest\tfixture\n' > "$manifest"
if check > "$fixture/output" 2>&1; then
    echo 'Approving one declaration incorrectly approved other declarations in the same file' >&2; exit 1
fi
printf 'crates/rmath/src/atomics.rs\tsync-static\tID\tfixture\tallow-process\ttest\tfixture\ncrates/rmath/src/atomics.rs\tsync-static\tFLAG\tfixture\tallow-process\ttest\tfixture\n' >> "$manifest"
printf 'crates/rmath/src/atomics.rs\timmutable-static\tALIASED\tfixture\tallow-process\ttest\tfixture\ncrates/rmath/src/atomics.rs\tforeign-static\tFOREIGN\tfixture\tallow-process\ttest\tfixture\n' >> "$manifest"
check
printf 'crates/rmath/src/atomics.rs\tsync-static\tSTALE\tfixture\tallow-process\ttest\tfixture\n' >> "$manifest"
if check > "$fixture/output" 2>&1 || ! rg -q 'stale manifest declaration: .*STALE' "$fixture/output"; then
    echo 'Scanner failed to reject a stale declaration' >&2; exit 1
fi
cat > "$fixture/crates/rmath/src/include.rs" <<'RUST'
fn include_generated_code() { include!("../outside.rs"); }
RUST
if check > "$fixture/output" 2>&1 || ! rg -q 'macro include may declare global state' "$fixture/output"; then
    echo 'Scanner failed to reject a Rust include outside the inventory' >&2; exit 1
fi
rm -f "$fixture/crates/rmath/src/include.rs"
cat > "$fixture/crates/rmath/src/path.rs" <<'RUST'
#[cfg_attr(target_os = "android", path = "../outside.rs")]
mod outside;
RUST
if check > "$fixture/output" 2>&1 || ! rg -q 'module outside has an unresolved path override' "$fixture/output"; then
    echo 'Scanner failed to reject an unresolved module path' >&2; exit 1
fi
printf 'Android global declaration scanner coverage passed.\n'
