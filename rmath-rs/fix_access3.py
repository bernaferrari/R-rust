#!/usr/bin/env python3
"""Fix remaining access site patterns for Global<T> variables - pass 2."""

import re
import os

SRC_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "rmath", "src")
GLOBAL_RE = re.compile(r"^static\s+(\w+)\s*:\s*Global\s*<[^>]+>\s*=", re.MULTILINE)


def find_global_vars(content):
    return set(GLOBAL_RE.findall(content))


def process_file(filepath):
    with open(filepath, "r") as f:
        content = f.read()
    original = content
    global_vars = find_global_vars(content)
    if not global_vars:
        return 0

    fixes = 0
    for var in sorted(global_vars, key=len, reverse=True):  # longest first
        bvar = re.escape(var)

        # 1. std::ptr::addr_of!(VAR) -> VAR.get()
        #    (*std::ptr::addr_of!(VAR)) -> (*VAR.get())
        old = f"(*std::ptr::addr_of!({var}))"
        new = f"(*{var}.get())"
        while old in content:
            content = content.replace(old, new)
            fixes += 1

        old = f"std::ptr::addr_of!({var})"
        new = f"{var}.get()"
        while old in content:
            content = content.replace(old, new)
            fixes += 1

        # 2. std::ptr::addr_of_mut!(VAR) -> VAR.get()
        old = f"(*std::ptr::addr_of_mut!({var}))"
        new = f"(*{var}.get())"
        while old in content:
            content = content.replace(old, new)
            fixes += 1

        old = f"std::ptr::addr_of_mut!({var})"
        new = f"{var}.get()"
        while old in content:
            content = content.replace(old, new)
            fixes += 1

        # 3. core::ptr::addr_of!(VAR) -> VAR.get()
        old = f"core::ptr::addr_of!({var})"
        new = f"{var}.get()"
        while old in content:
            content = content.replace(old, new)
            fixes += 1

        old = f"core::ptr::addr_of_mut!({var})"
        new = f"{var}.get()"
        while old in content:
            content = content.replace(old, new)
            fixes += 1

    # Now do line-by-line fixes
    lines = content.split("\n")
    new_lines = []
    for line in lines:
        orig_line = line

        # Skip comments and declarations
        stripped = line.lstrip()
        if (
            stripped.startswith("//")
            or stripped.startswith("*")
            or stripped.startswith("/*")
        ):
            new_lines.append(line)
            continue

        for var in sorted(global_vars, key=len, reverse=True):
            bvar = re.escape(var)

            # Skip if already processed
            if (
                f"{var}.get()" in line
                or f"{var}.read()" in line
                or f"{var}.write(" in line
                or f"{var}.with(" in line
            ):
                continue

            # Skip declaration lines
            if re.match(rf"^static\s+{bvar}\s*:", line):
                continue

            # Skip lines that are inside the Global type declaration
            if "Global::new(" in line and var in line:
                continue

            # VAR = expr; (top-level assignment, not field/idx)
            m = re.match(rf"^(\s*){bvar}\s*=\s*(.+?)\s*;\s*$", line)
            if m:
                indent = m.group(1)
                rhs = m.group(2).rstrip()
                # Don't match if rhs contains { (struct init assigned to .write would be wrong)
                if "{" not in rhs or "Global::new" in rhs:
                    line = f"{indent}{var}.write({rhs});"
                    continue

            # match VAR { ... } -> match VAR.read() { ... }
            line = re.sub(rf"\bmatch\s+{bvar}\s*\{{", f"match {var}.read() {{", line)

            # &raw const VAR -> &raw const *VAR.get() -- no, this is wrong
            # Actually: &raw const VAR gives *const T. For Global<T>, we want VAR.get() as *const T
            # But &raw const is unstable. Let's skip this.

        if line != orig_line:
            fixes += 1
        new_lines.append(line)

    content = "\n".join(new_lines)

    if content != original:
        with open(filepath, "w") as f:
            f.write(content)
    return fixes


total = 0
for root, dirs, files in os.walk(SRC_DIR):
    if "support" in root:
        continue
    for fname in files:
        if fname.endswith(".rs"):
            fp = os.path.join(root, fname)
            n = process_file(fp)
            if n > 0:
                print(f"  {n} fixes in {fp}")
                total += n
print(f"\nTotal: {total}")
