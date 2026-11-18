# /// script
# requires-python = ">=3.10"
# dependencies = ["pillow"]
# ///
"""產生 feditor 的應用程式圖示：透明背景、帶藍色外框的文字文件、右下角的終端機視窗。

用法（在專案根目錄）：
    uv run ui/assets/make_icon.py            # 產生 ui/assets/icon.png、icon.ico 與 icon-macos.png
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

    # ── 終端機視窗（右下角，蓋在文件上）──────────────────────────
    x0, y0, x1, y1 = 0.28, 0.40, 0.97, 0.94
    rad = 0.07
    box = [P(x0, y0), P(x1, y1)]
    # 陰影
    sh = layer(lambda d: d.rounded_rectangle(
        [P(x0 + 0.012, y0 + 0.02), P(x1 + 0.012, y1 + 0.02)], radius=rad * S, fill=(8, 30, 110, 130)))
    img = Image.alpha_composite(img, sh.filter(ImageFilter.GaussianBlur(radius=0.008 * S)))

    def term(d):
        # 外框（深藍）→ 內部深色底
        d.rounded_rectangle(box, radius=rad * S, fill=NAVY)
        ins = 0.022 if small else 0.018
        d.rounded_rectangle([P(x0 + ins, y0 + ins), P(x1 - ins, y1 - ins)],
                            radius=(rad - ins) * S, fill=(0x1E, 0x1E, 0x2E, 255))
        # 標題列
        bar_h = 0.11 if small else 0.075
        d.rounded_rectangle([P(x0 + ins, y0 + ins), P(x1 - ins, y0 + ins + bar_h + rad)],
                            radius=(rad - ins) * S, fill=(0x3A, 0x4A, 0x8C, 255))
        d.rectangle([P(x0 + ins, y0 + ins + bar_h), P(x1 - ins, y0 + ins + bar_h + rad)],
                    fill=(0x1E, 0x1E, 0x2E, 255))
        if not small:
            for i, c in enumerate([(0xFF, 0x6B, 0x6B), (0xFF, 0xC8, 0x3D), (0x5F, 0xD3, 0x8A)]):
                cx, cy = P(x0 + 0.055 + i * 0.045, y0 + ins + bar_h / 2)
                r = 0.014 * S
                d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=c + (255,))
        # 提示符號 > 與底線 _
        green = (0x5F, 0xE3, 0x9A, 255)
        lw = max(1, int((0.06 if small else 0.05) * S))
        cy = (y0 + y1) / 2 + 0.08
        h = 0.11
        cxl = x0 + 0.12
        pts = [P(cxl, cy - h), P(cxl + h, cy), P(cxl, cy + h)]
        d.line(pts, fill=green, width=lw, joint="curve")
        for p in (pts[0], pts[2]):
            r = lw / 2
            d.ellipse([p[0] - r, p[1] - r, p[0] + r, p[1] + r], fill=green)
        d.line([P(cxl + 0.19, cy + h), P(cxl + 0.34, cy + h)], fill=green, width=lw)

    img = Image.alpha_composite(img, layer(term))
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

    # macOS 版：Dock 與 .icns 的圖案要縮到畫布的 80% 左右、四周留透明邊，不然比一般 App 的圖示大
    mac = Image.new("RGBA", (512, 512), (0, 0, 0, 0))
    mac.alpha_composite(render(410), (51, 51))
    mac.save(OUT_DIR / "icon-macos.png")

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
