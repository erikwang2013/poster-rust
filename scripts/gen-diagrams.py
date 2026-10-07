#!/usr/bin/env python3
"""生成 README 用 SVG 图（中/英各一套）：架构设计 / 功能设计 / 请求周期 / 生命周期。

用法：python3 scripts/gen-diagrams.py
输出：docs/{architecture,feature-design,request-flow,lifecycle}-{zh,en}.svg

风格与配色取自项目宠物 Posty（assets/pet.svg）：#FF6B6B / #4ECDC4 / #45B7D1 /
#FFEAA7 / #2D3436 / #FFFDF8。每张图底部标注版权 https://erik.xyz，右下角嵌入 Posty。
"""

from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"

INK = "#2D3436"
CREAM = "#FFFDF8"
PAPER = "#FFFFFF"
GROUND = "#F7F5F2"
RED = "#FF6B6B"
TEAL = "#4ECDC4"
BLUE = "#45B7D1"
YELLOW = "#FFEAA7"
GREY = "#E4EBF0"
MUTED = "#8A6A5B"

FONT = "system-ui,-apple-system,'PingFang SC','Microsoft YaHei','Noto Sans CJK SC',sans-serif"
COPYRIGHT_ZH = "Copyright © 2026 erik <erik@erik.xyz> — https://erik.xyz"
COPYRIGHT_EN = "Copyright © 2026 erik <erik@erik.xyz> — https://erik.xyz"

_pet_inner_cache = None


def pet_group(x, y, scale, opacity=1.0):
    """把项目宠物 Posty（assets/pet.svg 的内容）内嵌到图中。"""
    global _pet_inner_cache
    if _pet_inner_cache is None:
        raw = (ROOT / "assets" / "pet.svg").read_text(encoding="utf-8")
        inner = raw.split(">", 1)[1].rsplit("</svg>", 1)[0]
        # 去掉 <title>/<desc>，只留图形
        import re
        inner = re.sub(r"<title>.*?</title>", "", inner, flags=re.S)
        inner = re.sub(r"<desc>.*?</desc>", "", inner, flags=re.S)
        _pet_inner_cache = inner
    return (
        f'<g transform="translate({x},{y}) scale({scale})" opacity="{opacity}">'
        f"{_pet_inner_cache}</g>"
    )


def esc(t):
    return t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def text(x, y, content, size=14, fill=INK, weight="400", anchor="middle", opacity=1.0):
    return (
        f'<text x="{x}" y="{y}" font-family="{FONT}" font-size="{size}" fill="{fill}" '
        f'font-weight="{weight}" text-anchor="{anchor}" opacity="{opacity}">{esc(content)}</text>'
    )


def box(x, y, w, h, fill=PAPER, stroke=GREY, rx=10, dash=None, sw=1.5):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    return (
        f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" fill="{fill}" '
        f'stroke="{stroke}" stroke-width="{sw}"{d}/>'
    )


def lines_centered(cx, y, items, size=12, fill=INK, lh=17, opacity=1.0, weight="400"):
    out = []
    for i, t in enumerate(items):
        out.append(text(cx, y + i * lh, t, size=size, fill=fill, opacity=opacity, weight=weight))
    return "".join(out)


def arrow_defs(color=INK):
    return f'''<defs>
  <marker id="ah" markerWidth="10" markerHeight="10" refX="8" refY="4" orient="auto">
    <path d="M0,0 L9,4 L0,8 z" fill="{color}"/>
  </marker>
</defs>'''


def arrow(x1, y1, x2, y2, color=INK, dash=None, width=1.8):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    return (
        f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{color}" stroke-width="{width}"'
        f' marker-end="url(#ah)"{d}/>'
    )


def arrow_path(d, color=INK, dash=None, width=1.8):
    da = f' stroke-dasharray="{dash}"' if dash else ""
    return (
        f'<path d="{d}" fill="none" stroke="{color}" stroke-width="{width}" '
        f'marker-end="url(#ah)"{da}/>'
    )


def wrap_text(s, limit):
    """按空格优先、无空格按字符折行。"""
    tokens = s.split(" ") if " " in s else list(s)
    out, line = [], ""
    for t in tokens:
        candidate = t if not line else (line + " " + t if " " in s else line + t)
        if len(candidate) > limit and line:
            out.append(line)
            line = t
        else:
            line = candidate
    if line:
        out.append(line)
    return out


def wrap_svg(title_lines, width, height, body, copyright_line):
    head = "".join(text(24, 40 + i * 26, t, size=22 if i == 0 else 15, weight="700" if i == 0 else "400",
                        fill=INK if i == 0 else MUTED, anchor="start") for i, t in enumerate(title_lines))
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" role="img" aria-label="{esc(title_lines[0])}">
{arrow_defs()}
<rect width="{width}" height="{height}" fill="{PAPER}"/>
{head}
{body}
<line x1="24" y1="{height - 44}" x2="{width - 24}" y2="{height - 44}" stroke="{GREY}" stroke-width="1.5"/>
{text(24, height - 20, copyright_line, size=12, fill=MUTED, anchor="start")}
{pet_group(width - 156, height - 142, 0.36, 0.95)}
</svg>
'''


# ─────────────────────────── 1) 架构设计 ───────────────────────────

def architecture(lang):
    zh = lang == "zh"
    W, H = 1080, 780
    body = []
    layers = [
        (96, 118, "#FFECEC", RED, "API 层" if zh else "API Layer",
         [("辅助函数" if zh else "Helpers", "captcha_create · captcha_verify · poster_create"),
          ("Guard 请求守卫" if zh else "Guard", ("原生 Rust；接线期构造、请求期克隆" if zh else "Native Rust; built at wiring, cloned per request")),
          ("8 框架集成（feature 门控）" if zh else "8 integrations (feature-gated)",
           "axum · actix-web · rocket · poem · salvo · warp · bee-rust · e-cat")]),
        (256, 118, "#E7F7F5", TEAL, "业务层" if zh else "Business Layer",
         [("CaptchaManager", ("点击 / 旋转 / 滑块 / 随机 + 限流 + 轨迹校验" if zh else "Click / Rotate / Slider / Random + rate limit + trajectory")),
          ("PosterBuilder", ("14 种元素 + JSON 模板 {{变量}} + 往返导出" if zh else "14 elements + JSON templates {{vars}} + round-trip"))]),
        (416, 118, "#E8F3F9", BLUE, "核心层" if zh else "Core Layer",
         [("ImageDriver", ("image crate 单驱动：画布 / TTF 文字 / 图元" if zh else "Single `image`-crate driver: canvas / TTF text / primitives")),
          ("Storage", ("Memory / File / Redis（feature）" if zh else "Memory / File / Redis (feature)")),
          ("qrcode 封装" if zh else "qrcode wrapper", ("模块矩阵 → 精确像素" if zh else "module matrix → exact pixels")),
          ("PosterConfig", ("键名对齐 PHP config/poster.php" if zh else "key names aligned with PHP"))]),
        (576, 118, "#FFF8E1", "#E8B93C", "基础层" if zh else "Foundation",
         [("Rust ≥ 1.85（edition 2024）" if zh else "Rust ≥ 1.85 (edition 2024)",
           "全依赖纯 Rust，无 GD / ImageMagick 等系统库" if zh else "All deps pure Rust — no GD / ImageMagick"),
          ("随包素材" if zh else "Bundled assets",
           "宠物 Posty · 6 张验证码背景 · 阿里巴巴普惠体" if zh else "Posty · 6 captcha backgrounds · Alibaba PuHuiTi font")]),
    ]
    y = 84
    for ly, lh, fill, accent, label, items in layers:
        body.append(box(88, ly, 880, lh, fill=fill, stroke=accent))
        body.append(text(108, ly + 26, label, size=15, weight="700", fill=INK, anchor="start"))
        col_w = 860 / len(items) - 12
        for i, (t, sub) in enumerate(items):
            cx = 108 + i * (col_w + 12)
            body.append(box(cx, ly + 36, col_w, lh - 52, fill=CREAM, stroke=GREY, rx=8))
            body.append(text(cx + col_w / 2, ly + 60, t, size=13, weight="700"))
            # 副标题按 ~22 字宽折行
            lines_out = wrap_text(sub, int(col_w / 11.5))
            body.append(lines_centered(cx + col_w / 2, ly + 80, lines_out[:3], size=10.5, fill=MUTED, lh=14))
    for ly in (256, 416, 576):
        body.append(arrow(528, ly - 38, 528, ly - 8, color=MUTED))
    title = (["poster-rust 架构设计", "分层依赖：上层只调用下层，替换存储或素材不影响业务代码"] if zh else
             ["poster-rust Architecture", "Layered: upper layers call lower ones — swapping storage or assets leaves business code untouched"])
    return wrap_svg(title, W, H, "".join(body), COPYRIGHT_ZH if zh else COPYRIGHT_EN)


# ─────────────────────────── 2) 功能设计 ───────────────────────────

def feature_design(lang):
    zh = lang == "zh"
    W, H = 1080, 640
    body = []

    # 左：验证码
    body.append(box(48, 84, 470, 470, fill="#FFECEC", stroke=RED))
    body.append(text(68, 112, "验证码 Captcha", size=16, weight="700", anchor="start"))
    body.append(lines_centered(283, 140,
        (["四种交互：点击（文字 / 11 种程序化图标）· 旋转 · 滑块（矩形 / 凹凸拼图）· 随机切换"] if zh
         else ["Four flows: Click (text / 11 procedural icons) · Rotate · Slider (square / jigsaw) · Random"]),
        size=11, fill=MUTED, lh=16))
    sec = ([("一次性", "验证成功或超次即删 key"), ("防暴力", "默认最多 3 次"),
            ("有效期", "默认 300 秒"), ("限流", "跨 key 窗口 30 次 / 60 秒"),
            ("轨迹校验", "可选：点数 / 耗时 / 线性度"),
            ("背景", "内置 6 张 / 目录随机 / 程序化三风格")] if zh else
           [("One-shot", "key deleted on success or too many attempts"), ("Brute-force", "3 attempts by default"),
            ("TTL", "300 seconds by default"), ("Rate limit", "cross-key window 30 / 60s"),
            ("Trajectory", "optional: points / duration / linearity"),
            ("Backgrounds", "6 bundled / directory / 3 procedural styles")])
    for i, (t, sub) in enumerate(sec):
        y = 168 + i * 62
        body.append(box(68, y, 430, 50, fill=CREAM, stroke=GREY, rx=8))
        body.append(text(86, y + 21, t, size=12.5, weight="700", anchor="start"))
        body.append(text(86, y + 39, sub, size=10.5, fill=MUTED, anchor="start"))

    # 右：海报
    body.append(box(562, 84, 470, 470, fill="#E8F3F9", stroke=BLUE))
    body.append(text(582, 112, "海报生成 Poster", size=16, weight="700", anchor="start"))
    body.append(lines_centered(797, 140,
        (["链式 Builder API，14 种元素 + JSON 模板（{{变量}} 替换、导出往返一致）"] if zh
         else ["Fluent Builder, 14 element types + JSON templates ({{vars}}, lossless round-trip)"]),
        size=11, fill=MUTED, lh=16))
    elems = ([("基础", "文字 · 图片 · 头像 · 形状 · 分割线"),
              ("复合", "二维码 · 表格 · 水印 · 图表（柱/折/饼）· 日历"),
              ("装饰", "艺术字（描边/阴影/渐变/霓虹）· Emoji · 字体图标 · 颜文字"),
              ("宠物", "Posty 可画进海报，也可作缺图占位"),
              ("输出", "jpg / png / webp / gif + base64 data URI"),
              ("模板", "use_template · replace_elements · to_array 往返")] if zh else
             [("Basic", "Text · Image · Avatar · Shape · Line"),
              ("Composite", "QR · Table · Watermark · Chart (bar/line/pie) · Calendar"),
              ("Decorative", "Artistic text (stroke/shadow/gradient/neon) · Emoji · Icon · Emoticon"),
              ("Mascot", "Posty in the poster, or as the missing-image placeholder"),
              ("Output", "jpg / png / webp / gif + base64 data URI"),
              ("Templates", "use_template · replace_elements · to_array round-trip")])
    for i, (t, sub) in enumerate(elems):
        y = 168 + i * 62
        body.append(box(582, y, 430, 50, fill=CREAM, stroke=GREY, rx=8))
        body.append(text(600, y + 21, t, size=12.5, weight="700", anchor="start"))
        body.append(text(600, y + 39, sub, size=10.5, fill=MUTED, anchor="start"))

    title = (["poster-rust 功能设计", "两大模块：人机验证与海报出图"] if zh
             else ["poster-rust Feature Design", "Two modules: human verification and poster rendering"])
    return wrap_svg(title, W, H, "".join(body), COPYRIGHT_ZH if zh else COPYRIGHT_EN)


# ─────────────────────────── 3) 请求周期 ───────────────────────────

def request_flow(lang):
    zh = lang == "zh"
    W, H = 1160, 600
    body = []

    steps = ([("HTTP 请求", "GET /captcha/new · GET /captcha/{key} · POST /captcha/verify"),
              ("框架提取器", "axum / actix-web / rocket / poem / salvo / warp / bee-rust / e-cat"),
              ("Guard", "接线期构造（存储探针快速失败）→ 请求期克隆（两次原子计数）")] if zh else
             [("HTTP request", "GET /captcha/new · GET /captcha/{key} · POST /captcha/verify"),
              ("Framework extractor", "axum / actix-web / rocket / poem / salvo / warp / bee-rust / e-cat"),
              ("Guard", "built at wiring (storage probe fails fast) → cloned per request (two atomic bumps)")])
    for i, (t, sub) in enumerate(steps):
        x = 60 + i * 380
        body.append(box(x, 96, 330, 76, fill="#F1F5F8", stroke=GREY))
        body.append(text(x + 165, 128, t, size=14, weight="700"))
        body.append(lines_centered(x + 165, 148, [sub[:52], sub[52:104]] if len(sub) > 52 else [sub],
                                   size=10, fill=MUTED, lh=13))
        if i < 2:
            body.append(arrow(x + 330, 134, x + 380, 134, color=MUTED))

    lanes = ([("生成", TEAL, "#E7F7F5", ["create(类型)", "画布 + 目标 / 缺口 / 旋转",
                                        "PNG 持久化到 Storage（TTL）", "返回 CaptchaResult JSON"]),
              ("出图", BLUE, "#E8F3F9", ["GET {path}/{key}", "image_bytes(key)",
                                        "image/png", "Cache-Control: no-store"]),
              ("校验", RED, "#FFECEC", ["verify(key, answer)", "限流 → 取存储 → 原子自增尝试数",
                                       "容差比对（18px / ±5° / ±4px）", "通过→删 key；失败→可重试"]),
              ("响应", "#E8B93C", "#FFF8E1", ["JSON / PNG", "一次性 key", "≤3 次尝试", "TTL 300s"]),
              ] if zh else
             [("Generate", TEAL, "#E7F7F5", ["create(kind)", "canvas + targets / notch / rotation",
                                            "PNG persisted to Storage (TTL)", "returns CaptchaResult JSON"]),
              ("Serve image", BLUE, "#E8F3F9", ["GET {path}/{key}", "image_bytes(key)",
                                               "image/png", "Cache-Control: no-store"]),
              ("Verify", RED, "#FFECEC", ["verify(key, answer)", "rate limit → storage → atomic attempt count",
                                         "tolerance check (18px / ±5° / ±4px)", "pass → delete key; fail → retry"]),
              ("Response", "#E8B93C", "#FFF8E1", ["JSON / PNG", "one-shot key", "≤3 attempts", "TTL 300s"]),
              ])
    for i, (name, accent, fill, items) in enumerate(lanes):
        x = 60 + i * 265
        body.append(box(x, 232, 245, 250, fill=fill, stroke=accent))
        body.append(text(x + 122, 262, name, size=14, weight="700"))
        for j, it in enumerate(items):
            y = 292 + j * 46
            body.append(box(x + 16, y - 20, 213, 36, fill=CREAM, stroke=GREY, rx=7))
            body.append(lines_centered(x + 122, y - 2, wrap_text(it, 22),
                                       size=10, fill=INK, lh=12))
        if i < 3:
            body.append(arrow(x + 245, 357, x + 265, 357, color=MUTED))
    body.append(arrow(225, 172, 225, 232, color=MUTED))
    body.append(arrow(448, 172, 448, 232, color=MUTED))
    body.append(arrow(682, 172, 682, 232, color=MUTED))
    body.append(arrow(1140 - 60, 172, 1140 - 60, 232, color=MUTED))

    title = (["poster-rust 请求周期", "一次 HTTP 请求：提取守卫 → 生成 / 出图 / 校验 → 响应"] if zh
             else ["poster-rust Request Cycle", "One HTTP request: extract guard → generate / serve / verify → respond"])
    return wrap_svg(title, W, H, "".join(body), COPYRIGHT_ZH if zh else COPYRIGHT_EN)


# ─────────────────────────── 4) 生命周期 ───────────────────────────

def lifecycle(lang):
    zh = lang == "zh"
    W, H = 1080, 620
    body = []

    def timeline(y, title, accent, fill, steps, notes):
        out = [box(48, y, 984, 240, fill=fill, stroke=accent, rx=12),
               text(72, y + 30, title, size=15, weight="700", anchor="start")]
        n = len(steps)
        step_w = 200
        gap = (984 - 48 - n * step_w) / (n - 1)
        for i, (t, sub) in enumerate(steps):
            x = 72 + i * (step_w + gap)
            out.append(box(x, y + 52, step_w, 74, fill=CREAM, stroke=GREY, rx=9))
            out.append(text(x + step_w / 2, y + 78, t, size=12.5, weight="700"))
            out.append(lines_centered(x + step_w / 2, y + 97, sub, size=10, fill=MUTED, lh=13))
            if i < n - 1 and gap > 0:
                out.append(arrow(x + step_w + 3, y + 89, x + step_w + gap - 3, y + 89, color=MUTED))
        out.append(lines_centered(540, y + 156, notes, size=11, fill=MUTED, lh=16))
        return "".join(out)

    body.append(timeline(84, ("验证码生命周期" if zh else "Captcha lifecycle"), RED, "#FFECEC",
        ([("生成", ["随机 key（32 位 hex）", "画布 + 答案"]),
          ("存储", ["答案 + 类型 + attempts=0", "TTL 300 秒"]),
          ("校验", ["限流 → 原子自增", "≤ 3 次尝试"]),
          ("终局", ["通过：删 key（一次性）", "超次 / 过期：作废"])] if zh else
         [("Generate", ["random key (32-hex)", "canvas + answer"]),
          ("Store", ["answer + type + attempts=0", "TTL 300s"]),
          ("Verify", ["rate limit → atomic bump", "≤ 3 attempts"]),
          ("Final", ["pass: key deleted (one-shot)", "over-limit / expired: invalid"])]),
        (["容差：点击 18px · 旋转 ±5° · 滑块 ±4px；限流跨 key 生效（默认 30 次 / 60 秒）"] if zh else
         ["Tolerance: click 18px · rotate ±5° · slider ±4px; cross-key rate limit (30 / 60s by default)"])))

    body.append(timeline(348, ("海报生命周期" if zh else "Poster lifecycle"), BLUE, "#E8F3F9",
        ([("构建", ["PosterBuilder（默认 750×1334）", "链式 add_* ×14"]),
          ("模板（可选）", ["JSON + {{变量}} 替换", "replace_elements 控制叠放"]),
          ("渲染", ["背景（纯色 / 图片 cover / 渐变）", "逐元素画到同一画布"]),
          ("输出", ["save：jpg / png / webp / gif", "output：base64 data URI"])] if zh else
         [("Build", ["PosterBuilder (default 750×1334)", "fluent add_* ×14"]),
          ("Template (optional)", ["JSON + {{vars}} substitution", "replace_elements controls layering"]),
          ("Render", ["background (solid / cover image / gradient)", "elements drawn onto one canvas"]),
          ("Output", ["save: jpg / png / webp / gif", "output: base64 data URI"])]),
        (["宠物 Posty 可画进海报（add_pet）或作为缺图占位（Placeholder::Pet）；模板导出可再次导入，结构一致"] if zh else
         ["Posty can be drawn in (add_pet) or used as the missing-image placeholder (Placeholder::Pet); templates round-trip losslessly"])))

    title = (["poster-rust 生命周期", "验证码从生成到作废、海报从构建到输出"] if zh
             else ["poster-rust Lifecycle", "Captcha from creation to invalidation; poster from build to output"])
    return wrap_svg(title, W, H, "".join(body), COPYRIGHT_ZH if zh else COPYRIGHT_EN)


def main():
    DOCS.mkdir(exist_ok=True)
    for lang in ("zh", "en"):
        (DOCS / f"architecture-{lang}.svg").write_text(architecture(lang), encoding="utf-8")
        (DOCS / f"feature-design-{lang}.svg").write_text(feature_design(lang), encoding="utf-8")
        (DOCS / f"request-flow-{lang}.svg").write_text(request_flow(lang), encoding="utf-8")
        (DOCS / f"lifecycle-{lang}.svg").write_text(lifecycle(lang), encoding="utf-8")
        print(f"wrote 4 diagrams for {lang}")


if __name__ == "__main__":
    main()
