# Bakes the Hornet's paint into assets/fa18_paint.jpg (plain Python, with Pillow and NumPy):
#
#   python assets/blender/fa18_paint.py [<dir of pictures>] [<out.jpg>]
#
# The paint is three pictures of the aircraft: from straight above, from
# straight below, and from its left side (assets/blender/paint/top.png,
# bottom.png, side.png). The game projects them back onto the skin
# (plane.wgsl, `hornet_paint`), so they must line up with the model: paint
# them over the guides that
#
#   blender -b --factory-startup --python assets/blender/fa18.py -- <scratch dir> --guides assets/blender/paint
#
# renders (<view>_guide.png, with the silhouette in <view>_mask.png). The ones
# here were painted over those guides by an image model (GPT Image, through
# the Codex CLI) and can be touched up or replaced in any paint program.
#
# This script cuts each picture to the model's silhouette, spreads the paint
# at its edge outward over the background (so the projection never picks up
# white at an edge) and lays the three side by side, the side view turned nose
# up like the others: top | bottom | side. The enemy's fictional aggressor
# livery (AGGRESSOR_* below) is derived from the same pictures and laid beside
# them in the same order, for plane.wgsl's bandit branch.
import os
import sys

import numpy as np
from PIL import Image, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "paint")
OUT = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(HERE), "fa18_paint.jpg")

# ---------------------------------------------------------------- the enemy livery
# A fictional aggressor scheme, not a real unit's: the player's tactical greys
# turned to a dark blue-grey, with red wingtips and a red band behind the
# radome. (The game selects it for bandits; docs/fablefight-limitations.md
# records it as fictional.)
AGGRESSOR_DARK = (34, 39, 47)
AGGRESSOR_LIGHT = (126, 136, 148)
AGGRESSOR_RED = (142, 26, 26)
AGGRESSOR_TIP = 5.35   # span station (m) beyond which a wingtip is red
AGGRESSOR_NOSE = (7.30, 8.10)  # station band (m) behind the radome
GUIDE_SPAN, GUIDE_LEN, GUIDE_Y = 13.2, 19.8, 0.4  # as fa18.py


def spread(rgb, ok, rounds=128):
    """Grow the painted pixels outward, a pixel a round, over the ones that are not."""
    rgb = rgb.astype(np.float32) * ok[..., None]
    ok = ok.astype(np.float32)
    for _ in range(rounds):
        s = np.zeros_like(rgb)
        n = np.zeros_like(ok)
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                if dy or dx:
                    s += np.roll(rgb, (dy, dx), (0, 1))
                    n += np.roll(ok, (dy, dx), (0, 1))
        grow = (ok == 0) & (n > 0)
        if not grow.any():
            break
        rgb[grow] = s[grow] / n[grow][:, None]
        ok[grow] = 1
    rgb[ok == 0] = rgb[ok > 0].mean(0)
    return rgb


def enemy(view):
    """The fictional aggressor livery, painted over the player's pictures:
    dark blue-grey, red wingtips and a red band behind the radome."""
    mask = Image.open(os.path.join(SRC, view + "_mask.png")).convert("L")
    pic = Image.open(os.path.join(SRC, view + ".png")).convert("RGB")
    if pic.size != mask.size:
        pic = pic.resize(mask.size, Image.LANCZOS)
    rgb = np.asarray(pic, np.float32)
    # The player's paint's brightness drives the new scheme, so the panel
    # lines, weathering and markings stay where they were.
    lum = np.clip((rgb @ np.array([0.30, 0.59, 0.11], np.float32)) / 255.0, 0.0, 1.0)
    dark, light = (np.asarray(c, np.float32) for c in (AGGRESSOR_DARK, AGGRESSOR_LIGHT))
    rgb = dark + (light - dark) * (lum ** 0.85)[..., None]
    # Red bands: the outer part of each wingtip (seen from above or below),
    # and a band across the fuselage behind the radome (in every view).
    h, w = rgb.shape[:2]
    ppm = w / (GUIDE_SPAN if view in ("top", "bottom") else GUIDE_LEN)
    cols = (np.arange(w, dtype=np.float32) + 0.5 - w / 2) / ppm
    rows = (np.arange(h, dtype=np.float32) + 0.5 - h / 2) / ppm
    if view in ("top", "bottom"):
        station = GUIDE_Y - rows[:, None]
        red = (np.abs(cols)[None, :] > AGGRESSOR_TIP) | ((station > AGGRESSOR_NOSE[0]) & (station < AGGRESSOR_NOSE[1]))
    else:  # nose left in the painted picture
        station = GUIDE_Y - cols[None, :]
        red = np.broadcast_to((station > AGGRESSOR_NOSE[0]) & (station < AGGRESSOR_NOSE[1]), (h, w))
    rgb = np.where(red[..., None], np.asarray(AGGRESSOR_RED, np.float32), rgb)
    inside = np.asarray(mask.filter(ImageFilter.MinFilter(7))) > 128
    rgb = np.where(inside[..., None], rgb, 255.0)
    return Image.fromarray(np.clip(rgb, 0, 255).astype(np.uint8))


def layer(view):
    mask = Image.open(os.path.join(SRC, view + "_mask.png")).convert("L")
    paint = Image.open(os.path.join(SRC, view + ".png")).convert("RGB")
    if paint.size != mask.size:
        paint = paint.resize(mask.size, Image.LANCZOS)
    rgb = np.asarray(paint)
    # Painted: inside the silhouette (less a few pixels, where the picture's
    # own edge may fall short of the model's) and not the white of the background.
    inside = np.asarray(mask.filter(ImageFilter.MinFilter(7))) > 128
    ok = inside & (rgb.min(axis=2) < 236)
    out = Image.fromarray(np.clip(spread(rgb, ok), 0, 255).astype(np.uint8))
    print(f"{view}: {paint.size[0]}x{paint.size[1]}, {ok.mean() * 100:.0f}% painted")
    return out


def bake(view, pic):
    """One of the enemy pictures, cut and spread like the player's."""
    mask = Image.open(os.path.join(SRC, view + "_mask.png")).convert("L")
    rgb = np.asarray(pic)
    inside = np.asarray(mask.filter(ImageFilter.MinFilter(7))) > 128
    return Image.fromarray(np.clip(spread(rgb, inside), 0, 255).astype(np.uint8))


enemy_pics = {view: enemy(view) for view in ("top", "bottom", "side")}
top, bottom, side = layer("top"), layer("bottom"), layer("side")
etop, ebottom, eside = (bake(view, enemy_pics[view]) for view in ("top", "bottom", "side"))
side = side.transpose(Image.Transpose.ROTATE_270)  # nose up, like the other two
eside = eside.transpose(Image.Transpose.ROTATE_270)
assert top.size == bottom.size == side.size == etop.size == ebottom.size == eside.size, (top.size, bottom.size, side.size)
w, h = top.size
atlas = Image.new("RGB", (w * 6, h))
for k, im in enumerate((top, bottom, side, etop, ebottom, eside)):
    atlas.paste(im, (w * k, 0))
atlas.save(OUT, quality=92, subsampling=0)
print("wrote", OUT, atlas.size, os.path.getsize(OUT), "bytes")
