#!/usr/bin/env python3
"""Set the rustorb-dds version everywhere it is written down.

    tools/set-version.py 0.3.0      # rewrite the files
    tools/set-version.py --check    # fail if the files disagree

The version lives in the Cargo workspace (and its path dependencies), the
Python package, the Maven pom and the C++ CMake project. Run
`cargo update --workspace` after a rewrite so that `--locked` builds pass.
"""

import argparse
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
VERSION = r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?"

# (file, pattern with one group for the version, expected number of matches)
PLACES = [
    ("Cargo.toml", rf'(?m)^version = "({VERSION})"$', 1),
    ("Cargo.toml", rf'(?m)^rustorb-[a-z-]+ = \{{ path = "[^"]+", version = "({VERSION})"[^}}]*\}}$', 4),
    ("python/pyproject.toml", rf'(?m)^version = "({VERSION})"$', 1),
    ("python/rustorb_dds/__init__.py", rf'(?m)^__version__ = "({VERSION})"$', 1),
    ("java/pom.xml", rf"(?m)^  <version>({VERSION})</version>$", 1),
    ("cpp/CMakeLists.txt", rf"(?m)^project\(rustorb_dds_cpp VERSION ({VERSION}) LANGUAGES CXX\)$", 1),
    ("dotnet/src/Rustorb.Dds/Rustorb.Dds.csproj", rf"(?m)^    <Version>({VERSION})</Version>$", 1),
    ("ruby/lib/rustorb/dds/version.rb", rf'(?m)^  VERSION = "({VERSION})"$', 1),
]


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("version", nargs="?", help="the new version, e.g. 0.3.0")
    ap.add_argument("--check", action="store_true", help="only check that all places agree")
    args = ap.parse_args()
    if not args.check and not args.version:
        ap.error("give a version or --check")
    if args.version and not re.fullmatch(VERSION, args.version):
        ap.error(f"not a version: {args.version}")

    found = {}
    texts = {}
    for name, pattern, count in PLACES:
        text = texts.setdefault(name, (ROOT / name).read_text(encoding="utf-8"))
        matches = list(re.finditer(pattern, text))
        if len(matches) != count:
            sys.exit(f"{name}: expected {count} version(s) for {pattern!r}, found {len(matches)}")
        for m in matches:
            found.setdefault(m.group(1), []).append(name)

    if args.check:
        if len(found) != 1:
            for version, names in sorted(found.items()):
                print(f"{version}: {', '.join(sorted(set(names)))}", file=sys.stderr)
            sys.exit("the versions disagree")
        print(next(iter(found)))
        return

    for name, pattern, _ in PLACES:
        texts[name] = re.sub(
            pattern,
            lambda m: m.group(0)[: m.start(1) - m.start(0)]
            + args.version
            + m.group(0)[m.end(1) - m.start(0) :],
            texts[name],
        )
    for name, text in texts.items():
        (ROOT / name).write_text(text, encoding="utf-8")
    print(f"set the version to {args.version}; now run `cargo update --workspace`")


if __name__ == "__main__":
    main()
