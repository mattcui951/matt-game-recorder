"""
Minimal read-only LevelDB table reader (no dependencies), used to inspect Outplayed's
IndexedDB. Prints every key/value pair from the .ldb tables and .log of a LevelDB folder.

  py -I tools/leveldb_dump.py <leveldb dir> [needle]
"""

import struct
import sys
from pathlib import Path


def varint(b, i):
    r = s = 0
    while True:
        c = b[i]
        i += 1
        r |= (c & 0x7F) << s
        if c < 0x80:
            return r, i
        s += 7


def snappy(b):
    n, i = varint(b, 0)
    out = bytearray()
    while i < len(b):
        tag = b[i]
        i += 1
        t = tag & 3
        if t == 0:
            ln = tag >> 2
            if ln >= 60:
                nb = ln - 59
                ln = int.from_bytes(b[i : i + nb], "little")
                i += nb
            ln += 1
            out += b[i : i + ln]
            i += ln
            continue
        if t == 1:
            ln = ((tag >> 2) & 7) + 4
            off = ((tag >> 5) << 8) | b[i]
            i += 1
        elif t == 2:
            ln = (tag >> 2) + 1
            off = int.from_bytes(b[i : i + 2], "little")
            i += 2
        else:
            ln = (tag >> 2) + 1
            off = int.from_bytes(b[i : i + 4], "little")
            i += 4
        for _ in range(ln):
            out.append(out[-off])
    assert len(out) == n
    return bytes(out)


def block(data, off, size):
    raw = data[off : off + size]
    kind = data[off + size]
    return snappy(raw) if kind == 1 else raw


def entries(blk):
    nrest = struct.unpack("<I", blk[-4:])[0]
    end = len(blk) - 4 - 4 * nrest
    i = 0
    key = b""
    while i < end:
        shared, i = varint(blk, i)
        nonshared, i = varint(blk, i)
        vlen, i = varint(blk, i)
        key = key[:shared] + blk[i : i + nonshared]
        i += nonshared
        yield key, blk[i : i + vlen]
        i += vlen


def table(path):
    data = path.read_bytes()
    footer = data[-48:]
    _, j = varint(footer, 0)
    _, j = varint(footer, j)
    ioff, j = varint(footer, j)
    isize, j = varint(footer, j)
    for _, handle in entries(block(data, ioff, isize)):
        boff, k = varint(handle, 0)
        bsize, k = varint(handle, k)
        yield from entries(block(data, boff, bsize))


def log(path):
    data = path.read_bytes()
    i = 0
    buf = b""
    while i + 7 <= len(data):
        ln = struct.unpack("<H", data[i + 4 : i + 6])[0]
        typ = data[i + 6]
        if ln == 0 and typ == 0:
            i = (i // 32768 + 1) * 32768
            continue
        buf += data[i + 7 : i + 7 + ln]
        i += 7 + ln
        if typ in (1, 4):  # FULL or LAST record: a write batch
            rec, buf = buf, b""
            count = struct.unpack("<I", rec[8:12])[0]
            p = 12
            for _ in range(count):
                kind = rec[p]
                p += 1
                kl, p = varint(rec, p)
                k = rec[p : p + kl]
                p += kl
                if kind == 1:
                    vl, p = varint(rec, p)
                    yield k, rec[p : p + vl]
                    p += vl


def dump(folder):
    folder = Path(folder)
    for f in sorted(folder.glob("*.ldb")):
        yield from table(f)
    for f in sorted(folder.glob("*.log")):
        yield from log(f)


if __name__ == "__main__":
    needle = sys.argv[2].encode() if len(sys.argv) > 2 else None
    for k, v in dump(sys.argv[1]):
        if needle is None or needle in v or needle in v.replace(b"\x00", b""):
            print(repr(k[:80]), len(v))
            print(repr(v[:1500]))
            print()
