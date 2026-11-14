#!/usr/bin/env python3
"""Prints the user DATA submessages of a capture: for each RTPS message
between the ports of the given domains, the sender's vendor and prefix,
then each DATA of a user writer (not a builtin one) in hex.

    rtps_dump.py capture.pcap DOMAIN... [--max N] [--all] [--trace]

--all shows the builtin writers' DATA too. --trace shows instead one line
for each DATA, HEARTBEAT, ACKNACK and GAP between user endpoints, and
the text of each SEDP announcement.
"""
import re
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
        elif link == 276:  # Linux cooked v2
            p = p[20:]
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


def sn(b, e):
    high, low = struct.unpack(e + "iI", b[:8])
    return (high << 32) + low


def show_trace(dom, sport, dport, vendor, prefix, sid, flags, body):
    e = "<" if flags & 1 else ">"
    if sid in (0x15, 0x16):
        reader, writer, rest = body[4:8], body[8:12], body[12:]
    else:
        reader, writer, rest = body[0:4], body[4:8], body[8:]
    if sid == 0x15 and writer.hex() in ("000003c2", "000004c2"):
        # An endpoint's announcement: its text (topic, type, USER_DATA).
        text = re.findall(rb"[\x20-\x7e]{4,}", bytes(rest))
        print(f"domain {dom} {sport}->{dport} vendor {vendor} prefix {prefix} SEDP"
              f" {writer.hex()} " + " | ".join(t.decode() for t in text))
        return
    # TypeLookup (DDS-XTypes 1.3, 7.6.3.3.4): its requests and replies.
    typelookup = writer.hex() in ("000300c3", "000301c3") or reader.hex() in (
        "000300c4", "000301c4")
    if (writer[3] & 0xC0 == 0xC0 and (reader[3] & 0xC0 == 0xC0 or reader == bytes(4))
            and not typelookup):
        return
    what = {0x06: "ACKNACK", 0x07: "HEARTBEAT", 0x08: "GAP", 0x12: "NACK_FRAG",
            0x13: "HEARTBEAT_FRAG", 0x15: "DATA", 0x16: "DATA_FRAG"}[sid]
    if sid == 0x15:
        shown = 160 if typelookup else 64
        detail = f"sn {sn(rest, e)} len {len(body)} body {bytes(body[:shown]).hex()}"
    elif sid == 0x06:
        bits = struct.unpack(e + "I", rest[8:12])[0]
        words = rest[12:12 + 4 * ((bits + 31) // 32)]
        detail = f"base {sn(rest, e)} bits {bits} {words.hex()}"
    elif sid == 0x16:
        start, count, size, total = struct.unpack(e + "IHHI", rest[8:20])
        detail = (f"sn {sn(rest, e)} fragments {start}+{count} of {size}"
                  f" sample {total} len {len(body)}")
    elif sid == 0x12:
        base, bits = struct.unpack(e + "II", rest[8:16])
        words = rest[16:16 + 4 * ((bits + 31) // 32)]
        detail = f"sn {sn(rest, e)} base {base} bits {bits} {words.hex()}"
    elif sid == 0x13:
        detail = f"sn {sn(rest, e)} last fragment {struct.unpack(e + 'I', rest[8:12])[0]}"
    elif sid == 0x07:
        detail = f"first {sn(rest, e)} last {sn(rest[8:], e)}"
    else:
        detail = f"start {sn(rest, e)} base {sn(rest[8:], e)}"
    print(f"domain {dom} {sport}->{dport} vendor {vendor} prefix {prefix} {what}"
          f" flags {flags:#04x} writer {writer.hex()} reader {reader.hex()} {detail}")


def main():
    args = sys.argv[1:]
    limit = 20
    builtin = "--all" in args
    trace = "--trace" in args
    args = [a for a in args if a not in ("--all", "--trace")]
    if "--max" in args:
        i = args.index("--max")
        limit = int(args[i + 1])
        del args[i:i + 2]
    path, domains = args[0], {int(a) for a in args[1:]}
    shown = {}
    for sport, dport, u in packets(path):
        dom = domain_of(dport)
        if dom not in domains:
            # Replies to ports outside the domain's range (OpenDDS
            # unicast) come from a port of the domain.
            dom = domain_of(sport)
        if dom not in domains or u[:4] != b"RTPS":
            continue
        vendor, prefix = u[6:8].hex(), u[8:20].hex()
        at = 20
        while at + 4 <= len(u):
            sid, flags = u[at], u[at + 1]
            le = flags & 1
            n = struct.unpack("<H" if le else ">H", u[at + 2:at + 4])[0]
            body = u[at + 4:at + 4 + n] if n else u[at + 4:]
            if trace and sid in (0x06, 0x07, 0x08, 0x12, 0x13, 0x15, 0x16) and len(body) >= 8:
                show_trace(dom, sport, dport, vendor, prefix, sid, flags, body)
            elif sid == 0x15 and len(body) >= 12:
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
