#!/usr/bin/env python3
"""Reproduce the UTF-8 Perl class fixtures with an explicitly supplied GNU R."""
import argparse
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "crates/r-embed/tests/fixtures"


def r_string(value):
    # R rejects literal bidi controls in source and JSON surrogate pairs are not R.
    return '"' + ''.join(
        f"\\U{ord(char):08x}" if ord(char) > 127 else json.dumps(char)[1:-1]
        for char in value
    ) + '"'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rscript", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    def run(name, source):
        path = args.output_dir / (name + ".R")
        path.write_text(source, encoding="utf-8")
        result = subprocess.run(
            [str(args.rscript.resolve()), str(path.resolve())],
            env=dict(os.environ, LC_ALL="en_US.UTF-8"),
            capture_output=True, timeout=180, check=True,
        )
        (args.output_dir / (name + ".stdout")).write_bytes(result.stdout)
        (args.output_dir / (name + ".stderr")).write_bytes(result.stderr)
        if result.stderr:
            raise RuntimeError(f"GNU R diagnostics in {name}: {result.stderr!r}")
        return result.stdout.decode("utf-8")

    identity = run("oracle-identity", 'cat(R.version.string,"\\n",R.version[["svn rev"]],"\\n",extSoftVersion()[["PCRE"]],"\\n",sep="")').splitlines()
    if len(identity) != 3 or identity[1] != "90451" or identity[2] != "10.49 2026-09-28":
        raise RuntimeError(f"Expected pinned GNU R r90451 / PCRE2 10.49, got {identity!r}")

    corpus = json.loads((FIXTURES / "perl-character-class-kernel-corpus.json").read_text())
    cases = [row for row in corpus["cases"] if not row["bytes"]]
    source = 'subjects<-enc2utf8(c(' + ','.join(map(r_string, corpus["subjects"])) + '))\n'
    for row in cases:
        source += ('cat(tryCatch(paste0(as.integer(suppressWarnings(grepl('
                   + r_string(row["pattern"])
                   + ',subjects,perl=TRUE))),collapse=""),error=function(e) "ERROR"),"\\n",sep="")\n')
    actual = run("unicode-corpus", source).splitlines()
    expected = [row["utf8"] for row in cases]
    if actual != expected:
        raise RuntimeError("GNU UTF-8 pattern corpus differs from committed fixture")

    oracle = json.loads((FIXTURES / "perl-ucp-all-scalars.json").read_text())
    source = r'''
points<-c(1:55295,57344:1114111)
subjects<-intToUtf8(points,multiple=TRUE)
classes<-c("word","digit","space","alpha","alnum","blank","cntrl","lower","upper","graph","print","punct","xdigit","ascii")
for (kind in classes) {
 p<-points[grepl(paste0("(*UCP)\\A[[:",kind,":]]\\z"),subjects,perl=TRUE)]
 starts<-p[c(TRUE,diff(p)!=1L)]; ends<-p[c(diff(p)!=1L,TRUE)]
 cat(kind,":",paste(paste(starts,ends,sep="-"),collapse=","),"\n",sep="")
}
'''
    actual = {}
    for line in run("unicode-scalars", source).splitlines():
        kind, values = line.split(":")
        actual[kind] = [[int(n) for n in pair.split("-")] for pair in values.split(",")]
    if actual != oracle["ranges"]:
        raise RuntimeError("GNU Unicode scalar ranges differ from committed fixture")
    if run("unicode-class-public", (FIXTURES / "perl-character-class-contract.R").read_text()) != (FIXTURES / "perl-character-class-contract.out").read_text():
        raise RuntimeError("GNU public character-class print contract failed")
    for name in ["perl-class-family-contract", "perl-empty-pattern-contract", "gregexpr-capture-attribution-contract"]:
        actual = run(name, (FIXTURES / (name + ".R")).read_text())
        if actual != (FIXTURES / (name + ".out")).read_text():
            raise RuntimeError(f"GNU public UTF-8 contract failed: {name}")
    print(f"GNU UTF-8 oracle verified: {len(cases)} patterns, {len(oracle['ranges'])} complete scalar classes, 4 public contracts")


if __name__ == "__main__":
    main()
