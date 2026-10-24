"""Recompress preview PNGs written by the Rust example (stored deflate) in place."""
import glob, os
from PIL import Image
here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "previews")
for p in glob.glob(os.path.join(here, "*.png")):
    im = Image.open(p)
    im.load()
    if im.mode == "RGBA" and im.getextrema()[3][0] == 255:
        im = im.convert("RGB")
    im.save(p, optimize=True)
    print(os.path.basename(p), os.path.getsize(p))
