# 更新日志

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。

## [2.0.0] - 2026-10-11

### 安全
- 限流记录与验证码载荷彻底隔离：校验入口强校验 key 形态（32 位 hex），载荷键加 `captcha:` 命名空间——
  修复「把限流键当验证码 key 提交可自删限流记录」的跨 key 限流绕过。
- 限流身份哈希改用进程级随机种子（`RandomState`），不再可离线复算。
- `X-Forwarded-For` 默认不再采信（只信对端 IP）；确需按真实客户端分桶的部署显式开启
  `Guard::with_trust_forwarded_for(true)`（仅限覆写式可信代理）。
- warp / poem 的 verify 请求体补 2 MiB 上限（超限 413），与其他框架对齐。

### 修复
- 限流原子化：窗口序号并入计数键，新增 `Storage::increment_or_init`，消除窗口边界的并发突刺超发。
- 验证码一次性消费改为原子抢删（`Storage::delete` 返回 `Result<bool>`）：并发下恰好一个请求兑换成功。
- 存储回收与预算：Memory / File 摊还清扫过期条目；Memory 64 MiB、File 65536 文件预算（超限失败关闭）。
- 旋转文本临时画布像素预算 + 字形覆盖事件上限（超大模板参数返回错误而非 OOM / panic）。
- 艺术字描边宽度上限；元素几何护栏（table / chart / avatar / calendar 越界参数报错，不再溢出回绕）。
- `FileStorage::set` 持锁，与 `increment` 的读改写串行（消除丢失更新）。
- Redis 存储：读写超时 5s + 出错丢弃连接、下次惰性重建（后端抖动不再挂死整条链路）。
- warp / rocket 端点契约：`?type=` 与其余框架一致、warp 生成失败回 500、出图补 `Content-Type`。
- bee-rust：`register()` 补 verify 路由；`router(guard)` 不再丢弃参数（签名改为 `-> axum::Router`）。
- 换行支持假名 / 谚文等 CJK 扩展区（单 token 超宽逐字断行，不再整段溢出）。
- 模板 snake_case 键（`max_width` / `line_height` 等）不再被静默忽略（补 serde alias）。
- 修复 3 个模块 doctest 编译错误（全特性 CI 此前连红 8 次）。
- 8 个框架适配器：生成 / 校验 / 出图的阻塞调用移入阻塞池（`spawn_blocking` / `web::block`）。
- `PosterConfig::validate()`：`set_global` 校验非法配置（0 `ttl_secs` / 0 `max_attempts` / 越界质量等）并报错。

### 性能
- 字体加载与整行渲染缓存改为进程级（跨请求复用；键含文件 mtime，换字体自动失效）。

### 变更
- 公共枚举加 `#[non_exhaustive]`（`PosterError` / `Answer` / `CaptchaType` / `Difficulty` / `SliderShape` /
  `TargetType` / `TextAlign` / `EcLevel` / `Direction` / `Element` / `BackgroundSource` / `Placeholder`）——
  下游穷举 `match` 需保留通配臂（API 硬化）。

### 文档
- 全文档去 PHP 化（源码注释 / 测试名 / README 中英 / 架构文档 / 图表脚本），技术事实与数值原样保留；README 数字实测修正。
- CI 新增 9 个单 feature 编译矩阵；rustfmt 全仓落地，fmt 门禁转正。

## [1.1.2] - 2026-10-07

### 文档
- README（中 / 英）移除实现差异对照章节。
- 项目宠物下方新增语言标签：Rust 1.85+ · edition 2024（项目语言）与文档语言互链
  （shields.io 徽章，三个链接已验证均返回 200）。

## [1.1.1] - 2026-10-07

### 测试
- rocket / poem / salvo 的 `POST {path}/verify` 补运行时 e2e（框架自带测试客户端：
  `rocket::local::blocking::Client` / `poem::test::TestClient` / `salvo::test::TestClient`），
  覆盖请求守卫取值、提取顺序、非法 JSON 的 4xx 渲染与 XFF 分桶限流；
  另加 salvo `GET /captcha/new` 可达性回归（钉住 1.1.0 的子路由拼接修复）。
  v1.1.0 时这三个框架只有编译验证，此套补齐运行时覆盖。
  已知覆盖边界：poem / salvo 的测试客户端无法设置对端地址，「XFF 缺失 → 对端 IP」分支
  只有 rocket 跑到端到端；`Guard` 未托管 / `verify_as` 出错（401/500）在测试环境无法触发。

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
- **海报渲染结果缓存**：同一 Builder 上重复 `render()` / `save()` / `output()` 只渲染一次，
  任何元素 / 背景 / 尺寸变更使缓存失效。
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
  模板键名逐字固定，导出与导入可互相读写。
- **请求守卫**：原生 `Guard`（框架无关，接线期构造、请求期克隆）+ axum / actix-web /
  rocket / poem / salvo / warp / bee-rust / e-cat 八个框架集成，均以可选 feature 提供。
- **存储后端**：Memory / File（原子写）/ Redis，`Storage` trait 可替换。
- 辅助函数 `captcha_create` / `captcha_verify` / `poster_create`。
- 内置素材：项目宠物 Posty（PNG / SVG，内嵌）、6 张验证码背景（内嵌）、
  默认中文字体阿里巴巴普惠体。
- 中英双语文档 + 架构 / 功能设计图。

[2.0.0]: https://github.com/erikwang2013/poster-rust/releases/tag/v2.0.0
[1.1.2]: https://github.com/erikwang2013/poster-rust/releases/tag/v1.1.2
[1.1.1]: https://github.com/erikwang2013/poster-rust/releases/tag/v1.1.1
[1.1.0]: https://github.com/erikwang2013/poster-rust/releases/tag/v1.1.0
[1.0.1]: https://github.com/erikwang2013/poster-rust/releases/tag/v1.0.1
[1.0.0]: https://github.com/erikwang2013/poster-rust/releases/tag/v1.0.0
