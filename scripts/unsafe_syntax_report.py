#!/usr/bin/env python3
"""Count parsed Rust unsafe syntax in the core runtime and desktop CLI."""

import argparse
from collections import Counter, defaultdict
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
MODULE_NAME = re.compile(r"\bmod\s+(?:r#)?([\w]+)")
TEST_ATTRIBUTE = {
    "all": [
        {"kind": "attribute_item"},
        {"regex": r"^#\[\s*(?:test\s*\]|cfg\s*\(\s*(?:all\s*\(\s*)?test\s*[,\)])"},
    ]
}
ATTRIBUTE_SIBLINGS = {"not": {"any": [
    {"kind": kind} for kind in ("attribute_item", "line_comment", "block_comment")
]}}
EXTERNAL_MODULE = {"all": [{"kind": "mod_item"}, {"regex": r";\s*$"}]}


def scan(root, paths, rules):
    config = "\n---\n".join(json.dumps({
        "id": name, "language": "Rust", "severity": "hint",
        "message": name, "rule": rule,
    }) for name, rule in rules.items())
    result = subprocess.run(
        ["ast-grep", "scan", "--inline-rules", config, "--json=stream", *paths],
        cwd=root, capture_output=True, text=True,
    )
    if result.returncode not in (0, 1) or result.stderr.strip():
        raise ValueError(result.stderr.strip() or "ast-grep could not identify test scopes")
    return [json.loads(line) for line in result.stdout.splitlines()]


def excludes_tests(root, paths):
    """Identify explicitly test-only source, including nested external modules."""
    test_kinds = (
        "mod_item", "function_item", "impl_item", "const_item", "static_item",
        "struct_item", "enum_item", "union_item", "trait_item", "foreign_mod_item",
        "type_item", "expression_statement", "let_declaration", "macro_definition",
    )
    rules = {
        "test-scope": {"all": [
            {"any": [{"kind": kind} for kind in test_kinds]},
            {"follows": {**TEST_ATTRIBUTE, "stopBy": ATTRIBUTE_SIBLINGS}},
        ]},
        "external-module": EXTERNAL_MODULE,
        "path-module": {"all": [
            EXTERNAL_MODULE,
            {"follows": {
                "pattern": {
                    "context": '#[path = $PATH]\nmod item;',
                    "selector": "attribute_item",
                },
                "stopBy": ATTRIBUTE_SIBLINGS,
            }},
        ]},
        "inline-module": {"all": [{"kind": "mod_item"}, {"regex": r"\}\s*$"}]},
    }
    test_ranges = defaultdict(list)
    inline_modules = defaultdict(list)
    external_modules = []
    overrides = {}
    for node in scan(root, paths, rules):
        path = (root / node["file"]).resolve()
        offsets = node["range"]["byteOffset"]
        start, end = offsets["start"], offsets["end"]
        category = node["ruleId"]
        if category == "test-scope":
            test_ranges[path].append((start, end))
        elif category == "path-module":
            raw = node["metaVariables"]["single"]["PATH"]["text"]
            overrides[path, start] = json.loads(raw)
        else:
            name = MODULE_NAME.search(node["text"])[1]
            if category == "inline-module":
                inline_modules[path].append((start, end, name))
            else:
                external_modules.append((path, start, name))

    def inside_test(path, position):
        return any(start <= position < end for start, end in test_ranges.get(path, ()))

    references = defaultdict(list)
    for path, offset, name in external_modules:
        inline = [
            name for start, end, name in sorted(inline_modules[path])
            if start < offset < end
        ]
        directory = path.parent
        if path.name not in ("main.rs", "lib.rs", "mod.rs"):
            directory /= path.stem
        directory = directory.joinpath(*inline)
        if (path, offset) in overrides:
            parent = directory if inline else path.parent
            candidates = [parent / overrides[path, offset]]
        else:
            candidates = [directory / f"{name}.rs", directory / name / "mod.rs"]
        for candidate in candidates:
            if candidate.is_file():
                references[candidate.resolve()].append((path, inside_test(path, offset)))

    test_files = set()
    while True:
        newly_test_only = {
            path for path, incoming in references.items()
            if all(test or parent in test_files for parent, test in incoming)
        }
        if newly_test_only == test_files:
            break
        test_files = newly_test_only

    def excluded(path, position):
        absolute = root / path
        return absolute in test_files or inside_test(absolute, position)

    return excluded


def measure(root, paths, production_only=False):
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
    scope = "Written Rust syntax, including inline tests. Macro expansion is excluded."
    if production_only:
        # Resolve declarations outside a requested subdirectory before counting it.
        module_paths = [path for path in SOURCE_PATHS if (root / path).is_dir()]
        for path in paths:
            if not any(Path(path).is_relative_to(parent) for parent in module_paths):
                module_paths.append(path)
        excluded = excludes_tests(root, module_paths)
        sites = {site: kind for site, kind in sites.items() if not excluded(*site)}
        scope = "Written Rust syntax outside explicit test-only items and modules. Macro expansion is excluded."
    return {
        "source_paths": paths,
        "scope": scope,
        "counts": dict(Counter(sites.values())),
        "total": len(sites),
        "files": dict(Counter(path for path, _ in sites)),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--production", action="store_true", help="exclude explicit test-only source")
    parser.add_argument("paths", nargs="*", help="Rust source directories to count")
    args = parser.parse_args()
    try:
        result = measure(args.source_root.resolve(), args.paths or SOURCE_PATHS, args.production)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
