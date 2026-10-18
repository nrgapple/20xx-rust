#!/usr/bin/env python3
"""Infer field types of a fighter's special attributes from 1.02 machine code.

Usage: attr_access.py [--base OFFSET] PREFIX [PREFIX...]   (e.g. ftPr_)

`--base` follows a different Fighter pointer field instead of `dat_attrs`
(0x2D4), e.g. `--base 0x2D0` for the multi-jump table.

For every function whose symbol starts with a PREFIX, follow registers loaded
from `dat_attrs` (`lwz rX, 0x2D4(rY)`) within straight-line code and report
each offset read or written through them as float (lfs/stfs), int (lwz/stw),
halfword, or byte. Register tracking is local and approximate: a register
stops being tracked when overwritten, and branches are ignored. Treat the
output as evidence to check, not proof.
"""
import re
import subprocess
import sys
from collections import defaultdict

ROOT = __file__.rsplit("/tools/", 1)[0]
DOL = f"{ROOT}/originals/melee-1.02.dol"
SYMBOLS = f"{ROOT}/reference/melee/config/GALE01/symbols.txt"
INSPECT = subprocess.run(["sh", "-c", f'cd "{ROOT}" && ~/.cargo/bin/cargo metadata --format-version 1 --no-deps'],
                         capture_output=True, text=True).stdout
TARGET = re.search(r'"target_directory":"([^"]+)"', INSPECT).group(1)
KINDS = {"lfs": "float", "stfs": "float", "lfd": "double", "stfd": "double",
         "lwz": "int", "stw": "int", "lha": "half", "lhz": "half", "sth": "half",
         "lbz": "byte", "stb": "byte"}


def functions(prefixes):
    for line in open(SYMBOLS):
        name = line.split(" = ", 1)[0]
        if "type:function" in line and name.startswith(tuple(prefixes)):
            yield name


def main():
    args = sys.argv[1:]
    base = 0x2D4
    if args[:1] == ["--base"]:
        base, args = int(args[1], 0), args[2:]
    prefixes = args
    if not prefixes:
        sys.exit(__doc__)
    names = list(functions(prefixes))
    out = subprocess.run([f"{TARGET}/debug/dol-inspect", DOL, SYMBOLS, *names],
                         capture_output=True, text=True, check=True).stdout
    seen = defaultdict(set)
    tracked = set()
    for line in out.splitlines():
        if " @ 0x" in line:
            tracked, function = set(), line.split(" @ ")[0]
            continue
        m = re.match(r"\s+[0-9a-f]{8}: [0-9a-f]{8}\s+(\S+)\s+(.*)", line)
        if not m:
            continue
        op, args = m.groups()
        mem = re.match(r"([rf]\d+), (-?\d+)\(r(\d+)\)", args)
        if op == "lwz" and mem and int(mem.group(2)) == base:
            tracked.add(int(mem.group(1)[1:]))
            continue
        if mem and int(mem.group(3)) in tracked and op in KINDS:
            seen[int(mem.group(2))].add((KINDS[op], function))
        dest = re.match(r"r(\d+)", args)
        if dest and not op.startswith("st") and not op.startswith("cmp") and op not in ("lfs", "lfd", "stfs", "stfd"):
            tracked.discard(int(dest.group(1)))
        if op.startswith("bl"):
            tracked -= {3, 4, 5, 6, 7, 8, 9, 10, 11, 12}
    for offset in sorted(seen):
        kinds = sorted({kind for kind, _ in seen[offset]})
        users = sorted({fn for _, fn in seen[offset]})
        print(f"+{offset:#05x} {'/'.join(kinds):<10} {', '.join(users[:3])}{' ...' if len(users) > 3 else ''}")


if __name__ == "__main__":
    main()
