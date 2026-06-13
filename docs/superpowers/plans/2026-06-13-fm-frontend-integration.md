# Plan: field-mem-core 接口审计 + 前端接入

## 目标

field-mem-core 引擎有多个内部状态（anchors, events, traces, seeds, ECG）和操作（save/load/init/value_init），但当前前端只显示一个聊天窗口，完全不暴露这些数据。本计划补齐这个缺口。

**API 契约（两端共享）：**
```
GET /api/memory/status
→ {
    anchors: [{ label, density, stiffness, damping }],
    anchors_count: usize,
    events_count: usize,
    seeds_count: usize,
    ecg: { field_tension: f32, convergence_rate: f32, anisotropy_magnitude: f32 } | null
  }

POST /api/memory/save
→ { ok: true } | { error: "..." }

POST /api/memory/load
→ anchors_count, events_count, seeds_count

POST /api/memory/init
  body: { concepts: [{ label: string, density: number }] }
→ { anchors_count: usize }
```

## 任务划分

| 任务 | 涉及文件（绝对路径） | 性质 |
|---|---|---|
| **Task 1: 后端路由** | `crates/ext-server/src/memory_routes.rs`（新）<br>`crates/ext-server/src/main.rs`（改） | 写代码 |
| **Task 2: 前端面板** | `crates/ext-server/src/static/index.html`（改） | 写代码 |

两个任务不共享文件，适合并行。

---

### Task 1: 后端 — 暴露引擎状态路由

**目标：** 在 ext-server 中添加 `/api/memory/*` 路由组，将 DseEngine 的内部状态（anchors、events、ECG、persistence）暴露为 JSON API。

**产出格式：**
- 新文件 `crates/ext-server/src/memory_routes.rs`（~80 行）
- `main.rs` 中注册新路由、引入新模块

**范围边界：**
- **只可修改** `crates/ext-server/src/main.rs`（追加 3 行：mod 声明 + 两条路由调用）
- **只可创建** `crates/ext-server/src/memory_routes.rs`（新文件）
- **不可以碰** `crates/ext-server/src/routes.rs`、`crates/ext-server/src/llm.rs` 或任何 `crates/core/` 下的文件

**步骤：**

1. 创建 `crates/ext-server/src/memory_routes.rs`：
   - `use axum::{Json, extract::State, response::Json};`
   - `use std::sync::Arc;`
   - `use serde::Serialize;`
   - 使用与 `routes.rs` 相同的 `AppState` 引用方式：`State(state): State<Arc<AppState>>`
   - 定义以下响应结构体并用 `#[derive(Serialize)]`：

   ```
   pub struct AnchorBrief { label: String, density: u32, stiffness: f32, damping: f32 }
   pub struct MemoryStatus {
       anchors: Vec<AnchorBrief>,
       anchors_count: usize,
       events_count: usize,
       seeds_count: usize,
       ecg: Option<EcgBrief>,
   }
   pub struct EcgBrief { field_tension: f32, convergence_rate: f32, anisotropy_magnitude: f32 }
   pub struct SaveResponse { ok: bool }
   pub struct InitResponse { anchors_count: usize }
   ```

   - 路由函数命名和签名：
     - `pub async fn status(state) -> Json<MemoryStatus>`
     - `pub async fn save(state) -> Json<SaveResponse>` — 保存到 `"./memory_state"` sled 路径
     - `pub async fn load(state) -> Json<Value>`
     - `pub async fn init(state, Json(body): Json<InitRequest>) -> Json<InitResponse>` — 从请求添加 concept

   - 每个函数内部：
     ```
     let engine = state.engine.lock().unwrap();
     // ... 调用 engine.xxx() ...
     ```

2. 修改 `crates/ext-server/src/main.rs`：
   - 添加 `mod memory_routes;`
   - 在 `Router::new()` 链中添加：
     ```
     .route("/api/memory/status", axum::routing::get(memory_routes::status))
     .route("/api/memory/save", axum::routing::post(memory_routes::save))
     .route("/api/memory/load", axum::routing::post(memory_routes::load))
     .route("/api/memory/init", axum::routing::post(memory_routes::init))
     ```

3. 验证编译：`cargo check -p dse-server`

---

### Task 2: 前端 — 记忆仪表盘面板

**目标：** 在当前聊天 UI 侧边栏中添加记忆状态面板，展示 anchors、events、ECG 等数据。不改变聊天的核心功能。

**产出格式：**
- 修改后的 `crates/ext-server/src/static/index.html`

**范围边界：**
- **只可修改** `crates/ext-server/src/static/index.html`（追加 CSS + JS + DOM 元素）
- **不可以碰** `routes.rs`、`main.rs`、`llm.rs` 或 core 代码
- 后端 `/api/memory/*` 由 Task 1 实现；如果尚未就绪，前端优雅处理（隐藏或显示加载失败）

**步骤：**

1. 在现有 `<header>` 之后、`<div id="messages">` 之前，添加一个记忆状态侧面板 `<div id="memory-panel">`
2. 侧面板包含这些区域（全部用纯 JS 动态填充）：
   - **ECG 状态**：field_tension、convergence_rate、anisotropy_magnitude 数值标签
   - **Anchors 统计**：anchors_count、events_count、seeds_count
   - **Anchors 列表**（不超过 15 条）：label + density 柱状条
   - **操作按钮**：Save / Load / Init（Init 弹 prompt 输入 concept:密度）

3. CSS 要求：
   - 侧面板固定在右侧，宽度 280px，浅灰背景
   - 主内容区（#messages + #input-area）左移留出侧面板空间
   - 响应式：窄屏时侧面板折叠（display:none）
   - 风格与现有 UI 一致

4. JS 逻辑：
   - 页面加载后立即 fetch `/api/memory/status`
   - 每个新消息发送后，再次 fetch `/api/memory/status` 刷新面板
   - 如果 fetch 失败（例如后端还未部署该接口），面板显示"Loading..."然后几秒后隐藏——不阻塞聊天功能
   - Save / Load / Init 按钮调用对应的 POST 端点，然后刷新面板

5. 验收标准：
   - 页面加载后侧面板可见（或网络请求失败时优雅降级）
   - 聊天功能完全不受影响
   - 发送消息后面板自动刷新（anchors 密度可能变化）

