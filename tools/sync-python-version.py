#!/usr/bin/env python3
"""Keep PyPI metadata on the same release version as the Cargo workspace."""

from pathlib import Path
import re
import sys


def main() -> None:
    version = sys.argv[1]
    path = Path(__file__).resolve().parents[1] / "pyproject.toml"
    text = path.read_text()
    updated, count = re.subn(
        r'(?m)^(version = ")[^"]+("\s*)$',
        rf"\g<1>{version}\g<2>",
        text,
        count=1,
    )
    if count != 1:
        raise SystemExit("could not find [project] version in pyproject.toml")
    path.write_text(updated)


if __name__ == "__main__":
    main()
