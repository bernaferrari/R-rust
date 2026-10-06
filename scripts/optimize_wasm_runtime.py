#!/usr/bin/env python3
"""Optimize release assets with pinned Binaryen, preserving input Wasm features."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import urllib.request


VERSION = "133"
ARCHIVES = {
    ("Darwin", "arm64"): ("arm64-macos", "ad66da82ac13f163e424b1643f16c6dfcccc98b5966296b43e52d3cab04f84a8"),
    ("Darwin", "x86_64"): ("x86_64-macos", "13a9b90be775c6389ce3d1f879cb8627bea56708ba8c122983941d53a8199b95"),
    ("Linux", "x86_64"): ("x86_64-linux", "2dc9c7813f5375db93d96ead4b78222fcc3e2677bbb832297af4797782a37489"),
    ("Linux", "aarch64"): ("aarch64-linux", "89c07ea56faf38d0fbecf36ca8ec0721756716185f265b568e133d427f299bf8"),
    ("Windows", "AMD64"): ("x86_64-windows", "17a2cbeac6b5693c5fbafab3838d3c65fd9c1eb38b05f5baec6c657e8c84995b"),
    ("Windows", "ARM64"): ("arm64-windows", "492a8e1847a0be1554bb9a7f384227981d60bc013aedc02d8ba1372c3943178c"),
}


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def binaryen(cache):
    system = platform.system()
    target, expected = ARCHIVES[(system, platform.machine())]
    directory = Path(cache) / target
    directory.mkdir(parents=True, exist_ok=True)
    name = f"binaryen-version_{VERSION}-{target}.tar.gz"
    archive = directory / name
    if not archive.exists():
        url = f"https://github.com/WebAssembly/binaryen/releases/download/version_{VERSION}/{name}"
        with urllib.request.urlopen(url, timeout=60) as source:
            archive.write_bytes(source.read())
    if digest(archive) != expected:
        raise ValueError(f"Binaryen {VERSION} archive checksum mismatch: {archive}")
    executable = directory / f"binaryen-version_{VERSION}" / "bin" / ("wasm-opt.exe" if system == "Windows" else "wasm-opt")
    if not executable.exists():
        with tarfile.open(archive) as source:
            source.extractall(directory, filter="data")
    version = subprocess.check_output([str(executable), "--version"], text=True).strip()
    if version != f"wasm-opt version {VERSION} (version_{VERSION})":
        raise ValueError(f"Unexpected Binaryen version: {version}")
    return executable


def execution_sections(path):
    """Exact encoded standard sections; custom names cannot change execution."""
    data = Path(path).read_bytes()
    if data[:8] != b"\x00asm\x01\x00\x00\x00":
        raise ValueError("Invalid Wasm header in profiling module")
    position = 8
    result = bytearray(data[:8])
    while position < len(data):
        start = position
        kind = data[position]
        position += 1
        length = shift = 0
        while True:
            if position >= len(data) or shift > 28:
                raise ValueError("Invalid Wasm section length")
            value = data[position]
            position += 1
            length |= (value & 127) << shift
            if not value & 128:
                break
            shift += 7
        end = position + length
        if end > len(data):
            raise ValueError("Truncated Wasm profiling section")
        if kind:
            result.extend(data[start:end])
        position = end
    return bytes(result)


def optimize(package, executable, profile_names=None):
    package = Path(package)
    wasm = package / "r_wasm_bg.wasm"
    before = {"bytes": wasm.stat().st_size, "sha256": digest(wasm)}
    temporary = package / "r_wasm_bg.optimized.wasm"
    # Read target_features from the input. --all-features would authorize new
    # proposals such as compact imports that current browser/Node engines reject.
    subprocess.run([str(executable), str(wasm), "-Oz", "-o", str(temporary)], check=True)
    if not temporary.stat().st_size:
        raise ValueError("Binaryen produced an empty Wasm module")
    after = {"bytes": temporary.stat().st_size, "sha256": digest(temporary)}
    names_receipt = None
    if profile_names is not None:
        profile_names = Path(profile_names)
        profile_names.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run([str(executable), str(wasm), "-Oz", "-g", "-o", str(profile_names)], check=True)
        standard = execution_sections(temporary)
        if execution_sections(profile_names) != standard:
            raise ValueError("Profiling names changed production Wasm execution sections")
        names_receipt = {"file": str(profile_names), "bytes": profile_names.stat().st_size,
                         "sha256": digest(profile_names),
                         "execution_sections_sha256": hashlib.sha256(standard).hexdigest(),
                         "arguments": ["-Oz", "-g"],
                         "scope": "Diagnostic names only; all standard execution sections byte-identical to production"}
    temporary.replace(wasm)
    receipt = {"schema_version": 1, "binaryen_version": VERSION,
               "optimizer_sha256": digest(executable), "arguments": ["-Oz"],
               "feature_policy": "preserve input target_features", "input": before, "output": after}
    if names_receipt is not None:
        receipt["profile_names"] = names_receipt
    (package / "rust-runtime-optimization.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"Rust Wasm release asset: {before['bytes']} -> {after['bytes']} bytes (Binaryen {VERSION})")


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    args = parser.parse_args()
    optimize(args.package, binaryen(root / "target/task-tools/binaryen-133"),
             os.environ.get("RPORT_WASM_PROFILE_NAMES"))


if __name__ == "__main__":
    main()
