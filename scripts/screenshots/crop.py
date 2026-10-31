#!/usr/bin/env python3
"""Cut the session's screenshots to what each one is about, and make the
GIF from its frames: crop.py SHOTS_DIR OUT_DIR. Needs python3-pil.

The regions are for the 1280x800 screen shoot.sh uses, where windows open
centred and the top bar is 32 pixels high.
"""

import glob
import os
import sys

from PIL import Image, ImageDraw, ImageFont

TERMINAL = (180, 118, 1100, 716)
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"


def caption(frame):
    """What's happening in a GIF frame, by its name: gif-b-003 is the
    fourth j typed, gif-d the tip."""
    part, n = os.path.basename(frame)[4], int(frame[-7:-4])
    if part == "b":
        return "You press: " + " ".join("j" * (n + 1))
    if part == "c":
        return "You press: " + " ".join("j" * 7)
    if part == "d":
        return "Flow Buddy: 7 keys, where } takes 1"
    return "Vim, with the Flow Buddy plugin"


def with_caption(im, text):
    font = ImageFont.truetype(FONT, 22) if os.path.exists(FONT) else ImageFont.load_default()
    out = Image.new("RGB", (im.width, im.height + 44), "#151515")
    out.paste(im, (0, 0))
    ImageDraw.Draw(out).text((16, im.height + 9), text, fill="#f6d32d", font=font)
    return out


CROPS = {
    "card": (420, 0, 820, 128),
    "menu": (900, 0, 1280, 646),
    "vim-tip": TERMINAL,
    "practice": TERMINAL,
    "welcome": (322, 88, 958, 744),
}


def main(shots, out):
    os.makedirs(out, exist_ok=True)
    for name, box in CROPS.items():
        path = os.path.join(shots, f"{name}.png")
        if not os.path.exists(path):
            print(f"missing: {name}", file=sys.stderr)
            continue
        Image.open(path).crop(box).save(os.path.join(out, f"{name}.png"), optimize=True)
    frames = sorted(glob.glob(os.path.join(shots, "gif-*.png")))
    if frames:
        # Two thirds the size keeps the GIF small; the text stays readable.
        rgb = []
        for f in frames:
            im = with_caption(Image.open(f).convert("RGB").crop(TERMINAL), caption(f))
            rgb.append(im.resize((im.width * 2 // 3, im.height * 2 // 3), Image.LANCZOS))
        # One palette for every frame, from the last (it has the most colour),
        # so colours don't shift from frame to frame.
        palette = rgb[-1].quantize(colors=64)
        images = [im.quantize(palette=palette, dither=Image.Dither.NONE) for im in rgb]
        durations = [250] * len(images)
        durations[-1] = 3500
        images[0].save(
            os.path.join(out, "tip.gif"),
            save_all=True,
            append_images=images[1:],
            duration=durations,
            loop=0,
            optimize=True,
        )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
