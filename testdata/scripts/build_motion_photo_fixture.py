#!/usr/bin/env python3
"""Build a synthetic Pixel-style Motion Photo JPEG fixture for nom-exif tests.

Combines existing testdata files into a single JPEG that real Motion Photo
implementations should recognize:

  Inputs:
    testdata/exif-no-tz.jpg      base JPEG with standard EXIF (~17 KB)
    testdata/sony-a7-xavc.MP4    minimal valid ISO BMFF MP4 (~2.7 KB,
                                  just `ftyp` + `moov`, no `mdat`)

  Output:
    testdata/motion_photo_pixel_synth.jpg  (~20 KB)

The output is structured exactly like a Pixel Motion Photo:

   [SOI]
   [APP1 XMP] -- contains GCamera:MotionPhoto="1"
                 and GCamera:MotionPhotoOffset="<trailer length>"
   [APP1 EXIF] -- inherited from base JPEG
   [DQT/SOF0/...]
   [SOS]
   ...compressed image data...
   [EOI]
   <appended MP4 trailer>           <-- this is the embedded video

`GCamera:MotionPhotoOffset` is the byte length of the trailer; the parser
locates the MP4 by seeking to (file_size - offset).

Both inputs are existing testdata files in this repository; the resulting
fixture is therefore covered by this repository's licensing and contains
no third-party content.

Run this script to regenerate the fixture if either input changes:
    python3 testdata/scripts/build_motion_photo_fixture.py
"""
from __future__ import annotations

import pathlib
import struct
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
BASE_JPG = ROOT / "exif-no-tz.jpg"
TRAILER_MP4 = ROOT / "sony-a7-xavc.MP4"
OUT = ROOT / "motion_photo_pixel_synth.jpg"

XMP_NS_HEADER = b"http://ns.adobe.com/xap/1.0/\x00"


def build_xmp_packet(motion_photo_offset: int) -> bytes:
    return (
        b'<?xpacket begin="\xef\xbb\xbf" id="W5M0MpCehiHzreSzNTczkc9d"?>'
        b'<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="nom-exif synthetic fixture">'
        b'<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">'
        b'<rdf:Description rdf:about="" '
        b'xmlns:GCamera="http://ns.google.com/photos/1.0/camera/" '
        b'GCamera:MotionPhoto="1" '
        b'GCamera:MotionPhotoVersion="1" '
        b'GCamera:MotionPhotoPresentationTimestampUs="0" '
        b'GCamera:MotionPhotoOffset="' + str(motion_photo_offset).encode() + b'"/>'
        b"</rdf:RDF></x:xmpmeta>"
        b'<?xpacket end="w"?>'
    )


def insert_xmp_app1(jpeg: bytes, xmp_payload: bytes) -> bytes:
    """Insert an XMP APP1 segment after the existing EXIF APP1 segment.

    Real-world Pixel cameras place XMP after EXIF; matching that order also
    keeps the file's first bytes looking like a textbook JPEG (FF D8 FF E1
    Exif…), which avoids confusing the MIME sniffer.
    """
    if jpeg[:2] != b"\xff\xd8":
        raise SystemExit(f"{BASE_JPG.name}: missing SOI marker")
    if jpeg[2:4] != b"\xff\xe1":
        raise SystemExit(
            f"{BASE_JPG.name}: expected APP1 marker right after SOI; "
            f"this script assumes the base JPEG begins with SOI + APP1 EXIF."
        )

    # Length of the existing APP1 EXIF segment (includes its own 2 length bytes).
    exif_seg_len = struct.unpack(">H", jpeg[4:6])[0]
    insert_at = 4 + exif_seg_len  # marker(2) + length(2) + payload(exif_seg_len-2)

    segment_data = XMP_NS_HEADER + xmp_payload
    seg_len = len(segment_data) + 2
    if seg_len > 0xFFFF:
        raise SystemExit(
            f"XMP segment too large for a single APP1 (got {seg_len} bytes); "
            "ExtendedXMP would be required for a real-world fixture this big."
        )
    app1 = b"\xff\xe1" + struct.pack(">H", seg_len) + segment_data
    return jpeg[:insert_at] + app1 + jpeg[insert_at:]


def main() -> None:
    base = BASE_JPG.read_bytes()
    trailer = TRAILER_MP4.read_bytes()

    # Sanity-check the trailer is a parseable ISO BMFF: starts with a
    # `ftyp` box (4-byte size + 'ftyp').
    if len(trailer) < 8 or trailer[4:8] != b"ftyp":
        raise SystemExit(f"{TRAILER_MP4.name}: not a valid ISO BMFF (missing ftyp)")

    xmp = build_xmp_packet(motion_photo_offset=len(trailer))
    jpeg_with_xmp = insert_xmp_app1(base, xmp)
    out = jpeg_with_xmp + trailer

    OUT.write_bytes(out)
    rel = OUT.relative_to(ROOT.parent)
    print(
        f"wrote {rel}: {len(out)} bytes "
        f"(jpeg+xmp={len(jpeg_with_xmp)}, trailer={len(trailer)}, "
        f"MotionPhotoOffset={len(trailer)})"
    )


if __name__ == "__main__":
    sys.exit(main())
