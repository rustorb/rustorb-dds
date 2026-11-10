#!/usr/bin/env python3
"""Prints the user DATA submessages of a capture: for each RTPS message
between the ports of the given domains, the sender's vendor and prefix,
then each DATA of a user writer (not a builtin one) in hex.

    rtps_dump.py capture.pcap DOMAIN... [--max N] [--all]

--all shows the builtin writers' DATA too.
"""
import struct
import sys


def packets(path):
    with open(path, "rb") as f:
        d = f.read()
    magic = struct.unpack("<I", d[:4])[0]
    e = "<" if magic in (0xA1B2C3D4, 0xA1B23C4D) else ">"
    link = struct.unpack(e + "I", d[20:24])[0]
    at = 24
    while at + 16 <= len(d):
        _, _, incl, _ = struct.unpack(e + "IIII", d[at:at + 16])
        p = d[at + 16:at + 16 + incl]
        at += 16 + incl
        if link == 1:  # Ethernet
            if p[12:14] != b"\x08\x00":
                continue
            p = p[14:]
        elif link == 113:  # Linux cooked
            p = p[16:]
        if not p or p[0] >> 4 != 4:
            continue
        ihl = (p[0] & 15) * 4
        if p[9] != 17:
            continue
        sport, dport = struct.unpack(">HH", p[ihl:ihl + 4])
        yield sport, dport, p[ihl + 8:]


def domain_of(port):
    # PB 7400, DG 250: every port of domain d is in [7400 + 250 d, + 250).
    return (port - 7400) // 250 if port >= 7400 else None


def main():
    args = sys.argv[1:]
    limit = 20
    builtin = "--all" in args
    args = [a for a in args if a != "--all"]
    if "--max" in args:
        i = args.index("--max")
        limit = int(args[i + 1])
        del args[i:i + 2]
    path, domains = args[0], {int(a) for a in args[1:]}
    shown = {}
    for sport, dport, u in packets(path):
        dom = domain_of(dport)
        if dom not in domains or u[:4] != b"RTPS":
            continue
        vendor, prefix = u[6:8].hex(), u[8:20].hex()
        at = 20
        while at + 4 <= len(u):
            sid, flags = u[at], u[at + 1]
            le = flags & 1
            n = struct.unpack("<H" if le else ">H", u[at + 2:at + 4])[0]
            body = u[at + 4:at + 4 + n] if n else u[at + 4:]
            if sid == 0x15 and len(body) >= 12:
                writer = body[8:12]
                key = (dom, prefix, writer.hex())
                if (builtin or writer[3] & 0xC0 != 0xC0) and shown.get(key, 0) < limit:
                    shown[key] = shown.get(key, 0) + 1
                    print(f"domain {dom} {sport}->{dport} vendor {vendor} prefix {prefix}"
                          f" DATA flags {flags:#04x} writer {writer.hex()} reader {body[4:8].hex()}")
                    sub = u[at:at + 4 + len(body)]
                    for i in range(0, min(len(sub), 192), 32):
                        print("  " + sub[i:i + 32].hex(" "))
            if n == 0:
                break
            at += 4 + n


if __name__ == "__main__":
    main()
