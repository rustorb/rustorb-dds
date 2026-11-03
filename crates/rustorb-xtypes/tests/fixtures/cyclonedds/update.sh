#!/usr/bin/env bash
# Regenerates the TypeInformation and TypeObject fixtures from what
# Cyclone DDS's IDL compiler (idlc, package cyclonedds-tools) writes for
# the IDL in this directory. Cyclone DDS is used as a black box: only its
# output is kept.
#
# For each topic type T, two files of hex:
#   T.typeinfo  the TypeInformation (XCDR2, little endian)
#   T.typemap   idlc's type mapping: the minimal and the complete
#               TypeIdentifier/TypeObject pairs of T and of the types it
#               uses, and the complete-to-minimal TypeIdentifier pairs
set -eu
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for idl in *.idl; do
  idlc -x final -o "$tmp" "$idl"
done
idlc -v 2>&1 | head -1 > idlc-version.txt || true
python3 - "$tmp" <<'PY'
import glob, os, re, sys
for c in glob.glob(os.path.join(sys.argv[1], "*.c")):
    text = open(c).read()
    for kind, ext in (("INFO", "typeinfo"), ("MAP", "typemap")):
        pat = r"#define TYPE_%s_CDR_(\w+) \(unsigned char \[\]\)\{(.*?)\}" % kind
        for name, body in re.findall(pat, text, re.S):
            data = bytes(int(b, 16) for b in re.findall(r"0x([0-9a-f]{2})", body))
            with open("%s.%s" % (name, ext), "w") as f:
                for i in range(0, len(data), 32):
                    f.write(data[i:i + 32].hex() + "\n")
PY
