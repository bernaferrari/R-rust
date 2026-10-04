"""Regenerate console bytes with the Rscript executable from oracle/r-oracle.json."""

import argparse
import json
from pathlib import Path
import subprocess


CASES = [
    ("cat_no_newline", "cat('recovered')"),
    ("cat_blank_lines", "cat(' a  \\n\\n')"),
    ("cat_carriage_return", "cat('\\r')"),
    ("empty_invisible", "cat(''); invisible(7L)"),
    ("visible", "x <- 41; x + 1"),
    ("cat_then_visible", "cat('prefix'); 7L"),
    ("on_exit_then_visible", "f <- function(){on.exit(cat('exit'));return(7L)};f()"),
    ("intermediate_visible", "1L; 2L"),
    ("custom_print_no_newline", "print.zz<-function(x,...)cat('custom  ');structure(1,class='zz')"),
    ("custom_print_blank_lines", "print.zz<-function(x,...)cat('custom\\n\\n');structure(1,class='zz')"),
    ("intermediate_custom_print", "print.zz<-function(x,...)cat('custom  ');structure(1,class='zz');2L"),
    ("cat_then_error", "cat('before  ');stop('boom')"),
    ("cat_blank_lines_then_error", "cat('before\\n\\n');stop('boom')"),
    ("custom_print_then_error", "print.zz<-function(x,...) {cat('before  ');stop('boom')};structure(1,class='zz')"),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("rscript", type=Path)
    options = parser.parse_args()
    output = []
    for name, code in CASES:
        result = subprocess.run(
            [str(options.rscript), "--vanilla", "-e", code],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
        expected_code = 1 if name.endswith("error") else 0
        if result.returncode != expected_code:
            raise RuntimeError(f"{name}: unexpected exit {result.returncode}: {result.stderr!r}")
        output.append({"name": name, "code": code, "stdout": result.stdout.decode("utf-8"), "error": expected_code != 0})
    path = Path(__file__).with_name("expected.json")
    path.write_text(json.dumps(output, indent=2, ensure_ascii=False) + "\n")
    print(f"Generated {len(output)} independent console cases in {path}")


if __name__ == "__main__":
    main()
