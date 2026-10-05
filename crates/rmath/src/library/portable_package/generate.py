#!/usr/bin/env python3
"""Capture and authenticate original portable package images."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[5]
parser = argparse.ArgumentParser()
parser.add_argument("--rscript", required=True)
parser.add_argument("--package", choices=("methods", "utils", "tools"), required=True)
parser.add_argument("--output", required=True, type=Path)
args = parser.parse_args()
spec = importlib.util.spec_from_file_location("validate_r_oracle", ROOT / "scripts/validate_r_oracle.py")
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)
manifest = ROOT / "oracle/r-oracle.json"
pinned = validator.load_manifest(manifest)
validator.verify_runtime(pinned, manifest, args.rscript)
if args.output.exists() and any(args.output.iterdir()):
    raise ValueError("output must be new or empty")
args.output.mkdir(parents=True, exist_ok=True)
script = Path(__file__).with_suffix(".R")
result = subprocess.run([args.rscript, "--vanilla", str(script), args.package, str(args.output)], check=True, capture_output=True, text=True, timeout=120)
(args.output / "generation.log").write_text(result.stdout + result.stderr)
def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
files = {p.name: {"bytes": p.stat().st_size, "sha256": digest(p)} for p in sorted(args.output.iterdir()) if p.is_file()}
(args.output / "manifest.json").write_text(json.dumps({"source": pinned["source"], "runtime": pinned["runtime"], "oracle_manifest_sha256": digest(manifest), "generator_sha256": {"generate.R": digest(script), "generate.py": digest(Path(__file__))}, "files": files}, indent=2)+"\n")
print(result.stdout, end="")
