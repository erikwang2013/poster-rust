# poster-rust 架构设计与业务逻辑图

> 所有图表使用 Mermaid 语法，GitHub / GitLab 原生渲染。
> 本文档由 PHP 版 `docs/architecture.md` 改写而来，模块与驱动替换为 Rust 侧实现。

---

## 一、系统架构总览

```mermaid
graph TB
    subgraph "API Layer 接口层"
        HELPERS["辅助函数<br/>captcha_create / captcha_verify / poster_create<br/>crate 根导出，框架无关"]
        GUARD["Guard 请求守卫<br/>原生 Rust（Arc + Clone + Send + Sync）"]
        FACADES["框架集成（feature 门控）<br/>axum / actix-web / rocket / poem / salvo / warp / bee-rust / e-cat"]
    end

    subgraph "Business Layer 业务层"
        CAPTCHA["Captcha Module 验证码模块<br/>CaptchaManager → CaptchaFactory → Click/Rotate/Slider/Random"]
        POSTER["Poster Module 海报模块<br/>PosterBuilder → 14 Elements → PosterTemplate"]
    end

    subgraph "Core Layer 核心层"
        DRIVERS["Image Driver 图像驱动<br/>ImageDriver（image crate，纯 Rust 单驱动）"]
        STORAGE["Storage 存储<br/>Storage trait<br/>MemoryStorage / FileStorage / RedisStorage"]
        QRCODE["QR Code 二维码<br/>qrcode crate 封装（原 PHP 为自研生成器）"]
        CONFIG["Config 配置<br/>PosterConfig<br/>全局 + 实例覆盖，键名对齐 PHP config/poster.php"]
    end

    subgraph "Foundation 基础层"
        RUST["Rust ≥ 1.85 (edition 2024)<br/>image / ab_glyph / imageproc / qrcode / chrono / serde"]
        OPTIONAL["Optional 可选<br/>redis / 各 Web 框架（feature 门控）"]
    end

    HELPERS --> CAPTCHA
    HELPERS --> POSTER
    GUARD --> CAPTCHA
    FACADES --> GUARD

    CAPTCHA --> DRIVERS
    CAPTCHA --> STORAGE
    CAPTCHA --> CONFIG

    POSTER --> DRIVERS
    POSTER --> QRCODE
    POSTER --> CONFIG

    DRIVERS --> RUST
    STORAGE --> RUST
    STORAGE --> OPTIONAL
    QRCODE --> RUST
    CONFIG --> RUST
```

---

## 二、分层依赖关系

```mermaid
graph LR
    subgraph "Presentation 表现层"
        A1["HTTP Handler / 业务代码"]
    end

    subgraph "API 接口"
        B1["辅助函数"]
        B2["Guard 请求守卫"]
        B3["CaptchaManager"]
        B4["PosterBuilder"]
    end

    subgraph "Domain 领域"
        C1["ClickCaptcha<br/>RotateCaptcha<br/>SliderCaptcha<br/>RandomCaptcha"]
        C2["14 Element Types"]
        C3["RateLimiter / TrajectoryVerifier"]
        C4["PosterTemplate"]
    end

    subgraph "Infrastructure 基础设施"
        D1["ImageDriver"]
        D2["MemoryStorage"]
        D3["FileStorage"]
        D4["RedisStorage"]
        D5["qrcode 封装"]
        D6["PosterConfig"]
    end

    A1 --> B1
    A1 --> B2
    B2 --> B3
    B1 --> B3
    B1 --> B4
    B3 --> C1
    B3 --> C3
    B4 --> C2
    B4 --> C4

    C1 --> D1
    C1 --> D2
    C1 --> D3
    C1 --> D4
    C2 --> D1
    C2 --> D5
    B3 --> D6
    B4 --> D6
```

---

## 三、组件关系图

```mermaid
graph TB
    CM["CaptchaManager<br/>验证码管理器（Send + Sync）"] --> RL["RateLimiter<br/>窗口限流"]
    CM --> TV["TrajectoryVerifier<br/>轨迹校验（可选）"]
    CM --> CF["CaptchaFactory<br/>验证码工厂"]
    CF --> CC["ClickCaptcha<br/>点击验证"]
    CF --> RC["RotateCaptcha<br/>旋转验证"]
    CF --> SC["SliderCaptcha<br/>滑块验证"]
    CF --> RANDOM["random → 随机选取"]

    CC --> AC["AbstractCaptcha<br/>公共生成 / 校验逻辑"]
    RC --> AC
    SC --> AC

    AC --> ID["ImageDriver<br/>画布"]
    AC --> SI["Storage trait<br/>get / set(TTL) / delete / 计数"]

    SI --> MS["MemoryStorage<br/>进程内，默认"]
    SI --> FS["FileStorage<br/>临时目录 + 原子替换"]
    SI --> RS["RedisStorage<br/>feature = redis"]

    PB["PosterBuilder<br/>海报构建器"] --> ELEMENTS["Element 枚举<br/>14 种元素"]
    PB --> PT["PosterTemplate<br/>JSON 模板 + {{变量}}"]

    ELEMENTS --> ID
    ELEMENTS --> QG["qrcode 封装"]

    GUARD["Guard<br/>请求守卫"] --> CM
```

---

## 四、验证码生成流程

```mermaid
sequenceDiagram
    participant Client as 前端/客户端
    participant Helper as captcha_create()
    participant Manager as CaptchaManager
    participant Factory as CaptchaFactory
    participant Captcha as ClickCaptcha<br/>(or Rotate/Slider)
    participant Driver as ImageDriver
    participant Storage as Storage

    Client->>Helper: captcha_create(Some("click"), opts)
    Helper->>Manager: create("click") → generate()
    Manager->>Factory: create("click")
    alt type == random
        Factory->>Factory: 随机三选一
    end
    Factory-->>Manager: Box<dyn Captcha>
    Manager->>Captcha: set_difficulty / set_background / …
    Captcha->>Captcha: 生成随机 key（hex，不可预测）
    Captcha->>Driver: 创建背景
    Note over Captcha,Driver: 三级优先级<br/>① set_background() 指定图片<br/>② BackgroundSource::Dir 目录随机<br/>③ BackgroundSource::Procedural 程序化（minimal/vibrant/natural）<br/>默认 BackgroundSource::Embedded = 内置 6 张
    Driver-->>Captcha: 背景画布
    Captcha->>Captcha: 画目标 / 缺口 / 旋转
    Captcha->>Storage: set(key, 答案载荷, TTL)
    Captcha->>Storage: set(key:img, PNG 字节, TTL)
    Captcha-->>Helper: CaptchaResult { key, image(data URI), captcha_type, extra }
    Helper-->>Client: CaptchaResult（可直接 serde 序列化）
```

### 4.1 凹凸拼图轮廓（SliderCaptcha jigsaw）

`slider_shape = "jigsaw"` 时，缺口与拼图块共用同一套多边形轮廓：

- **轮廓生成** — 4 段边线 + 4 个半径 `k = 短边 / 5` 的半圆（圆心在各边中点，12 段采样），凸/凹逐边随机，共 16 种组合
- **两个画布原语** — `polygon()`（任意多边形填充，自动闭合）与 `mask()`（掩膜裁剪：掩膜透明度过半处目标像素置空）
- **像素级对齐不变量** — 缺口（多边形填充）与拼图块（掩膜裁剪）覆盖范围逐像素相等（有单测钉死）；拼图块 PNG 是外扩 `k` 后的外接矩形，服务端答案 x/y = PNG 左上角
- **边距保证** — 凸出不贴边、不越界（最小画布检查保证）

---

## 五、验证码验证流程

```mermaid
sequenceDiagram
    participant Client as 前端
    participant Manager as CaptchaManager
    participant Limiter as RateLimiter
    participant Storage as Storage

    Client->>Manager: verify(key, Answer)
    Note over Manager,Limiter: ① 跨 key 窗口限流（单 key 计数挡不住「每次换新 key 再猜一次」）<br/>默认 60s / 30 次，identity 由调用方提供（IP / 会话 / 账号）
    Manager->>Limiter: allow(identity)
    alt 窗口内超限
        Limiter-->>Manager: false
        Manager-->>Client: Ok(false)（不抛异常，避免暴露限流状态）
    end

    Manager->>Storage: get(key)
    alt key 不存在 / 已过期
        Storage-->>Manager: None
        Manager-->>Client: Ok(false)
    end
    Manager->>Storage: 原子自增尝试次数 → n
    alt n > max_attempts（默认 3）
        Manager->>Storage: delete(key)
        Manager-->>Client: Ok(false)
    end

    Manager->>Manager: 按存储的类型校验（click 逐点距离 / rotate 角度折算 / slider 像素）

    opt captcha.trajectory.enabled
        Manager->>Manager: TrajectoryVerifier::pass(trail)
        Note over Manager: 点数 ≥ min_points、耗时在窗口内、<br/>轨迹线性度 ≤ max_linearity（默认关闭，避免误伤触屏/无障碍）
    end

    alt 通过
        Manager->>Storage: delete(key)（一次性）
        Manager-->>Client: Ok(true)
    else 不通过
        Manager-->>Client: Ok(false)（key 保留，可重试至 max_attempts）
    end
```

---

## 六、海报生成流程

```mermaid
sequenceDiagram
    participant Client as 调用方
    participant Builder as PosterBuilder
    participant Driver as ImageDriver
    participant Element as Element 枚举
    participant QR as qrcode 封装

    Client->>Builder: PosterBuilder::new()（默认 750×1334 取配置）
    Client->>Builder: background("#FFFFFF") / background_gradient(..) / background("photo.jpg")
    Client->>Builder: add_text / add_image / add_qrcode / …（14 种）
    Builder->>Builder: elements.push(Element::…)（可序列怀的选项结构体）

    Client->>Builder: save(path, quality) / output(format, quality)

    Builder->>Builder: render()
    Note over Builder: 1. 应用模板（如有）：{{变量}} 替换 → Element<br/>2. 确定最终宽高<br/>3. 创建画布 + 背景

    loop 每个元素
        Builder->>Element: render(canvas, ctx)
        alt TextElement
            Element->>Driver: text(基线锚点 + TextOptions)
        else ImageElement
            Element->>Driver: load → overlay(缩放/圆角/阴影)
        else QrcodeElement
            Element->>QR: render(content, level, size, logo, label)
        else ChartElement
            Element->>Driver: rectangle / line / ellipse / polygon
        else CalendarElement
            Element->>Driver: rectangle + text（chrono 月历）
        else ArtisticTextElement
            Element->>Driver: text() 偏移描边 / 渐变蒙层着色 / 霓虹辉光
        end
    end

    Builder->>Driver: save / output（jpg / png / webp / gif）
```

---

## 七、请求守卫与框架集成

```mermaid
graph LR
    subgraph "接线期（快速失败）"
        STATE["应用状态<br/>Arc&lt;CaptchaManager&gt;"]
        G["Guard::from_manager(state)<br/>→ Result&lt;Guard&gt;"]
    end

    subgraph "八框架提取器（全部产出 Guard）"
        AX["axum: FromRequestParts"]
        AC["actix-web: FromRequest"]
        RK["rocket: FromRequest"]
        PM["poem: FromRequest"]
        SV["salvo: Extractible"]
        WP["warp: filter + with"]
        BE["bee-rust: axum 兼容（bee_router）"]
        EC["e-cat: axum 兼容"]
    end

    subgraph "请求期（不可失败）"
        H1["create() → CaptchaResult"]
        H2["verify(key, Answer) → bool"]
        H3["image(key) → image/png 响应"]
    end

    STATE --> G
    G --> AX
    G --> AC
    G --> RK
    G --> PM
    G --> SV
    G --> WP
    G --> BE
    G --> EC

    AX --> H1
    AX --> H2
    AX --> H3
    AC --> H1
    AC --> H2
    AC --> H3
```

---

## 八、安全模型

```mermaid
stateDiagram-v2
    [*] --> Generated: 生成
    Generated --> Active: 存储答案 + 类型 + attempts=0（TTL 300s）
    Active --> Expired: 超过 TTL
    Active --> VerifyAttempt: 用户提交

    VerifyAttempt --> Deleted: 尝试次数 > max_attempts(3)
    VerifyAttempt --> CheckAnswer: 尝试次数 <= 3

    CheckAnswer --> Success: 在容差内
    CheckAnswer --> Retry: 超出容差（key 保留）
    Retry --> Active

    Success --> Deleted: 一次性删除
    Expired --> Deleted
    Deleted --> [*]
```

安全特性：一次性 key、防暴力（3 次）、有效期（300s）、跨 key 窗口限流（60s/30 次）、可选轨迹校验、随机背景与目标位置（防 OCR/枚举）、原子尝试计数（防并发绕过）。

---

## 九、目录结构映射

```mermaid
graph LR
    ROOT["poster-rust/"] --> SRC["src/"]
    ROOT --> ASSETS["assets/"]
    ROOT --> TESTS["tests/"]
    ROOT --> EXAMPLES["examples/"]
    ROOT --> DOCS["docs/"]

    SRC --> LIB["lib.rs<br/>辅助函数 + 导出"]
    SRC --> GUARD_F["guard.rs<br/>原生请求守卫"]
    SRC --> CAPTCHA_DIR["captcha/<br/>4 种验证码 + 工厂 + 管理器 + 限流 + 轨迹"]
    SRC --> POSTER_DIR["poster/<br/>Builder + Template + 14 元素"]
    SRC --> DRIVERS_DIR["drivers/<br/>画布 + 文字 + 颜色"]
    SRC --> STORAGE_DIR["storage/<br/>memory / file / redis"]
    SRC --> INTEG_DIR["integrations/<br/>8 框架（feature 门控）"]
    SRC --> QR["qrcode.rs<br/>二维码封装"]

    ASSETS --> ASSETS_F["pet.svg / pet.png<br/>backgrounds/*.png<br/>fonts/Alibaba-PuHuiTi-Regular.ttf"]

    TESTS --> T_DIRS["captcha / poster / drivers / storage / qrcode / integrations"]
```

---

> 以上图表可在支持 Mermaid 的 Markdown 渲染器中直接查看（GitHub / GitLab / VS Code / Typora）。
