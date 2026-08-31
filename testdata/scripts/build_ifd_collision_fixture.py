#!/usr/bin/env python3
"""Build a synthetic TIFF exercising tag-code collisions across IFD namespaces.

Each Exif IFD has its own 16-bit tag namespace, so the same code means
different things depending on which IFD it appears in. This fixture puts
the two most relevant collisions into one file:

  code 0x000b  IFD0     ProcessingSoftware  (ASCII)
               GPS IFD  GPSDOP              (RATIONAL)

  code 0x0001  GPS IFD  GPSLatitudeRef      (ASCII "N")
               Interop  InteropIndex        (ASCII "R98")

Output:
  testdata/ifd-namespace-collision.tif

Layout (little-endian, classic TIFF):

  0x0000  TIFF header (II, 42, ifd0_offset)
  0x0008  IFD0     -- ProcessingSoftware, Make, ExifOffset, GPSInfo
          Exif IFD -- ExifVersion, InteropOffset
          GPS IFD  -- GPSVersionID, GPSDOP, GPSLatitudeRef
          Interop  -- InteropIndex, InteropVersion
          <pooled out-of-line values>
  <zero padding to 8 KiB>

The trailing padding is not structural: the parser's reader requires a
minimum buffer to classify the source, and a bare ~200-byte TIFF trips
an UnexpectedEof before any IFD is reached.
"""

import struct
from pathlib import Path

ASCII, SHORT, LONG, RATIONAL, UNDEFINED = 2, 3, 4, 5, 7
BYTE = 1

HEADER_LEN = 8
ENTRY_LEN = 12
PADDED_LEN = 8192


def ifd_len(n_entries):
    """Bytes occupied by an IFD with `n_entries` (count + entries + next-link)."""
    return 2 + n_entries * ENTRY_LEN + 4


class ValuePool:
    """Collects values too large for an entry's inline 4-byte slot."""

    def __init__(self, base):
        self.base = base
        self.buf = bytearray()

    def add(self, raw):
        offset = self.base + len(self.buf)
        self.buf.extend(raw)
        return offset


def entry(tag, fmt, count, payload):
    """One 12-byte IFD entry. `payload` is the raw 4-byte value/offset slot."""
    assert len(payload) == 4
    return struct.pack("<HHI", tag, fmt, count) + payload


def inline(raw):
    """Pad a <=4-byte value into an entry's inline slot."""
    assert len(raw) <= 4
    return raw.ljust(4, b"\x00")


def build_ifd(entries, next_ifd=0):
    out = struct.pack("<H", len(entries))
    out += b"".join(entries)
    out += struct.pack("<I", next_ifd)
    return out


def build():
    ifd0_off = HEADER_LEN
    exif_off = ifd0_off + ifd_len(4)
    gps_off = exif_off + ifd_len(2)
    interop_off = gps_off + ifd_len(3)
    pool = ValuePool(interop_off + ifd_len(2))

    software = pool.add(b"MyProcessingSoftware\x00")
    make = pool.add(b"ACME\x00")
    dop = pool.add(struct.pack("<II", 5, 2))  # 5/2 = 2.5

    ifd0 = build_ifd(
        [
            # 0x000b in IFD0 is ProcessingSoftware, NOT GPSDOP.
            entry(0x000B, ASCII, 21, struct.pack("<I", software)),
            entry(0x010F, ASCII, 5, struct.pack("<I", make)),
            entry(0x8769, LONG, 1, struct.pack("<I", exif_off)),
            entry(0x8825, LONG, 1, struct.pack("<I", gps_off)),
        ]
    )

    exif = build_ifd(
        [
            entry(0x9000, UNDEFINED, 4, inline(b"0230")),
            entry(0xA005, LONG, 1, struct.pack("<I", interop_off)),
        ]
    )

    gps = build_ifd(
        [
            entry(0x0000, BYTE, 4, inline(bytes([2, 3, 0, 0]))),
            # 0x000b in the GPS IFD is GPSDOP, NOT ProcessingSoftware.
            entry(0x000B, RATIONAL, 1, struct.pack("<I", dop)),
            entry(0x0001, ASCII, 2, inline(b"N\x00")),
        ]
    )

    interop = build_ifd(
        [
            # 0x0001 here is InteropIndex, NOT GPSLatitudeRef.
            entry(0x0001, ASCII, 4, inline(b"R98\x00")),
            entry(0x0002, UNDEFINED, 4, inline(bytes([0, 1, 0, 0]))),
        ]
    )

    body = (
        struct.pack("<2sHI", b"II", 42, ifd0_off)
        + ifd0
        + exif
        + gps
        + interop
        + bytes(pool.buf)
    )
    assert len(body) <= PADDED_LEN, len(body)
    return body.ljust(PADDED_LEN, b"\x00")


def main():
    out = Path(__file__).resolve().parents[1] / "ifd-namespace-collision.tif"
    out.write_bytes(build())
    print(f"wrote {out} ({out.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
