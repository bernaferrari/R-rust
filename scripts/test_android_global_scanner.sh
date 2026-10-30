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
scripts/cargo_dev.sh run -q -p rport-android-global-audit -- "$fixture" "$fixture/inventory"
fixture_fingerprint="$(awk -F '\t' '$3 == "HEAP" { print $4 }' "$fixture/inventory")"
if [[ ! "$fixture_fingerprint" =~ ^[0-9a-f]{64}$ ]]; then
    echo 'Scanner did not provide a source fingerprint' >&2; exit 1
fi
check() {
    awk -v hash="$fixture_fingerprint" 'BEGIN { FS=OFS="\t" } NF==7 { $8=hash } { print }' "$manifest" > "$fixture/manifest-next"
    mv -f "$fixture/manifest-next" "$manifest"
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
cp -f "$fixture/crates/rmath/src/atomics.rs" "$fixture/atomics-original.rs"
sed 's/AtomicU32::new(1)/AtomicU32::new(2)/' "$fixture/atomics-original.rs" > "$fixture/crates/rmath/src/atomics.rs"
if check > "$fixture/output" 2>&1 || ! rg -q 'source fingerprint changed' "$fixture/output"; then
    echo 'Scanner accepted a changed declaration under an old approval' >&2; exit 1
fi
cp -f "$fixture/atomics-original.rs" "$fixture/crates/rmath/src/atomics.rs"
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
printf 'crates/rmath/src/atomics.rs\tsync-static\tHEAP\tfixture\tallow-process\ttest\tfixture\ncrates/rmath/src/atomics.rs\tsync-static\tID\tfixture\tallow-process\ttest\tfixture\ncrates/rmath/src/atomics.rs\tsync-static\tFLAG\tfixture\tallow-process\ttest\tfixture\ncrates/rmath/src/atomics.rs\timmutable-static\tALIASED\tfixture\tallow-process\ttest\tfixture\ncrates/rmath/src/atomics.rs\tforeign-static\tFOREIGN\tfixture\tallow-process\ttest\tfixture\n' > "$manifest"
: > "$fixture/crates/rmath/src/inside.rs"
cat > "$fixture/crates/rmath/src/path.rs" <<'RUST'
#[path = "inside.rs"] mod inside;
RUST
check
: > "$fixture/crates/rmath/outside.rs"
cat > "$fixture/crates/rmath/src/path.rs" <<'RUST'
#[cfg_attr(target_os = "android", path = "../outside.rs")]
mod outside;
RUST
if check > "$fixture/output" 2>&1 || ! rg -q 'module outside has an unresolved path override' "$fixture/output"; then
    echo 'Scanner failed to reject an unresolved module path' >&2; exit 1
fi
mkdir -p "$fixture/crates/rmath/src/support"
: > "$fixture/crates/rmath/src/support/ignored.rs"
cat > "$fixture/crates/rmath/src/path.rs" <<'RUST'
#[path = "support/ignored.rs"] mod ignored;
RUST
if check > "$fixture/output" 2>&1 || ! rg -q 'module ignored has an unresolved path override' "$fixture/output"; then
    echo 'Scanner accepted an excluded source path' >&2; exit 1
fi
cat > "$fixture/crates/rmath/src/path.rs" <<'RUST'
mod nested { #[path = "inside.rs"] mod inner; }
RUST
if check > "$fixture/output" 2>&1 || ! rg -q 'module nested::inner has an unresolved path override' "$fixture/output"; then
    echo 'Scanner accepted an ambiguous nested source path' >&2; exit 1
fi
printf 'Android global declaration scanner coverage passed.\n'
