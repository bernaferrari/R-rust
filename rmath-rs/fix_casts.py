#!/usr/bin/env python3
"""Fix cast warnings by replacing `x as T` with `T::from(x)` where appropriate."""

import subprocess
import re
import os

BASE_DIR = "/Users/bernardoferrari/Downloads/rport/rmath-rs"


def get_clippy_warnings():
    """Get clippy warnings about casts."""
    result = subprocess.run(
        ["cargo", "clippy", "--lib", "--message-format=short"],
        capture_output=True,
        text=True,
        cwd=BASE_DIR,
        env={**os.environ, "CARGO_TARGET_DIR": "target"},
    )
    warnings = []
    for line in result.stderr.split("\n"):
        if "can be expressed infallibly using" in line:
            warnings.append(line)
    return warnings


def get_warning_locations():
    """Get file:line:col for each warning."""
    result = subprocess.run(
        ["cargo", "clippy", "--lib"],
        capture_output=True,
        text=True,
        cwd=BASE_DIR,
        env={**os.environ, "CARGO_TARGET_DIR": "target"},
    )

    locations = []
    lines = result.stderr.split("\n")
    i = 0
    while i < len(lines):
        line = lines[i]
        if "can be expressed infallibly using" in line:
            # Extract from/to types
            from_match = re.search(r"casts from `([^`]+)` to `([^`]+)`", line)
            if from_match:
                from_type = from_match.group(1)
                to_type = from_match.group(2)
                # Next line with --> has the location
                j = i + 1
                while j < len(lines):
                    loc_match = re.search(r"-->\s+(\S+):(\d+):(\d+)", lines[j])
                    if loc_match:
                        locations.append(
                            {
                                "file": loc_match.group(1),
                                "line": int(loc_match.group(2)),
                                "col": int(loc_match.group(3)),
                                "from_type": from_type,
                                "to_type": to_type,
                            }
                        )
                        break
                    j += 1
        i += 1
    return locations


def fix_cast_in_line(line_text, from_type, to_type, col):
    """Fix a cast at the given column position."""
    # Find the cast pattern starting at col
    # The expression before 'as' needs to be identified

    # Strategy: find ' as TYPE' pattern and work backwards to find the expression
    remaining = line_text[col - 1 :]  # 0-indexed

    # Find the ' as TYPE' pattern
    cast_pattern = rf"\s+as\s+{re.escape(to_type)}\b"
    match = re.search(cast_pattern, remaining)
    if not match:
        return None, line_text

    # Get the expression before 'as'
    before_cast = remaining[: match.start()]
    after_cast = remaining[match.end() :]

    # Find the expression boundary (work backwards from the cast)
    # Need to find the complete expression
    expr = find_expression_end(before_cast.rstrip())
    if expr is None:
        return None, line_text

    # Build the replacement
    replacement = f"{to_type}::from({expr})"
    new_remaining = replacement + after_cast

    # Reconstruct the line
    new_line = line_text[: col - 1] + new_remaining
    return expr, new_line


def find_expression_end(text):
    """Find the expression that ends at the end of text."""
    if not text:
        return None

    # Work backwards to find the expression
    # Handle common patterns:
    # - Simple variable: x
    # - Function call: func()
    # - Array access: arr[i]
    # - Field access: obj.field
    # - Parenthesized: (expr)
    # - Binary op: a + b
    # - Unary: -x, !x
    # - Literal: 123

    # Count parentheses/brackets to find boundaries
    depth = 0
    i = len(text) - 1

    while i >= 0:
        c = text[i]
        if c == ")":
            depth += 1
        elif c == "(":
            if depth == 0:
                # Found opening paren, include it
                return text[i:]
            depth -= 1
        elif c == "]":
            depth += 1
        elif c == "[":
            if depth == 0:
                return text[i:]
            depth -= 1
        elif c == "}":
            depth += 1
        elif c == "{":
            if depth == 0:
                return text[i:]
            depth -= 1
        elif depth == 0 and c in (";", ","):
            # Expression boundary
            return text[i + 1 :]
        i -= 1

    return text[i + 1 :] if i >= 0 else text


def fix_file(filepath, fixes):
    """Apply fixes to a file."""
    with open(filepath, "r") as f:
        lines = f.readlines()

    # Sort fixes by line number (reverse) to apply from bottom to top
    fixes.sort(key=lambda x: x["line"], reverse=True)

    changed = False
    for fix in fixes:
        line_idx = fix["line"] - 1
        if line_idx >= len(lines):
            continue

        line = lines[line_idx]
        col = fix["col"]
        from_type = fix["from_type"]
        to_type = fix["to_type"]

        # Try to fix the cast
        _, new_line = fix_cast_in_line(line, from_type, to_type, col)
        if new_line != line:
            lines[line_idx] = new_line
            changed = True

    if changed:
        with open(filepath, "w") as f:
            f.writelines(lines)

    return changed


def main():
    print("Getting clippy warnings...")
    locations = get_warning_locations()
    print(f"Found {len(locations)} cast warnings")

    # Group by file
    by_file = {}
    for loc in locations:
        filepath = os.path.join(BASE_DIR, loc["file"])
        if filepath not in by_file:
            by_file[filepath] = []
        by_file[filepath].append(loc)

    print(f"Fixing {len(by_file)} files...")
    for filepath, fixes in by_file.items():
        if fix_file(filepath, fixes):
            print(f"  Fixed {filepath}")

    print("Done!")


if __name__ == "__main__":
    main()
