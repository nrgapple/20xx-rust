#!/usr/bin/env python3
"""Build the official 20XX 5.0.2 image, checking both full-disc MD5s."""

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
BASE_MD5 = "0e63d4223b01d9aba596259dc155a174"
OUTPUT_MD5 = "d926ba5b39551f5245fd655bc1dfeb3f"


def md5(path):
    digest = hashlib.md5()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, default=ROOT / "originals/melee-ntsc-1.02.iso")
    parser.add_argument("--output", type=Path, default=ROOT / "originals/20xx-hp-5.0.2.iso")
    parser.add_argument(
        "--patch", type=Path,
        default=ROOT / "originals/downloads/creator/20XXHP 5.0.2 Creator/SSBM, 20XXHP 5.0.2 patch.xdelta",
    )
    args = parser.parse_args()
    xdelta = shutil.which("xdelta3") or shutil.which("xdelta")
    if xdelta is None:
        parser.error("xdelta3 is required (macOS: brew install xdelta)")
    if os.path.lexists(args.output):
        parser.error(f"refusing to overwrite {args.output}")
    for path in (args.base, args.patch):
        if not path.is_file():
            parser.error(f"missing input: {path}")
    actual = md5(args.base)
    if actual != BASE_MD5:
        parser.error(f"base MD5 mismatch: expected {BASE_MD5}, got {actual}")
    print(f"Base MD5 verified: {actual}", flush=True)
    with tempfile.TemporaryDirectory(prefix="20xx-build-", dir=args.output.parent) as folder:
        candidate = Path(folder) / "20xx.iso"
        subprocess.run([xdelta, "-d", "-s", str(args.base), str(args.patch), str(candidate)], check=True)
        actual = md5(candidate)
        if actual != OUTPUT_MD5:
            parser.error(f"output MD5 mismatch: expected {OUTPUT_MD5}, got {actual}")
        # Atomic publication without overwriting a file created in the meantime.
        os.link(candidate, args.output)
    print(f"20XX 5.0.2 MD5 verified: {actual}\nCreated {args.output}")


if __name__ == "__main__":
    main()
