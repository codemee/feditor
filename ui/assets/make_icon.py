# /// script
# requires-python = ">=3.10"
# dependencies = ["pillow"]
# ///
"""產生 feditor 的應用程式圖示：透明背景、帶藍色外框的文字文件、斜放的鉛筆。

用法（在專案根目錄）：
    uv run ui/assets/make_icon.py            # 產生 ui/assets/icon.png 與 icon.ico
    uv run ui/assets/make_icon.py 預覽.png    # 另外輸出各尺寸的預覽圖（淺色與深色底各一排）

背景是透明的，所以文件要有藍色外框與柔和的陰影，放在白色或深色的工作列上才都看得清楚。
每個尺寸都以 8 倍解析度繪製再縮小，邊緣才會平滑；
16～32 px 的小尺寸使用簡化版（較粗的線、較少的文字行），避免糊成一團。
"""

import io
import math
import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

SUPERSAMPLE = 8
SIZES = [16, 24, 32, 48, 64, 128, 256]
OUT_DIR = Path(__file__).parent

BLUE = (0x2A, 0x55, 0xC8, 255)  # 文件外框
BLUE_LIGHT = (0x9D, 0xB4, 0xE3, 255)  # 文字行
BLUE_FOLD = (0xCF, 0xE0, 0xFF, 255)  # 摺角
NAVY = (0x14, 0x2F, 0x86, 255)  # 鉛筆外框


def render(size: int) -> Image.Image:
    S = size * SUPERSAMPLE
    small = size <= 32

    def P(x: float, y: float) -> tuple[float, float]:
        return (x * S, y * S)

    def layer(draw_fn) -> Image.Image:
        im = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        draw_fn(ImageDraw.Draw(im))
        return im

    def shadow(poly: list[tuple[float, float]], dx: float, dy: float, alpha: int) -> Image.Image:
        """多邊形的柔和陰影：偏移後塗色，再做高斯模糊。"""
        pts = [P(x + dx, y + dy) for x, y in poly]
        im = layer(lambda d: d.polygon(pts, fill=(8, 30, 110, alpha)))
        # 半徑小一點、偏移大一點：陰影只露在右下，不會往左上擴散成第二道淡邊
        return im.filter(ImageFilter.GaussianBlur(radius=0.008 * S))

    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))  # 透明背景

    # ── 文件 ────────────────────────────────────────────────
    doc = [(0.10, 0.06), (0.56, 0.06), (0.70, 0.20), (0.70, 0.90), (0.10, 0.90)]
    img = Image.alpha_composite(img, shadow(doc, 0.016, 0.022, 100))

    # 外框：先畫整個文件的外框色，再在裡面填白色（用 polygon 的外框線加粗，接角用圓角）
    border = 0.055 if small else 0.038

    def draw_doc(d):
        d.polygon([P(*p) for p in doc], fill=(255, 255, 255, 255))
        d.line(
            [P(*p) for p in doc + [doc[0]]],
            fill=BLUE,
            width=max(1, int(border * S)),
            joint="curve",
        )

    img = Image.alpha_composite(img, layer(draw_doc))

    # 摺角
    def draw_fold(d):
        fold = [P(0.56, 0.06), P(0.56, 0.20), P(0.70, 0.20)]
        d.polygon(fold, fill=BLUE_FOLD)
        d.line(fold + [fold[0]], fill=BLUE, width=max(1, int(border * 0.8 * S)), joint="curve")

    img = Image.alpha_composite(img, layer(draw_fold))

    # 文字行
    if small:
        rows = [(0.34, 0.58), (0.52, 0.58)]
        width = 0.075
    else:
        rows = [(0.28, 0.58), (0.41, 0.58), (0.54, 0.50), (0.67, 0.40)]
        width = 0.042

    def text_lines(d):
        for y, x_end in rows:
            d.line([P(0.19, y), P(x_end, y)], fill=BLUE_LIGHT, width=max(1, int(width * S)))
            r = width * S / 2
            for x in (0.19, x_end):
                cx, cy = P(x, y)
                d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=BLUE_LIGHT)

    img = Image.alpha_composite(img, layer(text_lines))

    # ── 鉛筆（尖端在左下，往右上延伸）──────────────────────────
    T = (0.43, 0.94)
    E = (0.91, 0.40)
    L = math.dist(T, E)
    u = ((E[0] - T[0]) / L, (E[1] - T[1]) / L)
    n = (-u[1], u[0])
    w = 0.17 if small else 0.15  # 筆身寬度

    def at(a: float, lat: float) -> tuple[float, float]:
        """沿筆軸 a、垂直偏移 lat 的位置（正規化座標）。"""
        return P(T[0] + u[0] * a + n[0] * lat, T[1] + u[1] * a + n[1] * lat)

    tip_len = 0.12
    body_end = 0.47
    ferrule_end = 0.54
    o = 0.019  # 輪廓外擴量

    outline = [
        at(-0.03, 0),
        at(tip_len, w / 2 + o),
        at(L + 0.02, w / 2 + o),
        at(L + 0.02, -(w / 2 + o)),
        at(tip_len, -(w / 2 + o)),
    ]
    shadow_pts = [(x / S + 0.012, y / S + 0.02) for x, y in outline]
    img = Image.alpha_composite(img, shadow(shadow_pts, 0.0, 0.0, 130))
    img = Image.alpha_composite(
        img, layer(lambda d: d.polygon(outline, fill=NAVY))
    )

    def pencil(d):
        # 木頭削尖處
        d.polygon([at(0, 0), at(tip_len, w / 2), at(tip_len, -w / 2)], fill=(0xF6, 0xD7, 0xA7, 255))
        # 筆芯
        d.polygon([at(0, 0), at(0.045, w * 0.18), at(0.045, -w * 0.18)], fill=(0x33, 0x33, 0x4A, 255))
        # 筆身：黃色，下半邊較深，做出立體感
        d.polygon(
            [at(tip_len, w / 2), at(body_end, w / 2), at(body_end, -w / 2), at(tip_len, -w / 2)],
            fill=(0xFF, 0xC8, 0x3D, 255),
        )
        d.polygon(
            [at(tip_len, w / 2), at(body_end, w / 2), at(body_end, w * 0.08), at(tip_len, w * 0.08)],
            fill=(0xF0, 0xA4, 0x1C, 255),
        )
        # 金屬環
        d.polygon(
            [at(body_end, w / 2), at(ferrule_end, w / 2), at(ferrule_end, -w / 2), at(body_end, -w / 2)],
            fill=(0xC9, 0xCF, 0xDA, 255),
        )
        # 橡皮擦
        d.polygon(
            [at(ferrule_end, w / 2), at(L, w / 2), at(L, -w / 2), at(ferrule_end, -w / 2)],
            fill=(0xF5, 0x8C, 0xA0, 255),
        )

    img = Image.alpha_composite(img, layer(pencil))

    return img.resize((size, size), Image.LANCZOS)


def write_ico(path: Path, images: list[tuple[int, bytes]]) -> None:
    """寫出 ICO：每個尺寸內嵌一張 PNG（Windows Vista 之後都支援，且保留透明度）。"""
    count = len(images)
    header = struct.pack("<HHH", 0, 1, count)
    offset = 6 + 16 * count
    entries, data = b"", b""
    for size, png in images:
        dim = 0 if size >= 256 else size  # 0 代表 256
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(png), offset + len(data))
        data += png
    path.write_bytes(header + entries + data)


def main() -> None:
    rendered = {s: render(s) for s in SIZES}

    rendered[256].save(OUT_DIR / "icon.png")
    pngs = []
    for s in SIZES:
        buf = io.BytesIO()
        rendered[s].save(buf, format="PNG")
        pngs.append((s, buf.getvalue()))
    write_ico(OUT_DIR / "icon.ico", pngs)

    # 檢查：四個角落必須是完全透明的，才是真的透明背景
    corners = [rendered[256].getpixel(xy)[3] for xy in [(0, 0), (255, 0), (0, 255), (255, 255)]]
    assert corners == [0, 0, 0, 0], f"背景不是透明的：角落 alpha = {corners}"
    print(f"已輸出 {OUT_DIR / 'icon.png'} 與 {OUT_DIR / 'icon.ico'}（{len(SIZES)} 個尺寸，背景透明）")

    if len(sys.argv) > 1:
        # 預覽圖：淺色與深色底各一排；小尺寸用最近鄰放大 4 倍以便檢查像素
        width = 256 + 40 + 4 * (16 + 24 + 32 + 48) + 90
        sheet = Image.new("RGBA", (width, 600), (255, 255, 255, 255))
        sheet.paste(Image.new("RGBA", (width, 300), (32, 32, 36, 255)), (0, 300))
        for y0 in (0, 300):
            sheet.alpha_composite(rendered[256], (10, y0 + 22))
            x = 256 + 40
            for s in (48, 32, 24, 16):
                sheet.alpha_composite(rendered[s].resize((s * 4, s * 4), Image.NEAREST), (x, y0 + 22))
                sheet.alpha_composite(rendered[s], (x, y0 + 22 + s * 4 + 12))
                x += s * 4 + 10
        sheet.save(sys.argv[1])
        print(f"預覽：{sys.argv[1]}")


if __name__ == "__main__":
    main()
