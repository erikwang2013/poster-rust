# 更新日志

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### 新增
- 性能基准套件 `benches/perf.rs`：验证码生成、750×1334 全元素海报、100 次文字的密集排版，
  输出毫秒表格；`PERF_ASSERT=1` 时对预算断言（滑块 < 200ms / 全元素海报 < 1500ms / 100 次文字 < 400ms）。
- GitHub Actions CI（`.github/workflows/ci.yml`）：rustfmt、clippy `-D warnings`、
  三平台默认特性测试 + ubuntu 全特性测试。
- README（中 / 英）徽章：crates.io / docs.rs / License / CI。
- docs.rs 元数据 `all-features = true`：8 个框架集成与 Redis 相关 API 进入在线文档。

### 性能
- 海报渲染结果缓存：同一 Builder 多次 `render()` 复用已渲染画布。
- 字形栅格化缓存：同字号同字符只栅格化一次，文字密集场景显著减少重复开销。
- 画布快路径：纯色 / 渐变背景走整块填充，不再逐像素兜底。
- 验证码限流按身份区分（IP / uid）：多用户服务不再相互挤占全局限额。

## [1.0.0] - 2026-10-07

### 新增
- 首次发布。
- **验证码**：点击（文字 / 矢量图标目标）、旋转、滑块（方形 / 拼图块）三种方式 + 随机切换；
  难度分级、自定义背景与文字池，纯 Rust 生成图片与答案，不依赖第三方服务。
- **海报生成**：链式 Builder API，14 种元素（文字、艺术字、图片、头像、二维码、形状、
  线条、水印、表格、图表、日历、Emoji、颜文字、图标）与 JSON 模板系统，
  模板键名与 PHP 版逐字一致、可互相读写。
- **请求守卫**：原生 `Guard`（框架无关，接线期构造、请求期克隆）+ axum / actix-web /
  rocket / poem / salvo / warp / bee-rust / e-cat 八个框架集成，均以可选 feature 提供。
- **存储后端**：Memory / File（原子写）/ Redis，`Storage` trait 可替换。
- 辅助函数 `captcha_create` / `captcha_verify` / `poster_create`，对应 PHP 版同名函数。
- 内置素材：项目宠物 Posty（PNG / SVG，内嵌）、6 张验证码背景（内嵌）、
  默认中文字体阿里巴巴普惠体。
- 中英双语文档 + 架构 / 功能设计图。

[Unreleased]: https://github.com/erikwang2013/poster-rust/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/erikwang2013/poster-rust/releases/tag/v1.0.0
