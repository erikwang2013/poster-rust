# poster-rust

<p align="center">
  <img src="assets/pet.svg" width="200" alt="poster-rust 项目宠物 Posty" />
</p>

Rust 图片验证码与海报生成工具包 —— 框架无关核心 + Guard 请求守卫 + axum / actix-web / rocket / poem / salvo / warp / bee-rust / e-cat 集成。

[English Documentation](README_EN.md) | [架构设计文档](docs/architecture.md)

## 项目简介

poster-rust 是一个 Rust 图像工具包，只做两件事，并且做到够用：

| 能力 | 说明 |
|------|------|
| **验证码** | 点击 / 旋转 / 滑块三种人机校验 + 随机切换，纯 Rust 生成图片与答案，不依赖第三方服务 |
| **海报生成** | 链式 Builder API，14 种元素覆盖文字、图片、二维码、表格、图表、日历等排版需求 |
| **框架无关** | 核心零框架依赖（纯 Rust 图像栈，无系统库），可作为普通 crate 使用 |
| **开箱即用** | 3 个辅助函数 + 原生 Guard 请求守卫 + 8 个框架集成，feature 门控 |
| **可替换** | 存储后端（Memory / File / Redis）为 trait 实现，按需替换 |

> 项目宠物 **Posty** —— 一只由海报本体、二维码卡片与滑块拼图组成的吉祥物，正好对应这个包的两大能力：出图与验证。它随包分发（[`assets/pet.svg`](assets/pet.svg) / [`assets/pet.png`](assets/pet.png)），可用 `add_pet()` 画进海报，也可配置为缺图占位图。

## 项目结构

```
poster-rust/
├── src/
│   ├── captcha/                # 验证码模块：4 种类型 + 工厂 + 管理器 + 限流 + 轨迹校验
│   ├── poster/                 # 海报模块：Builder + Template + 14 种元素（elements/）
│   ├── drivers/                # 图像驱动：画布 / TTF 文字 / 颜色
│   ├── storage/                # 验证数据存储：Memory / File / Redis
│   ├── integrations/           # 框架集成：8 个（feature 门控）
│   ├── guard.rs                # 原生 Guard 请求守卫（框架无关）
│   ├── qrcode.rs               # 二维码（qrcode crate 封装）
│   ├── config.rs               # 配置（键名对齐 PHP config/poster.php）
│   └── assets.rs               # 内置素材：宠物 / 背景 / 默认字体
├── assets/                     # pet.svg / pet.png / backgrounds / fonts
├── tests/                      # 集成测试，目录结构与 src/ 镜像
├── examples/                   # 可直接运行的示例（含 axum / actix 服务）
└── docs/                       # 架构文档（Mermaid）
```

## 功能

### 验证码（三种方式 + 随机切换）

| 类型 | 说明 |
|------|------|
| 点击验证 `click` | 用户按顺序点击图片上的目标文字（或程序化图标） |
| 旋转验证 `rotate` | 用户拖动滑块将图片旋转回正确角度 |
| 滑块验证 `slider` | 用户拖动拼图块到缺口位置（矩形 / 凹凸拼图两种轮廓） |
| 随机切换 `random` | 随机选取以上三种验证码之一 |

### 海报生成

链式 Builder API，支持 14 种元素：

| 元素 | 方法 | 说明 |
|------|------|------|
| 文字 | `add_text()` | 自动换行，对齐，多行，旋转 |
| 图片 | `add_image()` | 缩放，圆角，阴影 |
| 头像 | `add_avatar()` | 圆形裁剪，边框 |
| 二维码 | `add_qrcode()` | 纯 Rust 生成，中心 Logo，底部文案 |
| 形状 | `add_shape()` | 矩形/圆形，填充/描边，圆角，透明度 |
| 分割线 | `add_line()` | 颜色，宽度 |
| 水印 | `add_watermark()` | 平铺文字，角度，间距 |
| 表格 | `add_table()` | 表头，斑马纹，列宽 |
| 图表 | `add_chart()` | 柱状图 / 折线图 / 饼图 |
| 日历 | `add_calendar()` | 月历，高亮日期，标注 |
| 艺术字体 | `add_artistic_text()` | 描边 / 阴影 / 渐变 / 霓虹 |
| Emoji | `add_emoji()` | 彩色 emoji 表情渲染 |
| 字体图标 | `add_icon()` | FontAwesome 图标渲染 |
| 颜文字 | `add_emoticon()` | 日式颜文字 / 自定义表情 |
| 项目宠物 | `add_pet()` | 吉祥物 Posty 画进海报 |

## 安装

```bash
cargo add poster-rust
```

系统要求：Rust ≥ 1.85（edition 2024）。全部依赖为纯 Rust，无需 GD / ImageMagick 等系统库。

可选 feature：

| feature | 说明 |
|---------|------|
| `axum` / `actix` / `rocket` / `poem` / `salvo` / `warp` | 对应框架集成（提取器 + 出图路由） |
| `bee` | bee-rust 集成（含 `axum`） |
| `ecat` | e-cat 集成（含 `axum`） |
| `redis` | Redis 验证码存储（分布式部署） |

```toml
[dependencies]
poster-rust = { version = "0.1", features = ["axum", "redis"] }
```

## 使用说明

### 一、验证码

#### 1. 点击验证码 (ClickCaptcha)

用户需要按顺序点击图片上的目标文字（如"合""家""欢"），验证人类操作。

```rust
use poster::captcha::{Answer, CaptchaManager};

let manager = CaptchaManager::new()?;

let result = manager.create(Some("click"))?
    .set_difficulty("hard")              // easy(2目标) | medium(3目标) | hard(4目标)
    .set_background("/path/to/bg.jpg")   // 可选：自定义背景
    .generate()?;

// result.key    → 验证唯一标识，传给前端
// result.image  → data:image/png;base64,… 图片
// result.extra  → { "texts": [ { "order": 1, "text": "合" }, … ] }
//   前端按 order 顺序展示提示文字，用户依次点击对应位置
//   （目标坐标不返回，仅服务端校验）

// 校验：前端提交用户点击坐标
let pass = manager.verify(&result.key, Answer::Click(vec![
    (120.0, 80.0), (200.0, 150.0), (310.0, 95.0),
]))?;   // 容差半径 18px
```

`set_target_type("icon")` 可把目标文字换成程序化生成的矢量图形（11 种，无需图片素材）：
`extra.texts` 每项会多一个 `thumb`（该图形的 base64 小图），供前端展示点击提示；校验仍是坐标比对。

#### 2. 旋转验证码 (RotateCaptcha)

系统随机旋转图片 30°~330°，用户拖动滑块将图片旋转回正。

```rust
let result = manager.create(Some("rotate"))?
    .set_size(200)                 // 圆形直径 60-400（默认 200）
    .set_angle_range(45.0, 315.0)  // 自定义旋转角度范围
    .generate()?;

// 前端只展示旋转后的图片（不含角度答案）
let pass = manager.verify(&result.key, Answer::Rotate(185.0))?;  // ±5° 容差
```

#### 3. 滑块验证码 (SliderCaptcha)

系统从背景切出拼图块并偏移，用户拖动拼图到缺口位置。

```rust
let result = manager.create(Some("slider"))?.generate()?;
// result.extra → { "puzzle": "data:image/png;base64,…", "puzzle_w": 50, "puzzle_h": 50 }

let pass = manager.verify(&result.key, Answer::Slider(173.0))?;  // 用户滑动的 x 像素，±4px 容差
```

拼图形状支持两种，默认 `square`：

| shape | 效果 |
|-------|------|
| `square`（默认） | 矩形缺口 |
| `jigsaw` | 凹凸拼图：四边各自随机半圆凸/凹（16 种组合），缺口与拼图块共用同一轮廓 |

```rust
let result = manager.create(Some("slider"))?.set_shape("jigsaw").generate()?;
// 拼图块 PNG 是外扩后的外接矩形；服务端答案 x 是 PNG 左上角；
// ±4px 容差与轨迹校验均与 square 完全一致。
```

#### 4. 随机切换 (RandomCaptcha)

```rust
let result = manager.create(Some("random"))?.generate()?;
// result.captcha_type 返回实际选中的类型: "click" | "rotate" | "slider"
```

#### 验证安全特性

| 特性 | 说明 |
|------|------|
| 一次性 | 验证成功/超过最大次数后 key 删除 |
| 防暴力 | 默认最多验证 3 次（可配置） |
| 有效期 | 默认 300 秒（可配置） |
| 随机性 | 每次生成的背景颜色、噪声、目标位置均随机 |
| 会话级限流 | 跨 key 生效的窗口限流（默认 60 秒内 30 次） |
| 行为轨迹 | 可选（默认关闭）：校验拖动轨迹的点数/耗时/线性度 |
| 背景美化 | 内置 6 张背景图，或程序化渐变（简约/活泼/自然三风格随机） |
| 画布下限 | 背景过小时直接报错而不是退化 |

#### 行为轨迹校验（可选）

```rust
// PosterConfig.captcha.trajectory 开启后，slider / rotate 需要提交拖动轨迹：
let pass = manager.verify(&result.key, Answer::SliderWithTrail {
    x: 173.0,
    trail: vec![(12.0, 3.0, 0.0), (40.0, 9.0, 22.0)],  // (x, y, t_ms)
    duration_ms: 1200,
})?;
```

#### 背景图片配置

验证码背景支持三级优先级（见 `BackgroundSource`）：

1. **单张图片** —— `set_background("/path/to/bg.jpg")`
2. **图片目录** —— `BackgroundSource::Dir(path)`，目录内随机选用
3. **程序化生成** —— `BackgroundSource::Procedural`，三风格随机
4. 默认 `BackgroundSource::Embedded`：随包分发的 6 张 400×250 渐变背景

```rust
use poster::config::{self, BackgroundSource, PosterConfig};

let mut cfg = PosterConfig::default();
cfg.captcha.background_source = BackgroundSource::Dir("/path/to/my-backgrounds".into());
config::set_global(cfg)?;
```

### 二、海报生成

#### 基础用法

```rust
use poster::PosterBuilder;
use poster::poster::builder::Direction;

let mut builder = PosterBuilder::new()?;   // 默认 750×1334
builder.width(750).height(1334);

// 设置背景（三选一）
builder.background("#FFFFFF");                                 // 纯色背景
builder.background("/path/to/bg.jpg");                         // 图片背景（自动缩放）
builder.background_gradient("#FF6B6B", "#FF8E53", Direction::Vertical);

// 输出
builder.save("/output/poster.jpg", None)?;       // 保存到文件（路径推格式，质量取配置默认）
builder.save("/output/poster.jpg", Some(90))?;   // 显式质量 0-100
let data_url = builder.output("png", Some(90))?; // base64 data URL
```

#### 文字 `add_text()`

```rust
use poster::drivers::TextOptions;
use poster::poster::elements::text::TextElement;

builder.add_text("新品首发", TextElement {
    x: 80,                    // 横坐标
    y: 120,                   // 纵坐标（基线位置）
    style: TextOptions {
        size: 48.0,                   // 字号
        color: "#333333".into(),      // 颜色
        font: None,                   // 字体文件，None = 随包分发的阿里巴巴普惠体
        align: poster::drivers::TextAlign::Center,  // left | center | right
        max_width: 600.0,             // 最大宽度（自动换行）
        line_height: Some(72.0),      // 行高
        angle: 0.0,                   // 旋转角度
        ..Default::default()
    },
    ..Default::default()
});
```

#### 图片 / 头像 / 二维码

```rust
use poster::drivers::{OverlayOptions, ShadowOptions};
use poster::poster::elements::{avatar::AvatarElement, image::ImageElement, qrcode::QrcodeElement};

builder.add_image("/path/to/product.jpg", ImageElement {
    x: 75, y: 280,
    style: OverlayOptions {
        width: Some(600),             // 渲染宽度（自动缩放）
        height: Some(600),
        radius: 12,                   // 圆角半径
        shadow: Some(ShadowOptions {  // 阴影（可选）
            color: "#00000033".into(), offset_x: 4, offset_y: 4, blur: 10, opacity: None,
        }),
    },
    ..Default::default()
});

builder.add_avatar("/path/to/avatar.jpg", AvatarElement {
    x: 80, y: 60, size: 120,               // 头像尺寸（正方形）
    border: Some("#FF6B6B".into()),        // 边框颜色（可选）
    ..Default::default()
});

builder.add_qrcode("https://example.com/page/123", QrcodeElement {
    x: 275, y: 1050, size: 200,
    level: "H".into(),                     // 容错级别 L | M | Q | H
    logo: Some("/path/to/logo.png".into()), // 中心 Logo（可选）
    label: Some("扫码查看详情".into()),      // 底部文案（可选）
    label_size: 14, label_color: "#999999".into(),
    ..Default::default()
});
```

#### 形状 / 分割线 / 水印

```rust
use poster::poster::elements::{line::LineElement, shape::ShapeElement, watermark::WatermarkElement};

builder.add_shape("rect", ShapeElement {
    x: 0, y: 0, width: 750, height: 60,
    color: "#FF6B6B".into(),
    filled: true,             // true=填充 false=描边
    radius: Some(8),          // 圆角半径
    opacity: Some(0.8),       // 透明度 0-1（0-100 也接受）
    ..Default::default()
});
builder.add_shape("circle", ShapeElement {
    x: 100, y: 100, width: 80, height: 80,
    color: "#4ECDC4".into(),
    ..Default::default()
});

builder.add_line(LineElement {
    x1: 75, y1: 800, x2: Some(675), y2: Some(800),
    style: poster::drivers::LineOptions { color: "#EEEEEE".into(), width: 1 },
    ..Default::default()
});

builder.add_watermark("CONFIDENTIAL", WatermarkElement {
    size: 24.0, color: "#00000020".into(),   // 半透明
    angle: 30.0,                             // 倾斜角度
    spacing: Some(200),                      // 间距
    ..Default::default()
});
```

#### 表格 / 图表 / 日历

```rust
use poster::poster::elements::{table::TableElement, chart::ChartElement, calendar::CalendarElement};
use serde_json::json;

builder.add_table(TableElement {
    x: 50, y: 800, width: 650,
    columns: vec![150, 350, 150],                        // 列宽
    header: vec![json!("序号"), json!("项目"), json!("价格")],
    rows: vec![
        vec![json!("1"), json!("商品A"), json!("¥99")],
        vec![json!("2"), json!("商品B"), json!("¥199")],
    ],
    header_bg: "#333333".into(), header_color: "#FFFFFF".into(),
    row_bg: vec!["#FFFFFF".into(), "#F5F5F5".into()],    // 斑马纹
    font_size: 24, cell_padding: 10,
    ..Default::default()
});

builder.add_chart("bar", vec![                                 // bar | line | pie
        json!({"label": "一月", "value": 120}),
        json!({"label": "二月", "value": 200}),
        json!({"label": "三月", "value": 150}),
    ], ChartElement {
    x: 50, y: 100, width: 650, height: 400,
    colors: vec!["#FF6B6B".into(), "#4ECDC4".into(), "#45B7D1".into()],
    ..Default::default()
});

builder.add_calendar(CalendarElement {
    x: 50, y: 200,
    year: Some(2026), month: Some(5),   // None = 当月
    cell_size: 60, start_day: 1,        // 1=周一 0=周日
    title: Some("2026年5月".into()),     // 默认自动生成
    highlights: [
        ("2026-05-01".to_string(), json!({"bg": "#FF6B6B", "text": "劳动节"})),
        ("2026-05-16".to_string(), json!({"bg": "#FFEAA7", "text": "今天"})),
    ].into_iter().collect(),
    ..Default::default()
});
```

#### 艺术字体 / Emoji / 图标 / 颜文字

```rust
use poster::poster::elements::{artistic_text::ArtisticTextElement, emoji::EmojiElement, icon::IconElement, emoticon::EmoticonElement};

// 描边 / 阴影 / 渐变 / 霓虹 四种风格
builder.add_artistic_text("SALE", "stroke", ArtisticTextElement {
    x: 80, y: 120, size: 72.0,
    color: "#FF6B6B".into(),                 // 填充颜色
    stroke_color: "#000000".into(), stroke_width: 3,
    ..Default::default()
});
builder.add_artistic_text("VIP", "gradient", ArtisticTextElement {
    x: 80, y: 120, size: 60.0,
    color: "#FF6B6B".into(),                 // 顶部颜色
    color2: "#FF8E53".into(),                // 底部颜色（gradient 用）
    ..Default::default()
});

builder.add_emoji("😀", EmojiElement { x: 100, y: 100, size: 64, ..Default::default() });

builder.add_icon("heart", IconElement {           // 内置 FontAwesome 图标名（需提供图标字体）
    x: 20, y: 40, size: 32,
    color: "#E74C3C".into(),
    font: Some("/path/to/fa-solid-900.ttf".into()),
    ..Default::default()
});

builder.add_emoticon("happy", EmoticonElement { x: 20, y: 40, ..Default::default() });
// happy, love, cry, angry, surprised, cool, sleepy, wave, think, shrug, tableflip, lenny
```

#### 项目宠物 `add_pet()`

内置吉祥物 Posty（`assets/pet.png`，由 `assets/pet.svg` 栅格化）可直接画进海报，等价于 `add_image` 用内置宠物：

```rust
use poster::poster::elements::image::ImageElement;

builder.add_pet(ImageElement {
    x: 555, y: 140,
    style: poster::drivers::OverlayOptions {
        width: Some(150), height: Some(130),  // 建议保持 600:520 比例
        ..Default::default()
    },
    ..Default::default()
});

// 也可取路径自行使用（例如作为二维码中心 Logo）
let logo = poster::assets::pet_path();
```

**缺图占位图**：`add_image()` / `add_avatar()` 遇到不存在的文件时默认跳过不绘制。把 `poster.placeholder` 设为 `Placeholder::Pet`，缺图位置就会画出 Posty，一眼看出哪张图漏了：

```rust
use poster::config::{Placeholder, PosterConfig};
let mut cfg = PosterConfig::default();
cfg.poster.placeholder = Some(Placeholder::Pet);
```

### 三、模板系统

```rust
use poster::PosterTemplate;
use serde_json::json;

// 定义模板（JSON 可序列化，键名与 PHP 版一致）
let template = PosterTemplate::from_config(json!({
    "width": 750,
    "height": 1334,
    "elements": [
        {"type": "shape", "shape": "rect", "color": "#FF6B6B", "x": 0, "y": 0, "width": 750, "height": 300},
        {"type": "text", "text": "{{title}}", "x": 80, "y": 100, "size": 48, "color": "#FFFFFF"},
        {"type": "text", "text": "{{subtitle}}", "x": 80, "y": 180, "size": 28, "color": "#FFE0E0"},
        {"type": "image", "src": "{{cover}}", "x": 75, "y": 350, "width": 600, "height": 600, "radius": 12},
        {"type": "qrcode", "content": "{{url}}", "x": 275, "y": 1050, "size": 200, "label": "扫码查看详情"}
    ]
}))?;

// 使用模板 + 变量渲染
builder.use_template(template)
    .with([
        ("title", "新品首发"),
        ("subtitle", "限时特惠 · 买一送一"),
        ("cover", "/path/to/product.jpg"),
        ("url", "https://m.example.com/product/123"),
    ])
    .save("/output/poster.jpg", None)?;
```

`use_template()` 默认**替换**此前的 `add_*()` 元素；要「模板打底 + 再叠手写元素」：

```rust
builder.replace_elements(false).use_template(template).with(vars);
builder.add_pet(ImageElement { x: 20, y: 20, ..Default::default() });
```

反向导出：把当前 builder 转成模板结构，可再次喂回 `from_config()`：

```rust
let config = builder.to_array();                       // {"width":…, "height":…, "elements":[…]}
let template2 = PosterTemplate::from_config(config)?;  // 导出 → 再导入，结构一致
```

## 框架集成

所有框架适配器都产出 **`Guard`** 请求守卫：接线期构造、请求期克隆（两次原子计数，`Send + Sync`）。

```rust
use std::sync::Arc;
use poster::{Guard, captcha::CaptchaManager};

let guard = Guard::from_manager(Arc::new(CaptchaManager::new()?))?;  // 接线期快速失败（存储探针）
```

### axum

```rust
use axum::Router;
use poster::{Guard, integrations::axum::{GuardState, captcha_routes}};

#[derive(Clone)]
struct AppState { captcha: Guard }

impl GuardState for AppState {
    fn guard(&self) -> &Guard { &self.captcha }
}

let app: Router = Router::new()
    .merge(captcha_routes::<AppState>())   // GET /captcha/new · POST /captcha/verify · GET /captcha/{key}
    .with_state(AppState { captcha: guard });
```

`Guard` 实现 `FromRequestParts`，handler 里可直接提取：

```rust
async fn login(guard: Guard) -> axum::Json<serde_json::Value> {
    let result = guard.create(Some("click")).unwrap().generate().unwrap();
    axum::Json(serde_json::to_value(result).unwrap())   // CaptchaResult 可直接序列化
}
```

### actix-web

```rust
use actix_web::{web, App, HttpServer};

HttpServer::new(move || {
    App::new()
        .app_data(web::Data::new(guard.clone()))
        .configure(poster::integrations::actix::configure)   // 三条路由
})
```

### Rocket / Poem / Salvo / Warp / bee-rust / e-cat

同一套 `Guard`，各自提取器（Rocket `FromRequest` / Poem `FromRequest` / Salvo `Handler` / Warp filter / bee-rust（`bee_router`，axum 兼容）/ e-cat（axum 兼容））：

```toml
poster-rust = { version = "0.1", features = ["rocket"] }   # 或 poem / salvo / warp / bee / ecat
```

每个集成都提供请求守卫提取 + 出图路由（`GET {path}/{key} → image/png`，`Cache-Control: no-store`，等价 PHP 版 `captcha.route`）。可直接运行的示例见 [`examples/`](examples/)（`axum_captcha` / `actix_captcha` / `guard_native`）。

## 配置

```rust
use poster::config::{self, PosterConfig};

let mut cfg = PosterConfig::default();
cfg.captcha.ttl_secs = 600;
cfg.captcha.rate_limit.max = 60;
config::set_global(cfg)?;   // 进程级一次
```

主要配置项（键名对齐 PHP 版 `config/poster.php`）：

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `captcha.default_type` | `random` | 默认验证码类型：`click` / `rotate` / `slider` / `random` |
| `captcha.default_difficulty` | `medium` | 默认难度：`easy` / `medium` / `hard` |
| `captcha.slider_shape` | `square` | 滑块拼图形状：`square` / `jigsaw` |
| `captcha.click_words` | `[合,家,欢,…]` | click 验证码文字池，可自定义 |
| `captcha.background_source` | `Embedded` | `Embedded`（内置 6 张）/ `Dir(path)` / `Procedural` |
| `captcha.ttl_secs` | `300` | 验证码有效期（秒） |
| `captcha.max_attempts` | `3` | 最大验证次数 |
| `captcha.tolerance` | `{click:18, rotate:5, slider:4}` | 各类型容差（像素/角度） |
| `captcha.rate_limit` | `{max:30, window_secs:60}` | 窗口限流 |
| `captcha.trajectory` | `{enabled:false, …}` | 行为轨迹校验（默认关闭） |
| `poster.default_width` / `default_height` | `750` / `1334` | 画布默认宽高 |
| `poster.placeholder` | `None` | 缺图占位：`None` 跳过 / `Pet` 画 Posty / `Path(p)` 画指定图 |
| `poster.jpeg_quality` | `90` | `save()` 默认 JPEG 质量 |
| `poster.png_compression` | `6` | PNG 压缩级别 0-9 |

## 与 PHP 版的差异

poster-rust 参照 [poster-php](https://github.com/erikwang2013/poster-php) 移植，核心能力一一对齐；以下为有意差异：

| 项 | poster-php | poster-rust |
|----|-----------|-------------|
| 图像驱动 | GD / ImageMagick 双驱动 | 单一纯 Rust 驱动（`image` crate），无系统依赖 |
| 二维码 | 自研纯 PHP 生成器 | `qrcode` crate 封装 |
| 存储 | File / Session / Redis / PSR-16 | Memory / File / Redis（Session 为 PHP 特有） |
| 框架适配 | Laravel / ThinkPHP / Webman / Hyperf / Yii2 / Yii3 | Guard + axum / actix-web / rocket / poem / salvo / warp / bee-rust / e-cat |
| 配置 | `config/poster.php` 数组 | `PosterConfig` 结构体（键名对齐，可 serde 序列化） |
| 自定义元素注册 | 运行时注册新元素类 | 内置 14 种（枚举分发），运行时注册暂不提供 |
| 多语言文档 | 14 语言 | 中文 + 英文 |

## 开源不易，欢迎支持

| 微信 | 支付宝 |
|:---:|:---:|
| <img src="./docs/weixinpay.png" width="130" height="130" alt="微信赞赏码"> | <img src="./docs/alipay.png" width="130" height="130" alt="支付宝赞赏码"> |

---

## License

MIT License — Copyright (c) 2026 erik <erik@erik.xyz> — https://erik.xyz
