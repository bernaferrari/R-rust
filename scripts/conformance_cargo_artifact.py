#!/usr/bin/env python3
"""Select the actual rmath library reported by one successful Cargo build."""
import json
from pathlib import Path
import sys


def select_artifact(lines, crate="rmath", *, shared=False):
    artifacts = set()
    finished = []
    for number, line in enumerate(lines, 1):
        # cargo_dev emits its pruning receipt after Cargo's JSON stream.
        if not line.lstrip().startswith("{"):
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"invalid Cargo JSON on line {number}: {error}") from error
        if not isinstance(message, dict):
            continue
        if message.get("reason") == "build-finished" and message.get("success") is False:
            raise ValueError("Cargo reported an unsuccessful build")
        if message.get("reason") == "build-finished":
            finished.append(message.get("success"))
        if message.get("reason") != "compiler-artifact":
            continue
        target = message.get("target", {})
        profile = message.get("profile", {})
        if not isinstance(target, dict) or not isinstance(profile, dict):
            raise ValueError("invalid compiler-artifact target/profile")
        if target.get("name") != crate:
            continue
        kinds = target.get("kind", [])
        if not isinstance(kinds, list) or not all(isinstance(kind, str) for kind in kinds):
            raise ValueError("invalid compiler-artifact target kinds")
        if not any(kind in kinds for kind in (("cdylib",) if shared else ("lib", "rlib"))):
            continue
        if profile.get("test") is True or message.get("executable") is not None:
            continue
        filenames = message.get("filenames", [])
        if not isinstance(filenames, list) or not all(isinstance(name, str) for name in filenames):
            raise ValueError("invalid compiler-artifact filenames")
        for filename in filenames:
            if filename.endswith((".so", ".dylib", ".dll") if shared else (".rlib",)):
                if not Path(filename).is_file():
                    raise ValueError(f"Cargo artifact does not exist: {filename}")
                artifacts.add(filename)
    if shared and finished != [True]:
        raise ValueError("one successful shared-library build receipt is required")
    if len(artifacts) != 1:
        raise ValueError(f"expected one emitted {crate} library, found {len(artifacts)}")
    return artifacts.pop()


def main():
    try:
        with open(sys.argv[1], encoding="utf-8") as stream:
            artifact = select_artifact(stream, sys.argv[2] if len(sys.argv) > 2 else "rmath",
                                       shared="--shared" in sys.argv[3:])
    except (OSError, ValueError, IndexError) as error:
        print(f"Conformance Cargo artifact selection failed: {error}", file=sys.stderr)
        return 2
    print(artifact)
    return 0


if __name__ == "__main__":
    sys.exit(main())
