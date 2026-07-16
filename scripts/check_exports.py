#!/usr/bin/env python3
"""Verify that biturbo.dll exports every symbol declared in biturbo.def.

Usage:
    python3 scripts/check_exports.py <biturbo.dll> <biturbo.def>

Parses the .def file for exported symbol names, runs `dumpbin /exports` on the
DLL (requires MSVC tools in PATH — run from a Developer Command Prompt or after
`ilammy/msvc-dev-cmd`), and reports any missing/extra exports. Exits non-zero on
mismatch.
"""
import re
import subprocess
import sys


def parse_def(path):
    """Return the set of symbol names declared in a .def file."""
    names = set()
    with open(path, "r", encoding="utf-8") as fh:
        for line in fh:
            stripped = line.strip()
            if not stripped:
                continue
            # Skip directives: LIBRARY, EXPORTS, SECTIONS, comments (;), NAME=...
            upper = stripped.upper()
            if upper.startswith(("LIBRARY", "EXPORTS", "SECTIONS", ";")):
                continue
            # A symbol line looks like:  NAME @ORDINAL   or   NAME @ORDINAL DATA
            # The exported name is the first whitespace-delimited token,
            # possibly with a leading decorator on x86 (not used on x64 here).
            token = stripped.split()[0]
            # Strip any "=internalname" aliasing on the exported side.
            token = token.split("=")[0]
            # Ignore ordinals-only entries like @5 with no name.
            if token.startswith("@"):
                continue
            names.add(token)
    return names


def parse_dumpbin(dll_path):
    """Return the set of symbol names that the DLL actually exports."""
    result = subprocess.run(
        ["dumpbin", "/exports", dll_path],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        sys.stderr.write("dumpbin failed:\n" + result.stderr)
        sys.exit(2)

    names = set()
    # Export table rows look like:
    #     ordinal hint RVA      name
    #           1     0 00001070 adler32
    # The name is the last whitespace-delimited field, and the first field is
    # an integer ordinal.
    row_re = re.compile(r"^\s*\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(\S+)")
    in_table = False
    for line in result.stdout.splitlines():
        if "ordinal" in line.lower() and "name" in line.lower():
            in_table = True
            continue
        if not in_table:
            continue
        m = row_re.match(line)
        if m:
            names.add(m.group(1))
        elif line.strip() == "":
            # Blank line ends the export table section.
            if names:
                break
    return names


def main():
    if len(sys.argv) != 3:
        sys.stderr.write("usage: check_exports.py <biturbo.dll> <biturbo.def>\n")
        sys.exit(2)

    dll, defp = sys.argv[1], sys.argv[2]
    expected = parse_def(defp)
    actual = parse_dumpbin(dll)

    missing = expected - actual
    extra = actual - expected

    print(f"Expected (from {defp}): {len(expected)} symbols")
    print(f"Actual   (from {dll}):  {len(actual)} symbols")

    ok = True
    if missing:
        ok = False
        print(f"\nMISSING exports ({len(missing)}): declared in .def but NOT exported by DLL")
        for name in sorted(missing):
            print(f"  - {name}")
    if extra:
        ok = False
        print(f"\nEXTRA exports ({len(extra)}): exported by DLL but NOT declared in .def")
        for name in sorted(extra):
            print(f"  + {name}")

    if ok:
        print("\nAll exports match. OK")
        sys.exit(0)
    else:
        sys.exit(1)


if __name__ == "__main__":
    main()
