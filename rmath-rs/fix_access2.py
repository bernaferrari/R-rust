#!/usr/bin/env python3
"""
Fix all access site patterns for Global<T> and LocalKey<Cell<T>> variables.
"""

import re
import os
import sys

SRC_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "rmath", "src")

# Regex to find Global<T> variable declarations
GLOBAL_RE = re.compile(r"^static\s+(\w+)\s*:\s*Global\s*<[^>]+>\s*=", re.MULTILINE)
# Regex to find thread_local Cell<T> variables
THREAD_LOCAL_RE = re.compile(
    r"thread_local!\s*\{\s*static\s+(\w+)\s*:\s*Cell\s*<[^>]+>\s*=", re.MULTILINE
)


def find_global_vars(content):
    """Find all Global<T> variable names in the file."""
    return set(GLOBAL_RE.findall(content))


def find_thread_local_vars(content):
    """Find all thread_local Cell<T> variable names in the file."""
    return set(THREAD_LOCAL_RE.findall(content))


def fix_global_access(content, var):
    """Fix all access patterns for a Global<T> variable."""
    fixes = 0

    # Pattern 1: std::ptr::addr_of!(VAR) -> VAR.get() as *const _
    # Pattern 1b: (*std::ptr::addr_of!(VAR)).method() -> VAR.read().method() for Copy types
    # This is complex - handle (*std::ptr::addr_of!(VAR)) -> *VAR.get()
    old = f"(*std::ptr::addr_of!({var}))"
    new = f"(*{var}.get())"
    if old in content:
        content = content.replace(old, new)
        fixes += content.count(new)

    old = f"(*std::ptr::addr_of_mut!({var}))"
    new = f"(*{var}.get())"
    if old in content:
        content = content.replace(old, new)
        fixes += content.count(new)

    # Pattern 2: *std::ptr::addr_of_mut!(VAR) = expr; -> var.write(expr);
    # This is: *std::ptr::addr_of_mut!(VAR) = value -> var.write(value)
    # But it appears as (*std::ptr::addr_of_mut!(VAR)) already handled above

    # Pattern 3: unsafe { VAR } on its own (return value or expression) -> unsafe { VAR.read() }
    # Be careful: only when VAR is used as a value, not when followed by . or =
    # Match: unsafe { VAR } or unsafe { VAR } with only whitespace before }
    # Actually the simpler approach: within unsafe { }, bare VAR used as value

    # Let's handle specific patterns more carefully:

    # VAR.field = val -> unsafe { (*VAR.get()).field = val; }
    # But we need to NOT match inside already-fixed code

    # Let's try a different approach - use regex with word boundary

    bvar = re.escape(var)

    # Pattern: &*std::ptr::addr_of!(VAR) -> &*VAR.get()
    old = f"&*std::ptr::addr_of!({var})"
    new = f"&*{var}.get()"
    if old in content:
        content = content.replace(old, new)
        fixes += 1

    old = f"&mut *std::ptr::addr_of_mut!({var})"
    new = f"&mut *{var}.get()"
    if old in content:
        content = content.replace(old, new)
        fixes += 1

    # Pattern: &mut *std::ptr::addr_of_mut!(VAR) -> &mut *VAR.get()
    old = f"&mut *std::ptr::addr_of_mut!({var})"
    new = f"&mut *{var}.get()"
    if old in content:
        content = content.replace(old, new)
        fixes += 1

    # core::ptr::addr_of!(VAR) -> VAR.get() as *const _
    old = f"core::ptr::addr_of!({var})"
    new = f"{var}.get() as *const _"
    if old in content:
        content = content.replace(old, new)
        fixes += content.count(new)

    old = f"core::ptr::addr_of_mut!({var})"
    new = f"{var}.get()"
    if old in content:
        content = content.replace(old, new)
        fixes += content.count(new)

    # Pattern: unsafe { VAR } where VAR is the sole expression in unsafe block
    # This appears in return statements like: unsafe { VAR }
    # Replace with: unsafe { VAR.read() }
    # Be careful: only if VAR is followed by } or newline then }
    content, n = re.subn(
        r"unsafe\s*\{\s*" + bvar + r"\s*\}", f"unsafe {{ {var}.read() }}", content
    )
    fixes += n

    # Pattern: unsafe { VAR } as Type -> VAR.get() as Type
    content, n = re.subn(
        r"unsafe\s*\{\s*" + bvar + r"\s*\}\s*as\s", f"{var}.get() as ", content
    )
    fixes += n

    return content, fixes


def fix_global_access_advanced(content, var):
    """Fix more complex Global<T> access patterns using line-by-line analysis."""
    bvar = re.escape(var)
    fixes = 0
    lines = content.split("\n")
    new_lines = []

    i = 0
    while i < len(lines):
        line = lines[i]
        original = line

        # Skip comments
        stripped = line.lstrip()
        if (
            stripped.startswith("//")
            or stripped.startswith("/*")
            or stripped.startswith("*")
        ):
            new_lines.append(line)
            i += 1
            continue

        # Skip if this is a declaration line
        if re.match(rf"^static\s+{bvar}\s*:", line):
            new_lines.append(line)
            i += 1
            continue

        # Skip if already has .get() or .read() or .write()
        if f"{var}.get()" in line or f"{var}.read()" in line or f"{var}.write(" in line:
            new_lines.append(line)
            i += 1
            continue

        # Skip if inside a string literal (simple heuristic)
        # Actually this is hard to do perfectly, let's just be careful

        # Pattern: VAR = expr; (assignment to the whole variable)
        # This should be: VAR.write(expr);
        # But NOT: VAR.field = expr; or VAR[idx] = expr;
        m = re.match(rf"^(\s*){bvar}\s*=\s*(.+?)\s*;", line)
        if m:
            indent = m.group(1)
            rhs = m.group(2).rstrip()
            line = f"{indent}{var}.write({rhs});"
            fixes += 1
            new_lines.append(line)
            i += 1
            continue

        # Pattern: VAR.field = expr; -> unsafe { (*VAR.get()).field = expr; }
        m = re.match(rf"^(\s*){bvar}\.(\w+)\s*=\s*(.+?)\s*;", line)
        if m:
            indent = m.group(1)
            field = m.group(2)
            rhs = m.group(3).rstrip()
            line = f"{indent}unsafe {{ (*{var}.get()).{field} = {rhs}; }}"
            fixes += 1
            new_lines.append(line)
            i += 1
            continue

        # Pattern: unsafe { VAR.field } (read) -> unsafe { (*VAR.get()).field }
        # But also handle: setpixel(p, CURRENT_DRAWSTATE.hue) etc.
        # Replace VAR.field with (*VAR.get()).field when VAR.field is used as read
        line = re.sub(
            rf"\b{bvar}\.(\w+)", lambda m2: f"(*{var}.get()).{m2.group(1)}", line
        )

        # Pattern: VAR[idx] -> (*VAR.get())[idx]
        line = re.sub(rf"\b{bvar}(\[)", lambda m2: f"(*{var}.get()){m2.group(1)}", line)

        # Pattern: VAR == val or val == VAR -> VAR.read() == val
        line = re.sub(
            rf"\b{bvar}\s*(==|!=|<=|>=|<|>)\s*",
            lambda m2: f"{var}.read() {m2.group(1)} ",
            line,
        )
        line = re.sub(rf"(?<!=)\s*{bvar}\s*$", lambda m2: f"{var}.read()", line)
        # Also: expr == VAR -> expr == VAR.read()
        line = re.sub(rf"(==|!=)\s+{bvar}\s*([,;)\]])", rf"\1 {var}.read()\2", line)

        # Pattern: VAR += n -> VAR.write(VAR.read() + n)
        line = re.sub(
            rf"\b{bvar}\s*\+=\s*(.+?)\s*;",
            lambda m2: f"{var}.write({var}.read() + {m2.group(1).rstrip()});",
            line,
        )

        # Pattern: VAR -= n -> VAR.write(VAR.read() - n)
        line = re.sub(
            rf"\b{bvar}\s*-=\s*(.+?)\s*;",
            lambda m2: f"{var}.write({var}.read() - {m2.group(1).rstrip()});",
            line,
        )

        # Pattern: VAR as Type (cast) -> VAR.get() as Type  (but NOT inside already-fixed code)
        line = re.sub(rf"\b{bvar}\s+as\s+", f"{var}.get() as ", line)

        # Pattern: !VAR (bool not) -> !VAR.read()
        line = re.sub(rf"!{bvar}\b", f"!{var}.read()", line)

        # Pattern: n - VAR or n + VAR or n * VAR etc. (binary ops where VAR is rhs)
        # This is for things like: val - VAR, where VAR needs .read()
        # We already handle == and != above

        # Pattern: *VAR (deref, for pointer types) -> *VAR.read()
        # This is tricky because it conflicts with (*VAR.get()) patterns
        # Only apply if NOT already followed by .get()
        line = re.sub(
            rf"\(\*{bvar}\)(\.)", lambda m2: f"(*{var}.read()){m2.group(1)}", line
        )

        if line != original:
            fixes += 1

        new_lines.append(line)
        i += 1

    return "\n".join(new_lines), fixes


def fix_thread_local_access(content, var):
    """Fix all access patterns for a thread_local Cell<T> variable."""
    bvar = re.escape(var)
    fixes = 0
    lines = content.split("\n")
    new_lines = []

    for line in lines:
        original = line

        # Skip comments
        stripped = line.lstrip()
        if (
            stripped.startswith("//")
            or stripped.startswith("/*")
            or stripped.startswith("*")
        ):
            new_lines.append(line)
            continue

        # Skip if already has .with(
        if f"{var}.with(" in line:
            new_lines.append(line)
            continue

        # Pattern: VAR = expr; -> VAR.with(|v| v.set(expr));
        m = re.match(rf"^(\s*){bvar}\s*=\s*(.+?)\s*;", line)
        if m:
            indent = m.group(1)
            rhs = m.group(2).rstrip()
            line = f"{indent}{var}.with(|v| v.set({rhs}));"
            fixes += 1
            new_lines.append(line)
            continue

        # Pattern: VAR += n -> VAR.with(|v| v.set(v.get() + n));
        line = re.sub(
            rf"\b{bvar}\s*\+=\s*(.+?)\s*;",
            lambda m2: f"{var}.with(|v| v.set(v.get() + {m2.group(1).rstrip()}));",
            line,
        )

        # Pattern: VAR -= n -> VAR.with(|v| v.set(v.get() - n));
        line = re.sub(
            rf"\b{bvar}\s*-=\s*(.+?)\s*;",
            lambda m2: f"{var}.with(|v| v.set(v.get() - {m2.group(1).rstrip()}));",
            line,
        )

        # Pattern: VAR[idx] = expr -> VAR.with(|v| v.get()[idx] = expr);
        # Pattern: VAR[idx] (read) -> VAR.with(|v| v.get()[idx])
        line = re.sub(
            rf"\b{bvar}(\[[^\]]+\])\s*=\s*(.+?)\s*;",
            lambda m2: (
                f"{var}.with(|v| v.get(){m2.group(1)} = {m2.group(2).rstrip()});"
            ),
            line,
        )
        line = re.sub(rf"\b{bvar}(\[[^\]]+\])", f"{var}.with(|v| v.get()\\1)", line)

        # Pattern: VAR == val -> VAR.with(|v| v.get() == val)
        line = re.sub(
            rf"\b{bvar}\s*(==|!=)\s*",
            lambda m2: f"{var}.with(|v| v.get() {m2.group(1)} ",
            line,
        )

        # Pattern: VAR.method() -> VAR.with(|v| v.get().method())
        line = re.sub(
            rf"\b{bvar}\.(\w+)\(",
            lambda m2: f"{var}.with(|v| v.get().{m2.group(1)}(",
            line,
        )

        # Pattern: VAR as Type -> VAR.with(|v| v.get() as Type)
        line = re.sub(rf"\b{bvar}\s+as\s+", f"{var}.with(|v| v.get() as ", line)

        # Pattern: std::ptr::addr_of!(VAR) -> VAR.with(|v| v.as_ptr())
        old = f"std::ptr::addr_of!({var})"
        new = f"{var}.with(|v| v.as_ptr())"
        if old in line:
            line = line.replace(old, new)
            fixes += 1

        old = f"std::ptr::addr_of_mut!({var})"
        new = f"{var}.with(|v| v.as_ptr())"
        if old in line:
            line = line.replace(old, new)
            fixes += 1

        if line != original:
            fixes += 1

        new_lines.append(line)

    return "\n".join(new_lines), fixes


def process_file(filepath):
    """Process a single file, fixing all access patterns."""
    with open(filepath, "r") as f:
        content = f.read()

    original = content

    # Find all Global<T> and thread_local variables
    global_vars = find_global_vars(content)
    tl_vars = find_thread_local_vars(content)

    if not global_vars and not tl_vars:
        return 0

    total_fixes = 0

    # Fix Global<T> patterns - pass 1: addr_of patterns
    for var in global_vars:
        content, n = fix_global_access(content, var)
        total_fixes += n

    # Fix Global<T> patterns - pass 2: line-by-line
    for var in global_vars:
        content, n = fix_global_access_advanced(content, var)
        total_fixes += n

    # Fix thread_local patterns
    for var in tl_vars:
        content, n = fix_thread_local_access(content, var)
        total_fixes += n

    if content != original:
        with open(filepath, "w") as f:
            f.write(content)

    return total_fixes


def main():
    total = 0
    files_fixed = 0
    for root, dirs, files in os.walk(SRC_DIR):
        # Skip support directory
        if "support" in root:
            continue
        for fname in files:
            if fname.endswith(".rs"):
                filepath = os.path.join(root, fname)
                n = process_file(filepath)
                if n > 0:
                    print(f"  Fixed {n} patterns in {filepath}")
                    total += n
                    files_fixed += 1

    print(f"\nTotal: {total} patterns fixed in {files_fixed} files")


if __name__ == "__main__":
    main()
