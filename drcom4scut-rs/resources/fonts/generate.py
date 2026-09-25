"""Reproduce bundled static fonts: python generate.py /path/to/NotoSansSC-VF.ttf.

Requires fonttools==4.66.0. The source must match the pinned SHA-256 below.
Normal Rust builds use the checked-in compressed fonts and do not run this script.
"""
import hashlib
import io
import json
from pathlib import Path
import sys
import zlib

import fontTools
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

REVISION = "f8d157532fbfaeda587e826d4cd5b21a49186f7c"
SOURCE = f"https://raw.githubusercontent.com/notofonts/noto-cjk/{REVISION}/Sans/Variable/TTF/Subset/NotoSansSC-VF.ttf"
SOURCE_SHA256 = "d68bafcb48a2707749396aa12bbbd833cb70401f3a9a689fd2902c7e0d295964"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    source = Path(sys.argv[1])
    if digest(source.read_bytes()) != SOURCE_SHA256:
        raise ValueError("Source font does not match the pinned upstream binary")
    if fontTools.__version__ != "4.66.0":
        raise ValueError("Use fonttools==4.66.0 for reproducible instances")
    root = Path(__file__).resolve().parent
    files = []
    for weight, label in [(400, "Regular"), (500, "Medium")]:
        font = TTFont(source, recalcTimestamp=False)
        font = instantiateVariableFont(
            font, {"wght": weight}, inplace=True, optimize=True, updateFontNames=True
        )
        font.recalcTimestamp = False
        output = io.BytesIO()
        font.save(output)
        data = output.getvalue()
        stored = zlib.compress(data, 9)
        name = f"NotoSansSC-{label}.ttf.zlib"
        (root / name).write_bytes(stored)
        files.append({
            "file": name, "weight": weight,
            "uncompressed_bytes": len(data), "uncompressed_sha256": digest(data),
            "stored_bytes": len(stored), "stored_sha256": digest(stored),
        })
        print(name, len(stored), flush=True)
    manifest = {
        "source": SOURCE, "source_sha256": SOURCE_SHA256,
        "revision": REVISION, "fonttools": fontTools.__version__,
        "zlib": zlib.ZLIB_VERSION,
        "modification": "Static weight instances (400/500); names updated by fontTools; original glyph coverage retained; no UI-string subsetting.",
        "license_source": f"https://raw.githubusercontent.com/notofonts/noto-cjk/{REVISION}/Sans/LICENSE",
        "license_sha256": digest((root / "OFL.txt").read_bytes()),
        "files": files,
    }
    (root / "sources.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8", newline="\n"
    )


if __name__ == "__main__":
    main()
