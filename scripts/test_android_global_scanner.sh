#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/scripts" "$fixture/docs" "$fixture/crates/rmath/src"
cp -f "$root/scripts/check_android_globals.sh" "$fixture/scripts/"
: > "$fixture/docs/android-platform-global-allowlist.tsv"
cat > "$fixture/crates/rmath/src/atomics.rs" <<'RUST'
static HEAP: AtomicU32 = AtomicU32::new(1);
static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static FLAG: AtomicBool = AtomicBool::new(false);
RUST
cat > "$fixture/crates/rmath/src/test_only.rs" <<'RUST'
#[cfg(test)]
mod tests {
    static ONLY_TEST: AtomicU32 = AtomicU32::new(0);
}
RUST
if "$fixture/scripts/check_android_globals.sh" > "$fixture/output" 2>&1; then
    echo 'Scanner failed to reject unclassified atomic declarations' >&2
    exit 1
fi
for declaration in HEAP ID FLAG; do
    if ! rg -q "static $declaration:" "$fixture/output"; then
        echo "Scanner missed $declaration" >&2
        exit 1
    fi
done
if rg -q 'test_only.rs' "$fixture/output"; then
    echo 'Scanner included a cfg(test) module' >&2
    exit 1
fi
printf 'crates/rmath/src/atomics.rs\tatomic-fixture\tallow-process\ttest\tfixture\n' > "$fixture/docs/android-platform-global-allowlist.tsv"
"$fixture/scripts/check_android_globals.sh" >/dev/null
printf 'Android global scanner atomic coverage passed.\n'
