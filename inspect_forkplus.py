#!/usr/bin/env python3
"""Inspect ForkPlus.exe .NET metadata for the treemap P/Invoke ABI.

Extracts:
  * TypeDef rows for BtRect / BtTreemapItem / BtLayoutTreemapResult with their
    FieldLayout (explicit offsets) and PackingClass/ClassSize.
  * ImplMap (P/Invoke) entries for bt_layout_treemap / bt_release_layout_treemap
    with the unmanaged calling convention.
  * Method signatures for the managed wrappers (to infer parameter marshalling).
"""
import struct
import sys

import dnfile
from dnfile.enums import MetadataTables

TARGET_TYPES = {"BtRect", "BtTreemapItem", "BtLayoutTreemapResult"}
TARGET_IMPORTS = {"bt_layout_treemap", "bt_release_layout_treemap"}


def main(path):
    pe = dnfile.dnPE(path)
    md = pe.net.mdtables
    if md is None:
        print("no metadata tables")
        return

    # ---- FieldLayout: Field -> offset (explicit struct layout) ----
    field_layout = {}  # field_rid -> offset
    fl_table = md.FieldLayout
    if fl_table is not None:
        for row in fl_table.rows:
            field_layout[row.Field.row_index] = row.Offset

    # ---- FieldRVA: not relevant; skip ----

    # ---- TypeDef -> fields ----
    typedef_fields = {}  # typedef_rid -> list of (field_rid, name)
    field_names = {}
    f_table = md.Field
    if f_table is not None:
        for i, row in enumerate(f_table.rows, start=1):
            field_names[i] = row.Name

    # ClassLayout (ExplicitLayout / Pack / Size)
    class_layout = {}  # typedef_rid -> (pack, size)
    cl_table = md.ClassLayout
    if cl_table is not None:
        for row in cl_table.rows:
            class_layout[row.Parent.row_index] = (row.PackingSize, row.ClassSize)

    td_table = md.TypeDef
    print("==== TypeDefs of interest ====")
    if td_table is not None:
        # Build per-typedef field range from the FieldList coded index stream.
        td_field_start = []
        for row in td_table.rows:
            fl = row.FieldList
            # FieldList is a SimpleIndex into Field table; row_index gives 1-based rid
            try:
                start_idx = fl.row_index
            except AttributeError:
                # dnfile sometimes returns the row directly
                start_idx = fl.row_index if hasattr(fl, "row_index") else 0
            td_field_start.append(start_idx)
        n_fields = len(f_table.rows) if f_table else 0
        for ti, row in enumerate(td_table.rows, start=1):
            name = str(row.TypeName)
            if name in TARGET_TYPES or name.startswith("Bt"):
                ns = str(row.TypeNamespace) or "-"
                pack_size = class_layout.get(ti)
                print(f"\nTypeDef #{ti}: {ns}.{name}  (Extends={row.Extends})")
                if pack_size:
                    print(f"  ClassLayout: Pack={pack_size[0]} Size={pack_size[1]}")
                start = td_field_start[ti - 1]
                end = (
                    td_field_start[ti] if ti < len(td_field_start) else n_fields + 1
                )
                for fr in range(start, end):
                    if fr == 0 or fr > n_fields:
                        continue
                    fname = field_names.get(fr, "?")
                    off = field_layout.get(fr)
                    print(f"    Field #{fr}: {fname}  offset={off}")

    # ---- ImplMap: P/Invoke entries ----
    print("\n==== ImplMap (P/Invoke) entries ====")
    im_table = md.ImplMap
    if im_table is not None:
        for row in im_table.rows:
            iname = str(row.ImportName)
            if iname in TARGET_IMPORTS or "treemap" in iname.lower():
                # MappingFlags: low bits = calling convention
                flags = row.MappingFlags
                cc = flags & 0x7
                cc_names = {
                    1: "Cdecl",
                    2: "Stdcall",
                    3: "Thiscall",
                    4: "Fastcall",
                    5: "Varargs",
                }
                # MemberForwarded is a coded index to MethodDef
                mf = row.MemberForwarded
                print(
                    f"  {iname}  flags=0x{flags:x} (cc={cc_names.get(cc, cc)})  "
                    f"ModuleRef={row.ImportScope.row_index}  forwarded={mf}"
                )

    # ---- MethodDef signatures for the managed wrappers ----
    # We want bt_layout_treemap's managed signature to see param types.
    print("\n==== MethodDef signatures for treemap wrappers ====")
    me_table = md.MethodDef
    if me_table is not None:
        for mi, row in enumerate(me_table.rows, start=1):
            mname = str(row.Name)
            if "layout_treemap" in mname.lower() or "LayoutTreemap" in mname:
                sig = row.Signature
                print(f"  Method #{mi}: {mname}  Flags=0x{row.Flags:x}  ImplFlags=0x{row.ImplFlags:x}")
                print(f"    sig bytes: {sig.hex() if sig else None}")
                parse_method_sig(sig)

    # ---- Param table: names + marshalling for the treemap method ----
    print("\n==== Param table (names + marshal) for treemap methods ====")
    p_table = md.Param
    if p_table is not None and me_table is not None:
        for mi, mrow in enumerate(me_table.rows, start=1):
            mname = str(mrow.Name)
            if "layout_treemap" not in mname.lower() and "LayoutTreemap" not in mname:
                continue
            start = mrow.ParamList.row_index
            end = (
                me_table.rows[mi].ParamList.row_index
                if mi < len(me_table.rows)
                else (len(p_table.rows) + 1 if p_table else start)
            )
            print(f"  Method #{mi} {mname} params:")
            for pr in range(start, end):
                if pr == 0 or pr > len(p_table.rows):
                    continue
                prow = p_table.rows[pr - 1]
                # FieldMarshal table maps Param -> native type
                print(f"    Param #{pr}: seq={prow.Sequence} name={prow.Name} flags=0x{prow.Flags:x}")


def parse_method_sig(sig):
    """Best-effort parse of a MethodDefSig blob."""
    if not sig:
        return
    try:
        b = sig
        idx = 0
        cc = b[idx]
        idx += 1
        cc_desc = {0x0: "DEFAULT", 0x5: "VARARG", 0x10: "GENERIC", 0x20: "HASTHIS"}.get(
            cc, f"0x{cc:x}"
        )
        # generic param count if GENERIC
        if cc & 0x10:
            gpc = b[idx]
            idx += 1
            cc_desc += f" generic={gpc}"
        # param count (compressed int)
        pcount, used = read_compressed_uint(b, idx)
        idx += used
        print(f"    callingconv={cc_desc} paramcount={pcount}")
        # return type
        rt, used = read_type(b, idx)
        idx += used
        print(f"    return={rt}")
        # param types
        for i in range(pcount):
            pt, used = read_type(b, idx)
            idx += used
            print(f"    param[{i}]={pt}")
    except Exception as e:
        print(f"    (sig parse error: {e})")


def read_compressed_uint(b, idx):
    if idx >= len(b):
        return 0, 1
    c = b[idx]
    if (c & 0x80) == 0:
        return c, 1
    if (c & 0xC0) == 0x80:
        return ((c & 0x3F) << 8) | b[idx + 1], 2
    return (
        ((c & 0x1F) << 24) | (b[idx + 1] << 16) | (b[idx + 2] << 8) | b[idx + 3],
        4,
    )


def read_type(b, idx):
    if idx >= len(b):
        return "?", 1
    t = b[idx]
    used = 1
    primitive = {
        0x02: "bool", 0x03: "char", 0x04: "i1", 0x05: "u1", 0x06: "i2",
        0x07: "u2", 0x08: "i4", 0x09: "u4", 0x0A: "i8", 0x0B: "u8",
        0x0C: "r4", 0x0D: "r8", 0x0E: "string", 0x18: "i", 0x19: "u",
        0x1C: "object", 0x50: "bool&", 0x51: "char&", 0x52: "i1&",
        0x53: "u1&", 0x54: "i2&", 0x55: "u2&", 0x56: "i4&", 0x57: "u4&",
        0x58: "i8&", 0x59: "u8&", 0x5A: "r4&", 0x5B: "r8&", 0x5C: "string&",
        0x68: "i&", 0x69: "u&",
    }
    if t in primitive:
        return primitive[t], used
    if t == 0x0F:  # PTR <type>
        inner, u2 = read_type(b, idx + 1)
        return f"PTR<{inner}>", used + u2
    if t == 0x10:  # BYREF
        inner, u2 = read_type(b, idx + 1)
        return f"{inner}&", used + u2
    if t == 0x11:  # VALUETYPE TypeDefOrRef
        tok, u2 = read_compressed_uint(b, idx + 1)
        return f"VALUETYPE<token={tok:#x}>", used + u2
    if t == 0x12:  # CLASS TypeDefOrRef
        tok, u2 = read_compressed_uint(b, idx + 1)
        return f"CLASS<token={tok:#x}>", used + u2
    if t == 0x1D:  # ARRAY
        inner, u2 = read_type(b, idx + 1)
        return f"{inner}[]", used + u2
    return f"type=0x{t:x}", used


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ref_dll/ForkPlus.exe")
