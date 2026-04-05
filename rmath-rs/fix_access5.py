#!/usr/bin/env python3
"""Pass 5: Fix bare VAR references in function args, return, let bindings, and other expressions."""

import re, os

SRC = os.path.join(os.path.dirname(os.path.abspath(__file__)), "rmath", "src")
GV_RE = re.compile(
    r"^(?:pub\s+)?(?:crate\s+)?(?:pub\(crate\)\s+)?static\s+(\w+)\s*:\s*Global\s*<([^>]+)>",
    re.MULTILINE,
)
TL_RE = re.compile(
    r"thread_local!\s*\{\s*static\s+(\w+)\s*:\s*Cell\s*<([^>]+)>", re.MULTILINE
)

COPY_TYPES = {
    "c_int",
    "c_uint",
    "c_long",
    "c_ulong",
    "c_char",
    "c_uchar",
    "c_double",
    "c_float",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    "f32",
    "f64",
    "bool",
}


def is_copy(t):
    t = t.strip()
    if t in COPY_TYPES:
        return True
    if t.startswith("*mut ") or t.startswith("*const "):
        return True
    if t.startswith("Option<") and is_copy(t[7:-1]):
        return True
    return False


def process_file(filepath):
    with open(filepath, "r") as f:
        content = f.read()
    original = content

    gvars = {m.group(1): m.group(2) for m in GV_RE.finditer(content)}
    tlvars = {m.group(1): m.group(2) for m in TL_RE.finditer(content)}
    if not gvars and not tlvars:
        return 0

    fixes = 0
    lines = content.split("\n")
    new_lines = []

    for line in lines:
        orig = line
        stripped = line.lstrip()

        if (
            stripped.startswith("//")
            or stripped.startswith("*")
            or stripped.startswith("/*")
        ):
            new_lines.append(line)
            continue

        for var in sorted(gvars.keys(), key=len, reverse=True):
            bvar = re.escape(var)
            typ = gvars[var]
            copy = is_copy(typ)

            # Skip if already has accessor
            if (
                f"{var}.get()" in line
                or f"{var}.read()" in line
                or f"{var}.write(" in line
            ):
                continue

            # Skip declarations
            if re.match(rf"^(\s*)static\s+{bvar}\b", line):
                continue
            if f"Global::new(" in line:
                continue
            if stripped.startswith("use "):
                continue
            if f"thread_local!" in line and var in line:
                continue

            # PATTERN: let x = VAR; or let x: Type = VAR;
            # -> let x = VAR.read(); or let x = VAR.get();
            m = re.match(rf"^(\s*let\s+\w+(?::\s*[^=]+)?)\s*=\s*{bvar}\s*;", line)
            if m and copy:
                prefix = m.group(1)
                line = f"{prefix} = {var}.read();"
                continue

            # PATTERN: func(VAR) or func(VAR, -> func(VAR.read(),
            # Match VAR as a standalone argument (not followed by . or [)
            if copy:
                line = re.sub(rf"(?<![.\w]){bvar}(?![.\w\[])", f"{var}.read()", line)
            else:
                # For non-Copy types used as function args, this is harder
                # We'd need &*VAR.get() or similar, skip for now
                pass

        for var in sorted(tlvars.keys(), key=len, reverse=True):
            bvar = re.escape(var)

            if f"{var}.with(" in line:
                continue
            if stripped.startswith("use "):
                continue

            # Skip thread_local declarations
            if f"thread_local!" in line and var in line:
                continue

        if line != orig:
            fixes += 1
        new_lines.append(line)

    content = "\n".join(new_lines)

    # Also do global string replacements for remaining addr_of patterns
    for var in gvars:
        for old_pat, new_pat in [
            (f"std::ptr::addr_of!({var})", f"{var}.get()"),
            (f"std::ptr::addr_of_mut!({var})", f"{var}.get()"),
            (f"core::ptr::addr_of!({var})", f"{var}.get()"),
            (f"core::ptr::addr_of_mut!({var})", f"{var}.get()"),
            (f"(*std::ptr::addr_of!({var}))", f"(*{var}.get())"),
            (f"(*std::ptr::addr_of_mut!({var}))", f"(*{var}.get())"),
            (f"&mut *std::ptr::addr_of_mut!({var})", f"&mut *{var}.get()"),
        ]:
            if old_pat in content:
                content = content.replace(old_pat, new_pat)
                fixes += content.count(new_pat)

    if content != original:
        with open(filepath, "w") as f:
            f.write(content)
    return fixes


total = 0
for root, dirs, files in os.walk(SRC):
    if "support" in root:
        continue
    for f in files:
        if f.endswith(".rs"):
            n = process_file(os.path.join(root, f))
            if n > 0:
                print(f"  {n} in {os.path.join(root, f)}")
                total += n
print(f"\nTotal: {total}")
