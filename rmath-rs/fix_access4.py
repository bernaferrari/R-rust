#!/usr/bin/env python3
"""Fix remaining access site patterns - pass 4: line-by-line targeted fixes."""

import re, os

SRC = os.path.join(os.path.dirname(os.path.abspath(__file__)), "rmath", "src")
GV_RE = re.compile(r"^static\s+(\w+)\s*:\s*Global\s*<([^>]+)>\s*=", re.MULTILINE)


def get_global_vars(content):
    """Returns dict of var_name -> inner_type"""
    return {m.group(1): m.group(2) for m in GV_RE.finditer(content)}


def is_copy_type(t):
    """Heuristic: is this type Copy?"""
    t = t.strip()
    if any(
        t.startswith(x)
        for x in [
            "c_int",
            "c_uint",
            "c_long",
            "c_char",
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
            "c_void",
            "AtomicBool",
            "AtomicU",
            "AtomicI",
        ]
    ):
        return True
    if t in ("bool", "c_int", "c_uint", "c_long", "c_char", "c_double", "c_float"):
        return True
    if re.match(r"^\*mut ", t) or re.match(r"^\*const ", t):
        return True  # raw pointers are Copy
    if t.startswith("Option<") and is_copy_type(t[7:-1]):
        return True
    return False


def process_file(filepath):
    with open(filepath, "r") as f:
        content = f.read()
    original = content
    gvars = get_global_vars(content)
    if not gvars:
        return 0

    fixes = 0
    lines = content.split("\n")
    new_lines = []

    for line in lines:
        orig = line
        stripped = line.lstrip()

        # Skip pure comments
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
            copy = is_copy_type(typ)

            # Already handled?
            if (
                f"{var}.get()" in line
                or f"{var}.read()" in line
                or f"{var}.write(" in line
            ):
                continue

            # Skip declaration
            if re.match(rf"^(\s*)static\s+{bvar}\s*:", line):
                continue

            # Skip Global::new initializer lines
            if f"Global::new(" in line:
                continue

            # Skip use/import lines
            if stripped.startswith("use "):
                continue

            # PATTERN: &raw const VAR -> VAR.get() as *const T
            line = re.sub(rf"&raw const\s+{bvar}", f"{var}.get() as *const _", line)

            # PATTERN: (*VAR).field where VAR is Global<*mut T>
            # This means VAR holds a pointer, and we're dereffing the pointer
            # (*VAR) gives Global<*mut T>, but we want to deref the pointer
            # Should be: (*VAR.read()) which gives *mut T, then .field accesses the pointee
            # But actually: VAR.read() gives *mut T (Copy), then (*VAR.read()).field
            # Or even simpler: VAR.read() gives the pointer, so we need to deref it
            if re.match(r"^\*", typ):  # pointer type
                # (*VAR).field -> (*VAR.read()).field
                line = re.sub(rf"\(\*{bvar}\)(\.\w+)", rf"(*{var}.read())\1", line)
                # (*VAR) as bare expression (e.g., (*VAR).method(args))
                # This is already handled above for .field, but handle .method too
                line = re.sub(rf"\(\*{bvar}\)(\.\w+\()", rf"(*{var}.read())\1", line)
            else:
                # (*VAR).field for non-pointer: VAR is Global<T>, (*VAR) doesn't make sense
                # Should be (*VAR.get()).field
                line = re.sub(rf"\(\*{bvar}\)(\.\w+)", rf"(*{var}.get())\1", line)

            # PATTERN: VAR[idx] (indexing) -> (*VAR.get())[idx]
            line = re.sub(rf"\b{bvar}(\[)", rf"(*{var}.get())\1", line)

            # PATTERN: VAR as Type (cast) -> VAR.get() as Type
            line = re.sub(rf"\b{bvar}\s+as\s+", f"{var}.get() as ", line)

            # PATTERN: VAR.method() where method is on the inner type
            # e.g., VAR.is_null(), VAR.add(n), VAR.as_ptr()
            if copy:
                line = re.sub(
                    rf"\b{bvar}\.(is_null|add|offset|wrapping_add|wrapping_sub|as_ptr|as_mut_ptr|len)\(",
                    f"{var}.read().\\1(",
                    line,
                )

            # PATTERN: VAR == val, VAR != val, VAR < val, etc (comparison)
            # But only for Copy types
            if copy:
                line = re.sub(
                    rf"\b{bvar}\s*(==|!=|<=|>=|<|>)\s*", f"{var}.read() \\1 ", line
                )
                # val == VAR
                line = re.sub(rf"(==|!=)\s+{bvar}\b", f"\\1 {var}.read()", line)

            # PATTERN: VAR + n, n - VAR, VAR * n, etc (arithmetic) - Copy only
            if copy:
                line = re.sub(rf"(?<=[+\-*/%])\s*{bvar}\b", f" {var}.read()", line)
                line = re.sub(rf"\b{bvar}\s*(?=[+\-*/%])", f"{var}.read() ", line)

            # PATTERN: VAR += n -> VAR.write(VAR.read() + n)
            line = re.sub(
                rf"\b{bvar}\s*\+=\s*(.+?)\s*;",
                lambda m: f"{var}.write({var}.read() + {m.group(1).rstrip()});",
                line,
            )

            # PATTERN: VAR -= n
            line = re.sub(
                rf"\b{bvar}\s*-=\s*(.+?)\s*;",
                lambda m: f"{var}.write({var}.read() - {m.group(1).rstrip()});",
                line,
            )

            # PATTERN: !VAR (bool) -> !VAR.read()
            line = re.sub(rf"!{bvar}\b", f"!{var}.read()", line)

            # PATTERN: match VAR { -> match VAR.read() {
            line = re.sub(rf"\bmatch\s+{bvar}\s*\{{", f"match {var}.read() {{", line)

        if line != orig:
            fixes += 1
        new_lines.append(line)

    content = "\n".join(new_lines)

    # Handle multi-line patterns with simple string replacements
    for var in sorted(gvars.keys(), key=len, reverse=True):
        typ = gvars[var]
        copy = is_copy_type(typ)

        # Bare VAR used as value in expressions (not already handled)
        # This is for cases like: func(VAR), return VAR, let x = VAR
        # These should become: func(VAR.read()), return VAR.read(), let x = VAR.read();
        # But ONLY for Copy types and when VAR is not followed by . or [ or =
        # This is hard to do with regex alone, skip for now

        # Handle remaining addr_of patterns that might have been missed
        for prefix in ["std::ptr::", "core::ptr::"]:
            for macro in ["addr_of!", "addr_of_mut!"]:
                old = f"{prefix}{macro}({var})"
                if old in content:
                    new = f"{var}.get()"
                    content = content.replace(old, new)
                    fixes += 1

        old = f"(*std::ptr::addr_of!({var}))"
        if old in content:
            content = content.replace(old, f"(*{var}.get())")
            fixes += 1

        old = f"(*std::ptr::addr_of_mut!({var}))"
        if old in content:
            content = content.replace(old, f"(*{var}.get())")
            fixes += 1

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
