#!/usr/bin/env python3
"""Verify that the Biturbo dynamic library exports every symbol declared in biturbo.def.

Usage:
    python3 scripts/check_exports.py <lib> <biturbo.def>

Platform behavior:
    - Windows (.dll): runs `dumpbin /exports` (requires MSVC tools in PATH —
      run from a Developer Command Prompt or after `ilammy/msvc-dev-cmd`).
    - Linux   (.so)  : runs `nm -D --defined-only`.
    - macOS   (.dylib): runs `nm -gU`.

Parses the .def file for exported symbol names and reports any missing exports.
Exits non-zero on mismatch. "Extra" exports are informational only (libgit2/zlib
re-exports) and are NOT a failure.
"""
import os
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


def parse_dumpbin(lib_path):
    """Windows: parse `dumpbin /exports` output."""
    result = subprocess.run(
        ["dumpbin", "/exports", lib_path],
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


def parse_nm(lib_path, extra_args):
    """Linux/macOS: parse `nm` output."""
    cmd = ["nm"] + extra_args + [lib_path]
    result = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        sys.stderr.write("nm failed:\n" + result.stderr)
        sys.exit(2)

    names = set()
    # nm output columns: `<addr> <type> <name>` (defined) or `         U <name>` (undef).
    # We only collect defined symbols (uppercase type letter).
    for line in result.stdout.splitlines():
        parts = line.split()
        if len(parts) < 3:
            # Windows-style "U name" lines (only 2 fields) are undefined, skip.
            continue
        # parts[-1] is the symbol name; parts[-2] is the type letter.
        sym_type = parts[-2]
        sym_name = parts[-1]
        # Lowercase = local; uppercase = external (defined). Skip undefined (U).
        if sym_type == "U":
            continue
        if sym_type.isupper():
            # Strip symbol versioning suffixes like `name@@VERSION` or `name@VERSION`
            # emitted by ld when a version script is used (Linux) — we only care
            # about the unversioned symbol name for comparison with biturbo.def.
            sym_name = sym_name.split("@", 1)[0]
            # macOS may decorate symbols with a leading `_` underscore; strip it
            # for cross-platform comparison with the .def names.
            if sym_name.startswith("_"):
                sym_name = sym_name[1:]
            names.add(sym_name)
    return names


def parse_exports(lib_path):
    """Dispatch to the right parser based on platform / file extension."""
    lower = lib_path.lower()
    if lower.endswith(".dll"):
        return parse_dumpbin(lib_path)
    if lower.endswith(".so") or ".so." in os.path.basename(lower):
        # nm -D --defined-only on Linux ELF shared objects.
        return parse_nm(lib_path, ["-D", "--defined-only"])
    if lower.endswith(".dylib"):
        # nm -gU on macOS Mach-O dylibs (g=extern, U=undefined, uppercase=defined).
        return parse_nm(lib_path, ["-gU"])
    # Fallback: try dumpbin (Windows hosts).
    return parse_dumpbin(lib_path)


def main():
    if len(sys.argv) != 3:
        sys.stderr.write("usage: check_exports.py <lib> <biturbo.def>\n")
        sys.exit(2)

    lib, defp = sys.argv[1], sys.argv[2]
    expected = parse_def(defp)
    actual = parse_exports(lib)

    missing = expected - actual
    extra = actual - expected

    print(f"Expected (from {defp}): {len(expected)} symbols")
    print(f"Actual   (from {lib}):  {len(actual)} symbols")

    # The cdylib intentionally re-exports libgit2/zlib symbols from its
    # statically-linked deps, so "extra" exports are expected and are NOT a
    # failure. The check that matters: every symbol declared in .def must be
    # present in the lib (catches accidental drop-outs / renames).
    ok = True
    if missing:
        ok = False
        print(f"\nMISSING exports ({len(missing)}): declared in .def but NOT exported by lib")
        for name in sorted(missing):
            print(f"  - {name}")
    if extra:
        # Informational only: show count + up to 10 samples, sorted.
        sample = sorted(extra)[:10]
        print(f"\nNote: {len(extra)} extra symbols exported (libgit2/zlib re-exports); not a failure.")
        print("      First samples: " + ", ".join(sample))

    if ok:
        print(f"\nAll {len(expected)} declared exports present. OK")
        sys.exit(0)
    else:
        sys.exit(1)


if __name__ == "__main__":
    main()
