#!/usr/bin/env python3
"""Compatibility wrapper for the canonical `cargo xtask docs` command."""
from __future__ import annotations
import argparse
import subprocess

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    command = ["cargo", "xtask", "docs", "check" if args.check else "build"]
    return subprocess.run(command, check=False).returncode

if __name__ == "__main__":
    raise SystemExit(main())
