"""
Build src-tauri/icons/icon.ico in the classic layout every part of Windows reads:
32-bit bitmap entries for 16-64 px and a PNG entry only at 256 px.
(`tauri icon` stores every size as PNG, which Explorer fails to show on some shortcuts.)

  npx tauri icon src-tauri/icons/icon.svg -o src-tauri/icons
  py -I tools/make_ico.py
"""

import io
import struct
from pathlib import Path

from PIL import Image

ICONS = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
SIZES = [16, 24, 32, 48, 64, 256]


def bmp_entry(img: Image.Image) -> bytes:
    w, h = img.size
    px = img.convert("RGBA").tobytes()
    # BITMAPINFOHEADER; height is doubled for the XOR + AND masks.
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    rows = []
    for y in range(h - 1, -1, -1):  # bottom-up
        row = px[y * w * 4 : (y + 1) * w * 4]
        rows.append(b"".join(bytes((row[i + 2], row[i + 1], row[i], row[i + 3])) for i in range(0, len(row), 4)))
    mask_row = ((w + 31) // 32) * 4
    and_mask = b"\x00" * (mask_row * h)  # alpha channel carries transparency
    return header + b"".join(rows) + and_mask


def main():
    src = Image.open(ICONS / "icon.png").convert("RGBA")
    entries = []
    for s in SIZES:
        img = src.resize((s, s), Image.LANCZOS)
        if s >= 256:
            buf = io.BytesIO()
            img.save(buf, "PNG")
            data = buf.getvalue()
        else:
            data = bmp_entry(img)
        entries.append((s, data))
    out = io.BytesIO()
    out.write(struct.pack("<HHH", 0, 1, len(entries)))
    offset = 6 + 16 * len(entries)
    for s, data in entries:
        dim = 0 if s >= 256 else s
        out.write(struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset))
        offset += len(data)
    for _, data in entries:
        out.write(data)
    (ICONS / "icon.ico").write_bytes(out.getvalue())
    print(f"wrote {ICONS / 'icon.ico'} ({len(out.getvalue())} bytes, sizes {SIZES})")


if __name__ == "__main__":
    main()
