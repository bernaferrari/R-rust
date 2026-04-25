#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="$ROOT_DIR/target/stock-r-performance"
ITERATIONS=5
CHECK=0
RUST_RUNNER_SRC="$ROOT_DIR/tests/conformance/src/main.rs"

usage() {
    cat <<'USAGE'
Usage: scripts/compare_stock_r_performance.sh [OPTIONS]

Compares stock C R (Rscript) with the Rust runtime on small overlapping R
programs. The harness checks output parity first, then reports wall time and
resident-memory measurements where /usr/bin/time exposes them.

Options:
  --iterations N        Process runs per runtime and case, default 5.
  --output-dir DIR      Write report artifacts to DIR.
  --check               Fail on output mismatch or benchmark execution errors.
  -h, --help            Show this help.
USAGE
}

while (($# > 0)); do
    case "$1" in
        --iterations)
            ITERATIONS="$2"
            shift 2
            ;;
        --output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --check)
            CHECK=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if ! [[ "$ITERATIONS" =~ ^[0-9]+$ ]] || [[ "$ITERATIONS" -eq 0 ]]; then
    echo "--iterations must be a positive integer" >&2
    exit 2
fi

if ! command -v Rscript >/dev/null 2>&1; then
    echo "SKIP: Rscript not found; stock C R comparison requires GNU R." >&2
    exit 0
fi

find_rust_rlib() {
    local found=""
    shopt -s nullglob
    local rust_rlibs=(
        "$ROOT_DIR"/target/debug/deps/librmath-*.rlib
        "$ROOT_DIR"/target/debug/deps/librmath.rlib
    )
    shopt -u nullglob
    if (( ${#rust_rlibs[@]} > 0 )); then
        found="$(ls -t "${rust_rlibs[@]}" 2>/dev/null | head -n1)"
    fi
    printf '%s' "$found"
}

mkdir -p "$OUTPUT_DIR"

RUSTFLAGS_FOR_BUILD="${RUSTFLAGS:-}"
if [[ "$RUSTFLAGS_FOR_BUILD" != *"-Awarnings"* ]]; then
    RUSTFLAGS_FOR_BUILD="${RUSTFLAGS_FOR_BUILD:+$RUSTFLAGS_FOR_BUILD }-Awarnings"
fi

echo "INFO: building Rust rmath artifact for benchmark runner." >&2
(cd "$ROOT_DIR" && env RUSTFLAGS="$RUSTFLAGS_FOR_BUILD" cargo build -p rmath >/dev/null)

RUST_RLIB="$(find_rust_rlib)"
if [[ -z "$RUST_RLIB" ]]; then
    echo "ERROR: Rust rmath artifact missing after build." >&2
    exit 1
fi

RUNNER_TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/rport-stock-r-bench-runner.XXXXXX")"
RUST_BIN="$RUNNER_TMP_DIR/rust_runner"
cleanup_runner() {
    rm -rf "$RUNNER_TMP_DIR"
}
trap cleanup_runner EXIT

if ! rustc --edition=2024 "$RUST_RUNNER_SRC" -L dependency="$ROOT_DIR/target/debug/deps" --extern rmath="$RUST_RLIB" -o "$RUST_BIN" >"$RUNNER_TMP_DIR/rustc.log" 2>&1; then
    echo "ERROR: failed to compile Rust benchmark runner"
    sed 's/^/  rustc | /' "$RUNNER_TMP_DIR/rustc.log"
    exit 1
fi

python3 - "$ITERATIONS" "$OUTPUT_DIR" "$RUST_BIN" "$ROOT_DIR" "$CHECK" <<'PY'
import json
import platform
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path

iterations = int(sys.argv[1])
output_dir = Path(sys.argv[2])
rust_bin = Path(sys.argv[3])
root_dir = Path(sys.argv[4])
check = sys.argv[5] == "1"

case_dir = output_dir / "cases"
case_dir.mkdir(parents=True, exist_ok=True)

cases = {
    "startup_scalar": "print(1 + 1)\n",
    "vector_summary": "x <- 1:10000\nprint(c(sum(x), length(unique(c(x, x))), min(x), max(x)))\n",
    "vector_arithmetic": "x <- 1:20000\ny <- x + x * 2\nprint(c(length(y), sum(y), y[100], y[20000]))\n",
    "numeric_edges": (
        "print(c(1 / 0, -1 / 0, 0 / 0))\n"
        "print(suppressWarnings(c(1 %% 0, 1L %% 0L, 1 %/% 0, 1.5 %/% 0.0)))\n"
    ),
}

def normalize(text: str) -> str:
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").splitlines()]
    return "\n".join(lines).strip()

def write_cases():
    written = {}
    for name, code in cases.items():
        path = case_dir / f"{name}.R"
        path.write_text(code)
        written[name] = path
    return written

def parse_time_stderr(stderr: str):
    rss_bytes = None
    if platform.system() == "Darwin":
        match = re.search(r"^\s*(\d+)\s+maximum resident set size$", stderr, re.MULTILINE)
        if match:
            rss_bytes = int(match.group(1))
    else:
        match = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", stderr)
        if match:
            rss_bytes = int(match.group(1)) * 1024
    return rss_bytes

def timed_run(command):
    timer = Path("/usr/bin/time")
    if timer.exists():
        if platform.system() == "Darwin":
            command = [str(timer), "-l", *command]
        else:
            command = [str(timer), "-v", *command]

    started = time.perf_counter()
    completed = subprocess.run(command, cwd=root_dir, text=True, capture_output=True)
    elapsed = time.perf_counter() - started
    rss_bytes = parse_time_stderr(completed.stderr)
    return {
        "returncode": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
        "elapsed_ms": elapsed * 1000.0,
        "rss_bytes": rss_bytes,
    }

def summarize(samples):
    elapsed = [sample["elapsed_ms"] for sample in samples]
    rss = [sample["rss_bytes"] for sample in samples if sample["rss_bytes"] is not None]
    return {
        "iterations": len(samples),
        "avg_ms": statistics.fmean(elapsed),
        "min_ms": min(elapsed),
        "max_ms": max(elapsed),
        "median_ms": statistics.median(elapsed),
        "avg_rss_bytes": int(statistics.fmean(rss)) if rss else None,
        "max_rss_bytes": max(rss) if rss else None,
    }

case_paths = write_cases()
rows = []
failures = []

for case, path in case_paths.items():
    runtimes = {
        "stock_c_r": ["Rscript", "--vanilla", str(path)],
        "rust_port": [str(rust_bin), str(path)],
    }
    outputs = {}
    runtime_samples = {}
    for runtime, command in runtimes.items():
        samples = []
        for _ in range(iterations):
            sample = timed_run(command)
            samples.append(sample)
            if sample["returncode"] != 0:
                failures.append(f"{case}/{runtime}: exited {sample['returncode']}")
        runtime_samples[runtime] = samples
        outputs[runtime] = normalize(samples[-1]["stdout"]) if samples else ""

    parity = outputs["stock_c_r"] == outputs["rust_port"]
    if not parity:
        failures.append(f"{case}: output mismatch")

    stock_summary = summarize(runtime_samples["stock_c_r"])
    rust_summary = summarize(runtime_samples["rust_port"])
    rows.append({
        "case": case,
        "parity": parity,
        "stock_c_r": stock_summary,
        "rust_port": rust_summary,
        "rust_vs_stock_avg_ratio": (
            rust_summary["avg_ms"] / stock_summary["avg_ms"]
            if stock_summary["avg_ms"] > 0.0
            else None
        ),
        "stock_output": outputs["stock_c_r"],
        "rust_output": outputs["rust_port"],
    })

report = {
    "iterations": iterations,
    "host": {
        "system": platform.system(),
        "machine": platform.machine(),
        "python": platform.python_version(),
    },
    "cases": rows,
    "failures": failures,
}

json_path = output_dir / "stock-r-performance-summary.json"
json_path.write_text(json.dumps(report, indent=2) + "\n")

lines = [
    "# Stock C R vs Rust Runtime Performance",
    "",
    "These measurements compare only R programs that both runtimes can execute and whose output matches.",
    "Wall time includes process startup, parsing, evaluation, and printing.",
    "",
    f"- Iterations per runtime/case: `{iterations}`",
    f"- Host: `{report['host']['system']} {report['host']['machine']}`",
    "",
    "| Case | Parity | Stock C R avg ms | Rust avg ms | Rust/Stock | Stock RSS | Rust RSS |",
    "| --- | --- | ---: | ---: | ---: | ---: | ---: |",
]

def rss_text(value):
    if value is None:
        return "n/a"
    return f"{value / (1024 * 1024):.1f} MiB"

for row in rows:
    stock = row["stock_c_r"]
    rust = row["rust_port"]
    ratio = row["rust_vs_stock_avg_ratio"]
    lines.append(
        "| `{case}` | {parity} | {stock_ms:.3f} | {rust_ms:.3f} | {ratio} | {stock_rss} | {rust_rss} |".format(
            case=row["case"],
            parity="yes" if row["parity"] else "no",
            stock_ms=stock["avg_ms"],
            rust_ms=rust["avg_ms"],
            ratio=f"{ratio:.2f}x" if ratio is not None else "n/a",
            stock_rss=rss_text(stock["avg_rss_bytes"]),
            rust_rss=rss_text(rust["avg_rss_bytes"]),
        )
    )

if failures:
    lines.extend(["", "## Failures", ""])
    lines.extend(f"- {failure}" for failure in failures)

markdown = "\n".join(lines) + "\n"
md_path = output_dir / "stock-r-performance-summary.md"
md_path.write_text(markdown)
print(markdown)
print(f"Wrote {md_path}")
print(f"Wrote {json_path}")

if failures and check:
    raise SystemExit(1)
PY
