#!/usr/bin/env python3
"""Count parsed Rust unsafe syntax in the core runtime and desktop CLI."""

import argparse
from collections import Counter
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SOURCE_PATHS = ["crates/rmath/src", "crates/nmath/src", "crates/r-host-cli/src"]
KINDS = {
    "function_type": "unsafe_function_types",
    "function_modifiers": "unsafe_function_declarations",
    "unsafe_block": "unsafe_blocks",
    "impl_item": "unsafe_implementations",
    "trait_item": "unsafe_traits",
    "foreign_mod_item": "unsafe_foreign_modules",
}
ITEM_KINDS = {"impl_item", "trait_item", "foreign_mod_item"}
QUALIFIER = re.compile(r"^\s*(?:pub(?:\([^()]*\))?\s+)?(?P<token>unsafe)\b")
KEYWORD = re.compile(r"(?P<token>\bunsafe\b)")


def measure(root, paths):
    sites = {}
    for kind, category in KINDS.items():
        result = subprocess.run(
            ["ast-grep", "run", "-l", "rust", "-k", kind, "--json=stream", *paths],
            cwd=root, capture_output=True, text=True,
        )
        if result.returncode not in (0, 1) or result.stderr.strip():
            raise ValueError(result.stderr.strip() or f"ast-grep failed for {kind}")
        for line in result.stdout.splitlines():
            node = json.loads(line)
            source = node["text"]
            match = (QUALIFIER if kind in ITEM_KINDS else KEYWORD).search(source)
            if match is None:
                continue
            path = Path(node["file"])
            if path.is_absolute():
                path = path.relative_to(root)
            position = node["range"]["byteOffset"]["start"]
            position += len(source[:match.start("token")].encode())
            # A function pointer qualifier can also appear as a modifiers
            # node. Its specific function type is counted first.
            sites.setdefault((path.as_posix(), position), category)
    return {
        "source_paths": paths,
        "scope": "Written Rust syntax, including inline tests. Macro expansion is excluded.",
        "counts": dict(Counter(sites.values())),
        "total": len(sites),
        "files": dict(Counter(path for path, _ in sites)),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("paths", nargs="*", help="Rust source directories to count")
    args = parser.parse_args()
    try:
        result = measure(args.source_root.resolve(), args.paths or SOURCE_PATHS)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
