#!/usr/bin/env python3
"""Writes dotnet/src/Rustorb.Dds/Native/Ffi.g.cs: a [LibraryImport]
declaration for each function of the C ABI
(crates/rustorb-dds-ffi/include/rustorb/dds.h).

    dotnet/tools/gen-pinvoke.py          # rewrite Ffi.g.cs
    dotnet/tools/gen-pinvoke.py --check  # fail if Ffi.g.cs is not up to date

The form is that of rustorb's dotnet/tools/gen-pinvoke.py. Handles
(pointers to the opaque structs of the ABI) are nint; the plain structs of
the header (rdds_duration, the QoS, the statuses, the listeners...) are
their C# layouts of Native/Structs.cs, by value or by pointer; bool is
byte (the C bool is one octet); char is byte; void* and function pointers
are nint. The stream functions, which neither call back nor block, are
marked [SuppressGCTransition].

Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HEADER = ROOT / "crates/rustorb-dds-ffi/include/rustorb/dds.h"
OUTPUT = ROOT / "dotnet/src/Rustorb.Dds/Native/Ffi.g.cs"

SCALARS = {
    "int32_t": "int", "uint32_t": "uint", "int": "int", "rdds_status": "int",
    "int64_t": "long", "uint64_t": "ulong", "size_t": "nuint",
    "int16_t": "short", "uint16_t": "ushort", "uint8_t": "byte", "int8_t": "sbyte",
    "char": "byte", "bool": "byte", "float": "float", "double": "double", "void": "void",
}

# The plain structs of the header and their C# layouts (Native/Structs.cs).
STRUCTS = {
    "rdds_duration": "RddsDuration",
    "rdds_time": "RddsTime",
    "rdds_guid": "RddsGuid",
    "rdds_instance_handle": "RddsInstanceHandle",
    "rdds_writer_qos": "RddsWriterQos",
    "rdds_reader_qos": "RddsReaderQos",
    "rdds_group_qos": "RddsGroupQos",
    "rdds_property": "RddsProperty",
    "rdds_participant_qos": "RddsParticipantQos",
    "rdds_matched_status": "RddsMatchedStatus",
    "rdds_incompatible_qos_status": "RddsIncompatibleQosStatus",
    "rdds_liveliness_changed_status": "RddsLivelinessChangedStatus",
    "rdds_count_status": "RddsCountStatus",
    "rdds_deadline_missed_status": "RddsDeadlineMissedStatus",
    "rdds_sample_rejected_status": "RddsSampleRejectedStatus",
    "rdds_writer_listener": "RddsWriterListener",
    "rdds_reader_listener": "RddsReaderListener",
    "rdds_listener": "RddsListener",
    "rdds_sample_info": "RddsSampleInfo",
    "rdds_sample_identity": "RddsSampleIdentity",
}

KEYWORDS = {"in", "out", "ref", "string", "object", "base", "params", "event", "fixed", "lock", "operator"}


def no_transition(name):
    """The stream functions: short, and they neither call back nor block."""
    if name in ("rdds_out_new", "rdds_out_release", "rdds_in_release"):
        return False
    return name.startswith(("rdds_out_", "rdds_in_"))


def declarations(text):
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    text = re.sub(r"//[^\n]*", "", text)
    text = re.sub(r"^\s*#.*$", "", text, flags=re.M)
    text = re.sub(r"typedef\s+struct\s+\w+\s*\{.*?\}\s*\w+\s*;", "", text, flags=re.S)
    text = re.sub(r"typedef[^;]*;", "", text)
    text = text.replace('extern "C" {', "").replace("}", "")
    for stmt in text.split(";"):
        s = " ".join(stmt.split())
        m = re.match(r"(.*?)\b(rdds_\w+)\s*\((.*)\)$", s)
        if m:
            yield m.group(1).strip(), m.group(2), m.group(3)


def split_args(args):
    parts, depth, cur = [], 0, ""
    for ch in args:
        if ch == "," and depth == 0:
            parts.append(cur.strip())
            cur = ""
            continue
        depth += ch == "("
        depth -= ch == ")"
        cur += ch
    if cur.strip():
        parts.append(cur.strip())
    return [] if parts == ["void"] else parts


def cs_type(fn, decl):
    decl = re.sub(r"\bconst\b", "", decl).strip()
    if "(" in decl:
        return "nint"
    base, stars = decl, 0
    while base.endswith("*"):
        base, stars = base[:-1].strip(), stars + 1
    if base in STRUCTS:
        return STRUCTS[base] + "*" * stars
    if base == "void" and stars == 1:
        return "nint"
    if base in SCALARS:
        return SCALARS[base] + "*" * stars
    if base.startswith("rdds_"):
        if stars == 0:
            raise SystemExit(f"{fn}: opaque type by value {decl!r}")
        return "nint" + "*" * (stars - 1)
    raise SystemExit(f"{fn}: unknown type {decl!r}")


def param(fn, decl):
    decl = decl.strip()
    m = re.match(r"(.*?)\(\s*\*\s*(\w+)\s*\)\s*\(.*\)$", decl)
    if m:
        return "nint", m.group(2)
    m = re.match(r"(.*?)(\w+)\s*\[\s*\w*\s*\]$", decl)
    if m:
        return cs_type(fn, m.group(1) + "*"), m.group(2)
    m = re.match(r"(.*?[\s\*])(\w+)$", decl)
    if not m:
        raise SystemExit(f"{fn}: cannot read parameter {decl!r}")
    return cs_type(fn, m.group(1)), m.group(2)


def generate():
    lines = []
    for ret, name, args in sorted(declarations(HEADER.read_text()), key=lambda d: d[1]):
        ps = []
        for a in split_args(args):
            t, n = param(name, a)
            if n in KEYWORDS:
                n = "@" + n
            ps.append(f"{t} {n}")
        r = cs_type(name, ret)
        attrs = "    [LibraryImport(Lib)]\n"
        if no_transition(name):
            attrs += "    [SuppressGCTransition]\n"
        lines.append(f"{attrs}    internal static partial {r} {name}({', '.join(ps)});")
    return TEMPLATE.replace("@DECLS@", "\n\n".join(lines))


TEMPLATE = """\
// Generated by dotnet/tools/gen-pinvoke.py from
// crates/rustorb-dds-ffi/include/rustorb/dds.h: do not edit.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

#nullable disable
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Rustorb.Dds.Native;

internal static unsafe partial class Ffi
{
    internal const string Lib = "rustorb_dds_ffi";

@DECLS@
}
"""


def main():
    text = generate()
    if "--check" in sys.argv:
        if not OUTPUT.exists() or OUTPUT.read_text() != text:
            raise SystemExit(f"{OUTPUT.relative_to(ROOT)} is out of date: run dotnet/tools/gen-pinvoke.py")
        return
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(text)


if __name__ == "__main__":
    main()
