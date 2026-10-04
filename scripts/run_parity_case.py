#!/usr/bin/env python3
"""Run one parity process with exact streams and an owned POSIX deadline."""

from __future__ import annotations

import argparse
import math
import os
from pathlib import Path
import signal
import subprocess
import sys


def positive_seconds(value: str) -> float:
    try:
        seconds = float(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("timeout must be a positive finite number") from error
    if not math.isfinite(seconds) or seconds <= 0:
        raise argparse.ArgumentTypeError("timeout must be a positive finite number")
    return seconds


def stop_group(process: subprocess.Popen[bytes]) -> None:
    # The group was created for this exact subprocess. Even if the direct child
    # exits on TERM, a descendant can remain, so always finish group cleanup.
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(process.pid, sig)
        except ProcessLookupError:
            pass
        if sig == signal.SIGTERM:
            try:
                process.wait(timeout=0.25)
            except subprocess.TimeoutExpired:
                pass
    process.wait()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--timeout", type=positive_seconds, required=True)
    parser.add_argument("--timeout-marker", type=Path, required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        parser.error("a command is required")
    if os.name != "posix":
        parser.error("the Bash parity harness requires POSIX process groups")
    if args.timeout_marker.exists():
        parser.error("timeout marker must be absent before execution")
    try:
        process = subprocess.Popen(command, start_new_session=True)
    except OSError as error:
        print(f"Cannot start parity process: {error}", file=sys.stderr)
        return 2
    try:
        code = process.wait(timeout=args.timeout)
    except subprocess.TimeoutExpired:
        stop_group(process)
        args.timeout_marker.write_text(f"{args.timeout:g}\n", encoding="utf-8")
        print(f"TIMEOUT after {args.timeout:g}s: {Path(command[0]).name}", file=sys.stderr)
        return 124
    except BaseException:
        stop_group(process)
        raise
    return code if code >= 0 else 128 - code


if __name__ == "__main__":
    raise SystemExit(main())
