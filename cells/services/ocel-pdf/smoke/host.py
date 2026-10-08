# SPDX-License-Identifier: AGPL-3.0-or-later
"""Host-only engine smoke, not a service target or fallback implementation.
Run: python3 cells/services/ocel-pdf/smoke/host.py tests/fixtures/ocel-pdf-demo.pdf
"""
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

package = Path(__file__).resolve().parent.parent
vendor = package / "vendor/mupdf"
fixture = Path(sys.argv[1]).resolve()
omit = {"harfbuzz.c", "memento.c", "time.c", "directory.c", "document-all.c",
        "output-docx.c", "output-pdfocr.c", "ocr-device.c", "leptonica-wrap.c"}
sources = sorted(p for directory in ("source/fitz", "source/pdf")
                 for p in (vendor / directory).glob("*.c") if p.name not in omit)
keys = "FREETYPE|LIBJPEG|ZLIB|JBIG2DEC|OPENJPEG|LCMS2"
sources += [vendor / path for path in re.findall(
    rf"^(?:{keys})_SRC \+= (\S+)", (vendor / "Makelists").read_text(), re.M)]
includes = ["include", "thirdparty/freetype/include", "scripts/freetype",
            "thirdparty/libjpeg", "scripts/libjpeg", "thirdparty/zlib",
            "thirdparty/jbig2dec", "thirdparty/openjpeg/src/lib/openjp2",
            "thirdparty/lcms2/include"]
# Deliberately use the host's complete libc/setjmp. The service's Cargo build
# rejects host targets and uses owned LP64 declarations plus the RV64 assembly.
with tempfile.TemporaryDirectory(prefix="ocel-pdf-host-") as directory:
    out = Path(directory)
    fonts = sorted((vendor / "resources/fonts/urw").glob("*.cff"))
    for index, font in enumerate(fonts):
        data = font.read_bytes()
        symbol = font.name.replace(".", "_").replace("-", "_")
        generated = out / f"font-{index}.c"
        generated.write_text(f"const unsigned int _binary_{symbol}_size={len(data)};\n"
            f"const unsigned char _binary_{symbol}[]={{" + ",".join(map(str, data)) + "};\n")
        sources.append(generated)
    sources += [package / "src/c/renderer.c", package / "smoke/host.c"]
    compiler = os.environ.get("HOST_CC", "cc")
    command = [compiler, "-std=gnu11", "-Os", "-ffunction-sections", "-fdata-sections",
               "-Wl,--gc-sections", "-include", str(package / "vendor/cellos-config.h")]
    command += [argument for include in includes for argument in ("-I", str(vendor / include))]
    command += list(map(str, sources)) + ["-lm", "-o", str(out / "smoke")]
    subprocess.run(command, check=True)
    subprocess.run([str(out / "smoke"), str(fixture)], check=True, cwd=out)
