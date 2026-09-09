#!/usr/bin/env python3
"""Preview obsolete hashed executables; keep libraries and incremental caches.

Run while Cargo is idle. --apply deletes the previewed candidates. Cargo can
relink removed variants when needed. This utility targets Unix Cargo layouts.
"""

import argparse
import collections
import fcntl
import json
import os
from pathlib import Path
import re
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--keep", type=int, default=2,
                        help="recent variants retained per binary name (default: 2)")
    parser.add_argument("--artifacts-json", type=Path,
                        help="also protect filenames from cargo --message-format=json")
    parser.add_argument("--target-dir", type=Path, help="Cargo target directory (defaults to CARGO_TARGET_DIR or project target)")
    args = parser.parse_args()
    if args.keep < 1:
        parser.error("--keep must be at least 1")
    root = Path(__file__).resolve().parent.parent
    target = args.target_dir or Path(os.environ.get("CARGO_TARGET_DIR", root / "target"))
    debug = target.resolve() / "debug"
    deps = debug / "deps"
    if not deps.is_dir():
        print("No target/debug/deps directory to prune.")
        return

    protected = set()
    if args.artifacts_json:
        for line in args.artifacts_json.read_text().splitlines():
            item = json.loads(line)
            if item.get("reason") == "compiler-artifact":
                protected.update(Path(p).resolve() for p in item.get("filenames", []))

    # Cargo holds this lock during builds. Never prune a build in progress.
    with (debug / ".cargo-lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            parser.exit(1, "Cargo is using target/debug; wait for it to finish.\n")
        groups = collections.defaultdict(list)
        locations = [deps]
        if (debug / "examples").is_dir():
            locations.append(debug / "examples")
        for path in (p for location in locations for p in location.iterdir()):
            match = re.fullmatch(r"(.+)-[a-f0-9]{16}", path.name)
            if (match and not path.is_symlink() and path.is_file()
                    and os.access(path, os.X_OK)):
                groups[(path.parent, match[1])].append(path)
        candidates = []
        for paths in groups.values():
            paths.sort(key=lambda path: path.stat().st_mtime_ns, reverse=True)
            candidates.extend(p for p in paths[args.keep:] if p.resolve() not in protected)
        candidates.sort()
        # Cargo replays cached diagnostics from these JSON-lines files. Clearing
        # old oversized warning logs leaves successful fingerprints intact.
        cutoff = time.time() - 7 * 24 * 60 * 60
        diagnostics = [p for p in (debug / ".fingerprint").glob("*/output-*")
                       if not p.is_symlink() and p.is_file()
                       and p.stat().st_mtime < cutoff and p.stat().st_size > 10 * 1024**2]
        total = sum(path.stat().st_size for path in candidates + diagnostics)
        for path in candidates:
            print(path)
        for path in diagnostics:
            print(f"Clear cached diagnostics: {path}")
        print(f"{len(candidates)} obsolete binaries, {len(diagnostics)} old warning logs; {total / 1024**3:.2f} GiB apparent size.")
        if args.apply:
            for path in candidates:
                path.unlink()
            for path in diagnostics:
                path.write_bytes(b"")
            print("Libraries, object files and incremental caches retained.")
        else:
            print("Preview only. Repeat with --apply to remove these binaries.")


if __name__ == "__main__":
    main()
