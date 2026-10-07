//! 性能基准：验证码生成 + 海报渲染（`harness = false`，零新依赖，`std::time` 计时）。
//!
//! ```bash
//! cargo bench --offline -j 2                # 打印毫秒表格
//! PERF_ASSERT=1 cargo bench --offline -j 2  # 额外对预算做断言，超标退出码 1
//! ```
//!
//! 场景（release 构建下运行）：
//! - `captcha slider` / `captcha click`：各生成 20 次取均值
//! - `poster 14 elements`：750×1334 全元素海报（14 种元素各一次）
//! - `poster 100 texts`：100 次 `add_text` 的文字密集场景
//!
//! 预算阈值取当前实现实测值的数倍，只用来拦「数量级退化」，不做微优化门禁。

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use poster::PosterConfig;
use poster::assets;
use poster::captcha::CaptchaManager;
use poster::drivers::{LineOptions, OverlayOptions, TextAlign, TextOptions};
use poster::poster::PosterBuilder;
use poster::poster::elements::{
    artistic_text::ArtisticTextElement, avatar::AvatarElement, calendar::CalendarElement,
    chart::ChartElement, emoji::EmojiElement, emoticon::EmoticonElement, image::ImageElement,
    line::LineElement, qrcode::QrcodeElement, shape::ShapeElement, table::TableElement,
    text::TextElement, watermark::WatermarkElement,
};
use poster::poster::Direction;
use poster::storage::MemoryStorage;
use serde_json::json;

/// 单场景统计（毫秒）。
struct Stats {
    name: &'static str,
    mean: f64,
    min: f64,
    max: f64,
    /// 预算（毫秒）；`PERF_ASSERT=1` 时对均值断言。
    budget_ms: f64,
}

/// 计时 `iters` 次；先预热一次，把字体 / 背景的一次性加载排除在样本外。
fn bench(name: &'static str, iters: usize, budget_ms: f64, mut f: impl FnMut()) -> Stats {
    f();
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        f();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    Stats {
        name,
        mean: samples.iter().sum::<f64>() / iters as f64,
        min: samples.iter().copied().fold(f64::INFINITY, f64::min),
        max: samples.iter().copied().fold(0.0, f64::max),
        budget_ms,
    }
}

fn main() {
    let stats = vec![
        bench("captcha slider (n=20)", 20, 200.0, || {
            let manager = captcha_manager();
            black_box(manager.create(Some("slider")).unwrap().generate().unwrap());
        }),
        bench("captcha click (n=20)", 20, 200.0, || {
            let manager = captcha_manager();
            black_box(manager.create(Some("click")).unwrap().generate().unwrap());
        }),
        bench("poster 14 elements (n=5)", 5, 1500.0, || {
            black_box(full_poster().unwrap());
        }),
        bench("poster 100 texts (n=5)", 5, 400.0, || {
            black_box(text_dense().unwrap());
        }),
    ];

    print_table(&stats);

    if std::env::var("PERF_ASSERT").is_ok_and(|value| value == "1") {
        assert_budgets(&stats);
    }
}

fn print_table(stats: &[Stats]) {
    println!("\n{:<26} {:>10} {:>10} {:>10}", "name (ms)", "mean", "min", "max");
    println!("{}", "-".repeat(60));
    for stat in stats {
        println!(
            "{:<26} {:>10.2} {:>10.2} {:>10.2}",
            stat.name, stat.mean, stat.min, stat.max
        );
    }
    println!();
}

fn assert_budgets(stats: &[Stats]) {
    let mut failed = false;
    for stat in stats {
        let ok = stat.mean <= stat.budget_ms;
        println!(
            "[{}] {}: mean {:.2}ms / 预算 {}ms",
            if ok { "PASS" } else { "FAIL" },
            stat.name,
            stat.mean,
            stat.budget_ms
        );
        failed |= !ok;
    }
    if failed {
        std::process::exit(1);
    }
}

/// 内存存储 + 默认配置：生成过程不落盘，样本只反映计算开销。
fn captcha_manager() -> CaptchaManager {
    CaptchaManager::with_config_and_storage(
        Arc::new(PosterConfig::default()),
        Arc::new(MemoryStorage::new()),
    )
}

/// 750×1334 全元素海报：14 种元素各画一次（poster_showcase 的紧凑版，不依赖 examples/）。
fn full_poster() -> poster::Result<()> {
    let pet = assets::pet_path().to_string_lossy().into_owned();

    let mut b = PosterBuilder::new()?;
    b.width(750).height(1334);
    b.background_gradient("#FFF6F0", "#FFE3D5", Direction::Vertical);

    b.add_shape(
        "rect",
        ShapeElement {
            x: 0,
            y: 0,
            width: 750,
            height: 180,
            color: "#FF6B6B".into(),
            filled: true,
            ..Default::default()
        },
    );
    b.add_artistic_text(
        "海报生成器",
        "stroke",
        ArtisticTextElement {
            x: 60,
            y: 128,
            size: 60.0,
            color: "#FFFFFF".into(),
            stroke_color: "#C2402F".into(),
            stroke_width: 2,
            ..Default::default()
        },
    );
    b.add_text(
        "纯 Rust · 14 种元素 · 一行链式调用",
        TextElement {
            x: 375,
            y: 238,
            style: TextOptions {
                size: 26.0,
                color: "#8A6A5B".into(),
                align: TextAlign::Center,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    b.add_avatar(
        pet.clone(),
        AvatarElement {
            x: 70,
            y: 300,
            size: 120,
            circle: true,
            border: Some("#FF6B6B".into()),
            border_width: 4,
            ..Default::default()
        },
    );
    b.add_image(
        pet.clone(),
        ImageElement {
            x: 560,
            y: 420,
            style: OverlayOptions {
                width: Some(120),
                height: Some(120),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    b.add_line(LineElement {
        x1: 60,
        y1: 520,
        x2: Some(690),
        y2: Some(520),
        x: None,
        y: None,
        style: LineOptions {
            color: "#FFD9C7".into(),
            width: 3,
        },
    });
    b.add_table(TableElement {
        x: 60,
        y: 600,
        header: vec![json!("渠道"), json!("新增"), json!("占比")],
        rows: vec![
            vec![json!("小程序"), json!(1280), json!("42%")],
            vec![json!("App"), json!(960), json!("31%")],
        ],
        width: 630,
        header_bg: "#FF6B6B".into(),
        header_color: "#FFFFFF".into(),
        row_bg: vec!["#FFF8F4".into(), "#FFFFFF".into()],
        ..Default::default()
    });
    b.add_chart(
        "bar",
        vec![
            json!({"label": "一月", "value": 120}),
            json!({"label": "二月", "value": 200}),
            json!({"label": "三月", "value": 160}),
            json!({"label": "四月", "value": 260}),
        ],
        ChartElement {
            x: 50,
            y: 780,
            width: 650,
            height: 200,
            ..Default::default()
        },
    );
    b.add_calendar(CalendarElement {
        year: Some(2026),
        month: Some(10),
        x: 30,
        y: 1000,
        cell_size: 42,
        ..Default::default()
    });
    b.add_pet(ImageElement {
        x: 572,
        y: 1226,
        style: OverlayOptions {
            width: Some(56),
            height: Some(48),
            ..Default::default()
        },
        ..Default::default()
    });
    b.add_emoticon(
        "happy",
        EmoticonElement {
            x: 340,
            y: 1262,
            size: 18,
            color: "#8A6A5B".into(),
            ..Default::default()
        },
    );
    b.add_emoji(
        "A",
        EmojiElement {
            x: 520,
            y: 1262,
            size: 24,
            ..Default::default()
        },
    );
    b.add_qrcode(
        "https://erik.xyz/poster-rust",
        QrcodeElement {
            x: 610,
            y: 1010,
            size: 120,
            label: Some("扫码看源码".into()),
            ..Default::default()
        },
    );
    b.add_watermark(
        "POSTY",
        WatermarkElement {
            size: 16.0,
            color: "#FF000010".into(),
            angle: -30.0,
            spacing_x: Some(180),
            spacing_y: Some(140),
            ..Default::default()
        },
    );

    black_box(b.render()?);
    Ok(())
}

/// 文字密集：100 次 `add_text` 后渲染一次。
fn text_dense() -> poster::Result<()> {
    let mut b = PosterBuilder::new()?;
    b.width(750).height(1334);
    b.background("#FFFFFF");
    for i in 0..100 {
        b.add_text(
            format!("第 {i} 行：纯文字密集渲染场景，测字形光栅化与排版开销"),
            TextElement {
                x: 40,
                y: 30 + i * 12,
                style: TextOptions {
                    size: 11.0,
                    color: "#333333".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
    }
    black_box(b.render()?);
    Ok(())
}
