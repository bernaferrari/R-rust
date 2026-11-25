#!/usr/bin/env python3
"""Scan for dead pub functions: zero external references across crates/,
single internal occurrence (definition site only).

Usage: python3 scripts/dead_fn_scan.py <subdir> [<subdir>...]
  e.g. python3 scripts/dead_fn_scan.py mainutils eval library sexp

Writes deletion candidates to /tmp/deletion_candidates.json and prints
a per-file summary. Verified methodology used for commits f283e41b6 and
cab799353: candidates were brace-matched out and compiler-verified.

CAUTION: internal occ=1 means the name appears nowhere else even in its
own file. Functions used only inside their own file by live callers show
occ>=2 and are excluded here; analyze those separately. Macros that
construct identifiers dynamically can hide references — always verify
with a full compile after deletion.
"""
import re, subprocess, json, sys
from pathlib import Path
from collections import defaultdict

subs = sys.argv[1:] or ['mainutils']
defs = defaultdict(list)
for sub in subs:
    for p in Path(f'crates/rmath/src/{sub}').rglob('*.rs'):
        try:
            text = p.read_text()
        except Exception:
            continue
        for m in re.finditer(r'(?m)^pub(?:\(crate\))?\s+(?:unsafe\s+)?fn\s+([A-Za-z0-9_]+)', text):
            defs[m.group(1)].append(str(p))

names = list(defs.keys())
print(f"pub fn names scanned: {len(names)}")
if not names:
    sys.exit(0)
pat = '\\b(' + '|'.join(re.escape(n) for n in names) + ')\\b'
out = subprocess.run(['rg', '--with-filename', '-o', '-e', pat, 'crates', '-g', '*.rs'],
                     capture_output=True, text=True).stdout
perfile = defaultdict(lambda: defaultdict(int))
for line in out.splitlines():
    if ':' in line:
        f, _, nm = line.rpartition(':')
        perfile[nm.strip()][f] += 1

byfile = defaultdict(list)
for name, files in defs.items():
    occ = perfile.get(name, {})
    external = sum(c for f, c in occ.items() if f not in files)
    if external == 0:
        internal_total = sum(occ.get(f, 0) for f in files)
        if internal_total == len(files):
            for f in files:
                byfile[f].append(name)

total = sum(len(v) for v in byfile.values())
print(f"definition-only dead fns: {total} across {len(byfile)} files")
for f in sorted(byfile, key=lambda x: -len(byfile[x]))[:15]:
    print(f"  {len(byfile[f]):3d}  {f}")
json.dump({f: n for f, n in byfile.items()}, open('/tmp/deletion_candidates.json', 'w'))
print("candidates written to /tmp/deletion_candidates.json")
