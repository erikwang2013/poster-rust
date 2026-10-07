# 更新日志

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。

## [1.1.0] - 2026-10-07

### 新增
- **8 个框架端点齐平**：rocket / poem / salvo / warp 补上 `POST {path}/verify` 校验端点
  （此前只有 axum / actix / bee-rust / e-cat 有）。请求体 `{"key","answer"}` → `{"pass": bool}`，
  限流身份按请求派生，与其余框架逐字段一致。
- 新增 `tests/integrations_warp.rs`（`warp::test`）：XFF 分桶限流、错误答案、非法 JSON 400。

### 修复
- **salvo 路由表（v1.0.x 存在）**：`{prefix}/new` 原先嵌在 `{prefix}/{key}` 之下，salvo 按相对段
  拼接后实际路径成了 `/{prefix}/{key}/{prefix}/new`——`GET /captcha/new` 一直 404。改为平级兄弟路由。
- **warp 出图路由未限方法（v1.0.x 存在）**：任意方法都会被 `path::param` 吃掉；补 `warp::get()`，
  否则 `POST /captcha/verify` 的非法 JSON 会「回退」到出图路由变成 404 而非 400。
- clippy `--all-targets` 清零（examples / tests / benches 共 5 处 lint），CI 的 clippy 同步升级为
  `--all-targets`；`src/lib.rs` 文档链接 `[Guard::verify]` → `[Guard::verify_as]`。

## [1.0.1] - 2026-10-07

### 修复
- **只开单个框架 feature 时无法编译**（v1.0.0 存在）：`axum` 缺 json/query/tokio/http1 特性、
  `poem` 缺 `server` 特性。此前只有 `--all-features` 通过，是因为 `bee-rust`（bee_router）
  间接打开了 axum 的这些特性，掩盖了缺口。现在 8 个 feature 逐个单独验证全绿：
  axum / actix / rocket / poem / salvo / warp / bee-rust / ecat（+ redis）。
- **集成层限流身份**：8 个框架的校验端点此前把所有请求计入同一个常量桶，用户量一大互相误杀。
  现按请求派生（`X-Forwarded-For` 第一段 → 对端 IP → `"unknown"`，见 `guard::client_identity`），
  并新增 `Guard::verify_as` 供 session / uid 级身份使用；README 补代理场景安全提示。

### 新增
- `PosterBuilder::output_bytes()`：返回原始编码字节（直接写 HTTP 响应体），`output()` 保持 data URI。
- 性能基准套件 `benches/perf.rs`：验证码生成、750×1334 全元素海报、100 次文字的密集排版，
  输出毫秒表格；`PERF_ASSERT=1` 时对预算断言（滑块 < 200ms / 全元素海报 < 1500ms / 100 次文字 < 400ms）。
- GitHub Actions CI（`.github/workflows/ci.yml`）：rustfmt（历史代码未统一，暂 `continue-on-error`）、
  clippy `-D warnings`、三平台默认特性测试 + ubuntu 全特性测试。
- README（中 / 英）徽章：crates.io / docs.rs / License / CI。
- docs.rs 元数据 `all-features = true`：8 个框架集成与 Redis 相关 API 进入在线文档。

### 性能
- **海报渲染结果缓存**（对齐 PHP 的 `$rendered` 语义）：同一 Builder 上重复 `render()` /
  `save()` / `output()` 只渲染一次，任何元素 / 背景 / 尺寸变更使缓存失效。
  示例里的多次输出调用（save×2 + output）实测合并为 1 次渲染（release，wall ~1.8s → ~0.25s），
  输出逐字节一致。
- **字形 / 整行渲染缓存**：同字号同文本只栅格化一次（覆盖分档：0 跳过、1 直写），
  重复绘制（水印、描边艺术字）与旋转重复场景显著提速；命中路径与逐字形绘制逐字节一致（有哨兵测试）。
- **画布快路径**：`blend_pixel` 整数 alpha 判定（0 跳过 / 255 直接拷贝）+ 行切片处理，
  减少热点循环的逐像素边界检查；半透明仍为标量 f32（代码内 `ponytail:` 标注天花板）。
- 清理脚手架：字体缓存改用 `Entry` API，消除两处 `expect()` panic 分支。

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

[Unreleased]: https://github.com/erikwang2013/poster-rust/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/erikwang2013/poster-rust/releases/tag/v1.1.0
[1.0.1]: https://github.com/erikwang2013/poster-rust/releases/tag/v1.0.1
[1.0.0]: https://github.com/erikwang2013/poster-rust/releases/tag/v1.0.0
