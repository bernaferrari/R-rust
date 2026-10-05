#!/usr/bin/env python3
"""Generate the original pinned GNU methods image independently of Rust."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("--rscript", required=True)
parser.add_argument("--output", type=Path, default=Path(__file__).with_name("assets"))
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
result = subprocess.run([args.rscript, "--vanilla", str(Path(__file__).with_suffix(".R")), str(args.output)], check=True, capture_output=True, text=True)
(args.output / "generation.log").write_text(result.stdout + result.stderr)
files = {p.name: {"bytes":p.stat().st_size,"sha256":hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(args.output.iterdir()) if p.is_file() and p.name != "manifest.json"}
(args.output / "manifest.json").write_text(json.dumps({"source_commit":"bac583951b728e97b9786804d3b4081f0fe18df5","r_version":"4.7.0","svn_revision":"90451","files":files},indent=2)+"\n")
print(result.stdout, end="")
