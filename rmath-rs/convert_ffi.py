#!/usr/bin/env python3
"""
Convert pub unsafe extern "C" fn with #[unsafe(no_mangle)] to plain pub unsafe fn
for functions NOT part of R's public C API.
"""

import re
import sys
import os

# Keep list - function names/prefixes that should remain as extern "C"
KEEP_PREFIXES = [
    "Rf_",
    "R_",
    "SET_",
]

KEEP_NAMES = {
    "TYPEOF",
    "LENGTH",
    "XLENGTH",
    "TRUELENGTH",
    "ATTRIB",
    "OBJECT",
    "NAMED",
    "LEVELS",
    "ALTREP",
    "MARK",
    "CAR",
    "CDR",
    "TAG",
    "CADR",
    "CAAR",
    "CDDR",
    "CDAR",
    "CADDR",
    "CDDDR",
    "CADDDR",
    "CAD5R",
    "PRINTNAME",
    "SYMVALUE",
    "INTERNAL",
    "FORMALS",
    "BODY",
    "CLOENV",
    "FRAME",
    "ENCLOS",
    "HASHTAB",
    "PRVALUE",
    "PRCODE",
    "PRENV",
    "PRIMOFFSET",
    "DATAPTR",
    "ROBJ_DATAPTR",
    "LOGICAL",
    "INTEGER",
    "REAL",
    "COMPLEX",
    "RAW",
    "CHAR",
    "STRING_ELT",
    "VECTOR_ELT",
    "LOGICAL_ELT",
    "INTEGER_ELT",
    "REAL_ELT",
    "COMPLEX_ELT",
    "RAW_ELT",
    "SCALAR_LVAL",
    "SCALAR_IVAL",
    "SCALAR_DVAL",
    "SET_STRING_ELT",
    "SET_VECTOR_ELT",
    "SET_LOGICAL_ELT",
    "SET_INTEGER_ELT",
    "SET_REAL_ELT",
    "SET_COMPLEX_ELT",
    "SET_RAW_ELT",
    "allocVector",
    "allocVector3",
    "cons",
    "lang2",
    "lang3",
    "allocList",
    "mkChar",
    "mkCharLen",
    "mkString",
    "ScalarLogical",
    "ScalarInteger",
    "ScalarReal",
    "ScalarComplex",
    "ScalarString",
    "ScalarRaw",
    "length",
    "isSymbol",
    "isList",
    "isInteger",
    "isReal",
    "isComplex",
    "isLogical",
    "isString",
    "isRaw",
    "isVector",
    "isVectorAtomic",
    "isFunction",
    "isEnvironment",
    "protect",
    "unprotect",
    "unprotect_ptr",
    "ProtectWithIndex",
    "FreeProtectIndex",
    "Reprotect",
    "PreserveObject",
    "ReleaseObject",
    "install",
    "installChar",
    "findVarInFrame",
    "findVar",
    "defineVar",
    "setVar",
    "findFun",
    "findFun3",
    "matchArgs",
    "matchArgs_NR",
    "isMissing",
    "ddfindVar",
    "typeToChar",
    "NewHashedEnv",
    "existsVarInFrame",
    "eval",
    "applyClosure",
    "set_seed",
    "get_seed",
    "unif_rand",
    "NewEnvironment",
    "mkPROMISE",
    "mkEVPROMISE",
    "allocSExp",
    "allocLang",
    "R_alloc",
    "vmaxget",
    "vmaxset",
    "NilValue",
    "UnboundValue",
    "MissingArg",
    "RestartToken",
    "GlobalEnv",
    "BaseEnv",
    "EmptyEnv",
    "Visible",
    "EvalDepth",
    "EvalDepthLimit",
    "True",
    "False",
    "Rprintf",
    "REprintf",
    "PrintValue",
    "PrintValueEnv",
}


def should_keep(func_name):
    """Check if function should remain as extern C"""
    # Check exact name match
    if func_name in KEEP_NAMES:
        return True
    # Check prefix match
    for prefix in KEEP_PREFIXES:
        if func_name.startswith(prefix):
            return True
    return False


def process_file(filepath):
    """Process a single file and convert functions that shouldn't be extern C"""
    with open(filepath, "r") as f:
        content = f.read()

    original = content
    converted = 0
    kept = 0

    # Pattern to match #[unsafe(no_mangle)] followed by pub unsafe extern "C" fn
    # We need to handle multi-line cases
    pattern = r'#\[unsafe\(no_mangle\)\]\s*\n\s*pub unsafe extern "C" fn (\w+)'

    def replace_func(match):
        nonlocal converted, kept
        func_name = match.group(1)
        if should_keep(func_name):
            kept += 1
            return match.group(0)  # Keep as-is
        else:
            converted += 1
            # Replace the #[unsafe(no_mangle)]\npub unsafe extern "C" fn with pub unsafe fn
            return f"pub unsafe fn {func_name}"

    content = re.sub(pattern, replace_func, content)

    if content != original:
        with open(filepath, "w") as f:
            f.write(content)

    return converted, kept


def main():
    files = sys.argv[1:]
    total_converted = 0
    total_kept = 0

    for filepath in files:
        if not os.path.exists(filepath):
            print(f"Skipping {filepath}: not found")
            continue
        converted, kept = process_file(filepath)
        total_converted += converted
        total_kept += kept
        print(f"{filepath}: converted={converted}, kept={kept}")

    print(f"\nTotal: converted={total_converted}, kept={total_kept}")


if __name__ == "__main__":
    main()
