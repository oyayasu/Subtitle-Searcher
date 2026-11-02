"""Generate a branded Windows .ico / PNG using Pillow (no binary assets to maintain)."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
ICONS = ROOT / "src-tauri" / "icons"
ICONS.mkdir(parents=True, exist_ok=True)
SIZE = 1024

base = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
pixels = base.load()
for y in range(SIZE):
    for x in range(SIZE):
        t = min(1.0, (x / SIZE) * 0.36 + (y / SIZE) * 0.64)
        pixels[x, y] = (
            int(39 * (1 - t) + 16 * t),
            int(92 * (1 - t) + 39 * t),
            int(100 * (1 - t) + 49 * t),
            255,
        )

mask = Image.new("L", (SIZE, SIZE))
m = ImageDraw.Draw(mask)
m.rounded_rectangle((0, 0, 1023, 1023), radius=232, fill=255)
base.putalpha(mask)

# Subtle depth without visual noise.
glow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
g = ImageDraw.Draw(glow)
g.ellipse((490, -300, 1250, 380), fill=(113, 215, 181, 38))
glow.putalpha(glow.getchannel("A").filter(ImageFilter.GaussianBlur(110)))
base = Image.alpha_composite(base, glow)

shadow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
sd = ImageDraw.Draw(shadow)
sd.rounded_rectangle((186, 238, 842, 724), radius=104, fill=(0, 12, 18, 135))
shadow = shadow.filter(ImageFilter.GaussianBlur(28))
base = Image.alpha_composite(base, shadow)

d = ImageDraw.Draw(base)
# Transcript speech bubble.
d.rounded_rectangle((186, 230, 838, 706), radius=104, fill=(238, 252, 245, 255))
d.polygon([(314, 700), (314, 787), (433, 697)], fill=(225, 248, 237, 255))
d.rounded_rectangle((294, 340, 718, 382), radius=21, fill=(115, 165, 156, 195))
d.rounded_rectangle((294, 438, 586, 478), radius=20, fill=(150, 191, 182, 145))

# Search lens and handle.
d.ellipse((476, 473, 826, 823), fill=(25, 68, 77, 255), outline=(112, 221, 191, 255), width=48)
d.ellipse((527, 524, 774, 772), fill=(31, 84, 95, 255))
d.line([(752, 766), (886, 899)], fill=(106, 217, 188, 255), width=62, joint="curve")
d.ellipse((846, 859, 910, 923), fill=(106, 217, 188, 255))
d.arc((569, 562, 704, 698), 206, 267, fill=(180, 255, 231, 185), width=20)

# Mask the decorative layers to the rounded app-square.
final_alpha = Image.new("L", (SIZE, SIZE))
ImageDraw.Draw(final_alpha).rounded_rectangle((0, 0, 1023, 1023), radius=232, fill=255)
base.putalpha(Image.composite(base.getchannel("A"), Image.new("L", base.size), final_alpha))

png = ICONS / "icon.png"
ico = ICONS / "icon.ico"
base.resize((512, 512), Image.Resampling.LANCZOS).save(png, format="PNG")
base.save(ico, format="ICO", sizes=[(16,16), (24,24), (32,32), (48,48), (64,64), (128,128), (256,256)])
print(f"Generated {png} and {ico}")
