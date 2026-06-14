# Plan: 会话持久化稳定性修复（真理源迁服务端）

## 目标

**只要连上服务端，会话永不丢失。**

不依赖浏览器 tab 是否关闭、是否刷新、是否切走、是否被浏览器回收内存。无论从哪个设备、哪个浏览器访问，看到的都是同一份真相。

## 当前根因（5 个叠加脆弱点）

| # | 现象 | 机制 |
|---|---|---|
| 1 | 切到 `/field` 切回找不到 | `var sessions = []` 在 JS 内存里，前端持有真理源 |
| 2 | 流式输出最后一笔丢失 | `syncSessionsToServer()` 是 fire-and-forget POST，切走时还没 flush |
| 3 | 多设备同步互相覆盖 | 每次全量 POST 整个 sessions 数组，"最后写入赢" |
| 4 | 文件半截写 | `std::fs::write` 直接覆盖，写入中途崩溃会留损坏 JSON |
| 5 | 切回要等 ≤3s | `pollAndMergeSessions` 3s 间隔，visibility 变化时也不强制刷 |

记忆库之所以"还在"，是因为它的真理源一开始就在服务端（`Arc<Mutex<DseEngine>>` + sled 磁盘），浏览器死活根本不参与。

## 设计原则

1. **服务端是会话的唯一真理源** —— 浏览器只是 view
2. **每次 mutation 立即落盘** —— 流式输出过程中切走也不丢
3. **写入原子化** —— 用 tmp + rename，不留半截文件
4. **删除前端 sessions 数组** —— 不存在"前端 hold 真理"的概念
5. **复用现有抽象** —— `Arc<Mutex<...>>` 模式和 engine 完全对齐

## 新 API 契约（前后端共享）

### 服务端真理源

```rust
pub struct AppState {
    pub engine: Arc<Mutex<DseEngine>>,
    pub libraries: Arc<Mutex<HashMap<String, (usize, usize)>>>,
    pub active_library: Arc<Mutex<String>>,
    pub sessions: Arc<Mutex<SessionsData>>,   // ← 新增
}
```

启动时 `load_sessions_from_disk()` → 一次性灌入 `Arc<Mutex>`。后续 GET 直接读内存快照，不再每次读盘。

### 路由

```
GET    /api/sessions                       → { ok, sessions, active_id }
POST   /api/sessions                       body: { title? }         → 创建新会话，返回新 session 对象
PATCH  /api/sessions/:id                   body: { title?, active? } → 改名 / 切 active
DELETE /api/sessions/:id                                              → 删除
POST   /api/sessions/:id/messages          body: { role, content, memoryCtx? } → 追加一条
PATCH  /api/sessions/:id/messages/:idx     body: { content, memoryCtx? } → 流式结束时一次性更新
```

**所有 mutation 路径都走同一个 helper：**

```rust
fn mutate_sessions<F, R>(state: &AppState, f: F) -> Result<R, String>
where F: FnOnce(&mut SessionsData) -> R {
    let mut data = state.sessions.lock().unwrap();
    let result = f(&mut data);
    save_sessions_to_disk(&data)?;       // 原子写
    Ok(result)
}
```

这样所有 mutation 都强制 atomic 落盘，不会漏。

### Atomic 写入

```rust
pub fn save_sessions_to_disk(data: &SessionsData) -> Result<(), String> {
    let path = data_path();
    let tmp = path.with_extension("json.tmp");
    let s = serde_json::to_string_pretty(data)?;
    {
        let mut f = std::fs::File::create(&tmp).map_err(...)?;
        std::io::Write::write_all(&mut f, s.as_bytes()).map_err(...)?;
        f.sync_all().map_err(...)?;       // 落盘
    }
    std::fs::rename(&tmp, &path).map_err(...)?;   // POSIX 原子替换
    Ok(())
}
```

`std::fs::rename` 在同一文件系统上是原子的。要么旧文件完整，要么新文件完整，永远不会有半截文件。

### active_id 处理

**简化掉**：服务端 `SessionsData.active_id` 字段保留（兼容旧文件），但**前端不再依赖**。每个设备的"当前查看"独立：
- 启动时默认第一个 session（按创建时间倒序）
- 用户点切换 → `PATCH /api/sessions/:id { active: true }`
- 多设备打开：各自独立（如果用户希望强同步，可以加 cookie，但目前不需要）

这其实是更合理的多设备语义——手机端打开不应该被桌面端的 active 强行切走。

## 前端改造（核心）

### 删除

```js
var sessions = [];                              // 删
var activeSessionId = null;                     // 删
function fetchSessionsFromServer() { ... }      // 改成新的 refreshSessions()
function syncSessionsToServer() { ... }         // 删（拆成多个细粒度操作）
function pollAndMergeSessions() { ... }         // 删
function startSessionPolling() { ... }          // 删
function stopSessionPolling() { ... }           // 删
function manualSessionSync() { ... }            // 简化
```

### 替换

```js
var sessionsView = [];                          // 当前 view 的 cache，乐观更新用
var activeSessionId = localStorage.getItem('fm-active-id');  // 跨刷新记忆

async function refreshSessions() {
  var r = await apiCall('/api/sessions', { method: 'GET' });
  if (r.ok) {
    sessionsView = r.data.sessions;
    // ...render
  }
}

async function createSession() {
  var r = await apiCall('/api/sessions', { method: 'POST', body: { title: '新会话' } });
  if (r.ok) {
    activeSessionId = r.data.session.id;
    localStorage.setItem('fm-active-id', activeSessionId);
    await refreshSessions();
  }
}

async function switchSession(id) {
  activeSessionId = id;
  localStorage.setItem('fm-active-id', id);
  await apiCall('/api/sessions/' + id, { method: 'PATCH', body: { active: true } });
  renderMessages();
}

async function deleteSession(id) {
  await apiCall('/api/sessions/' + id, { method: 'DELETE' });
  if (activeSessionId === id) activeSessionId = null;
  await refreshSessions();
}
```

### 流式输出原子化（关键修复）

```js
// send() 函数中：
async function send() {
  // ...省略前置
  
  // 1. 立即 POST user message（不依赖流是否成功）
  await apiCall('/api/sessions/' + sid + '/messages', {
    method: 'POST',
    body: { role: 'user', content: text }
  });
  
  // 2. 立即 POST assistant 占位（content="", content 待流结束回填）
  var placeholderResp = await apiCall('/api/sessions/' + sid + '/messages', {
    method: 'POST',
    body: { role: 'assistant', content: '', memoryCtx: null }
  });
  var assistantIdx = placeholderResp.data.idx;   // 服务端返回消息索引
  
  // 3. 流过程：纯 UI render，不发任何请求
  // （前端 var streamingDraft = { content, memoryCtx } 只在 UI 层用，不写真理源）
  
  // 4. 流结束：一次性 PATCH 占位消息
  await apiCall('/api/sessions/' + sid + '/messages/' + assistantIdx, {
    method: 'PATCH',
    body: { content: fullText, memoryCtx: memoryCtx }
  });
  
  // 5. refresh（确保 UI 和服务端一致；如果乐观更新则可省）
}
```

**关键点**：流过程中切走 tab，**user message 和 assistant 占位都已落盘**。流结束时 PATCH 没发出 → 下次打开看到一个空 assistant message（content=""），但**结构完整**，用户可以选择重发或继续。

### visibility 变化时强制 refresh

```js
document.addEventListener('visibilitychange', function() {
  if (!document.hidden) {
    // 切回前台：强制 refresh（不依赖 3 秒 polling）
    refreshSessions();
  }
});
```

替代原 polling + visibilitychange 的双逻辑。

### `manualSessionSync()` 按钮

保留原 ↻ 按钮，但简化：
```js
async function manualSessionSync() {
  await refreshSessions();   // 拉服务端最新
  renderMessages();
}
```

## 任务划分

| 任务 | 涉及文件 | 风险等级 |
|---|---|---|
| **Task 1: 后端 sessions.rs 重构** | `crates/ext-server/src/sessions.rs` | 低 |
| **Task 2: 后端 main.rs 接入** | `crates/ext-server/src/main.rs` | 低 |
| **Task 3: 前端 app.js 改造** | `crates/ext-server/src/static/app.js` | 中-高 |
| **Task 4: 验证 + playwright 复现** | （测试） | 零 |

四个任务**串行依赖**（前端依赖后端新 API），不并行。

---

## Task 1: 后端 sessions.rs 重构

**目标：** 服务端 hold 真理源 + atomic write + API 细化

**改动范围（绝对路径）：**
- **只可修改** `crates/ext-server/src/sessions.rs`
- **不可**碰 `crates/ext-server/src/main.rs`（Task 2 负责）
- **不可**碰任何前端文件（Task 3 负责）

**新增结构：**

```rust
use std::sync::Arc;
use parking_lot_lite::Mutex;  // 或者继续用 std::sync::Mutex
use axum::{
    extract::{Path, State},
    Json,
};

pub fn shared() -> Arc<Mutex<SessionsData>> {
    Arc::new(Mutex::new(load_sessions_from_disk()))
}

// 新增 atomic write
pub fn save_sessions_to_disk(data: &SessionsData) -> Result<(), String> {
    // ... tmp + rename + sync_all
}

// 通用 mutation helper
pub fn mutate<F, R>(state: &Arc<Mutex<SessionsData>>, f: F) -> Result<R, String>
where F: FnOnce(&mut SessionsData) -> R {
    let mut data = state.lock().unwrap();
    let result = f(&mut data);
    save_sessions_to_disk(&data)?;
    Ok(result)
}

// 新增 endpoints
pub async fn list(State(s): State<Arc<AppState>>) -> Json<Value>
pub async fn create(State(s), Json(req): Json<CreateReq>) -> Result<Json<Value>, String>
pub async fn patch(State(s), Path(id): Path<String>, Json(req): Json<PatchReq>) -> ...
pub async fn delete(State(s), Path(id): Path<String>) -> ...
pub async fn append_message(State(s), Path(id): Path<String>, Json(req): Json<MessageReq>) -> ...
pub async fn update_message(State(s), Path((id, idx)): Path<(String, usize)>, Json(req): Json<MessageUpdateReq>) -> ...
```

**保留旧 endpoints（向后兼容，标 deprecated）：**
- `GET /api/sessions` 旧路径继续工作（= 新 `list`）
- `POST /api/sessions` 旧行为保留

（实际上 Task 2 会用新 handler 替换路由，可以直接切到新 API。）

**步骤：**
1. 改 `save_sessions_to_disk` → atomic rename + sync_all
2. 加 `shared() -> Arc<Mutex<SessionsData>>`（启动用）
3. 加 `mutate()` helper
4. 加新 endpoint handlers：`create`, `patch`, `delete`, `append_message`, `update_message`
5. `load` 和 `save` handler 改成读内存（不再每次读盘）

**验证：**
- `cargo check -p dse-server` 通过
- 启动服务，curl GET/POST/PATCH/DELETE 全部正常
- 写入中途 kill 进程，重启后 load 仍正常（因为 tmp 文件不会被 rename，旧文件完整）

---

## Task 2: 后端 main.rs 接入

**目标：** AppState 新增 `sessions` 字段，注册新路由

**改动范围：**
- **只可修改** `crates/ext-server/src/main.rs`
- 在 `AppState` 加 `pub sessions: Arc<Mutex<SessionsData>>`
- 启动时 `sessions: sessions::shared()`
- 注册新路由（替换 `axum::routing::get(sessions::load).post(sessions::save)` 为更细的）

**验证：**
- `cargo build` 通过
- 启动后 `GET /api/sessions` 立即能拿到内存里的 sessions（不读盘）

---

## Task 3: 前端 app.js 改造

**目标：** 删除前端真理源，改造为 view

**改动范围：**
- **只可修改** `crates/ext-server/src/static/app.js`
- **不碰** HTML / CSS（API 改动不影响 markup）

**改动清单：**

| 位置 | 当前 | 改成 |
|---|---|---|
| L2 `var sessions = []` | 全局真理源 | 删，改 `var sessionsView = []`（cache） |
| L3 `var activeSessionId` | 全局真理源 | 保留但来源 = localStorage + 服务端 |
| L72-80 `syncSessionsToServer()` | 全量 POST | 删 |
| L85-92 `syncSessionsDebounced()` | debounced 全量 POST | 删 |
| L94-107 `fetchSessionsFromServer()` | init 时拉 | 改名为 `refreshSessions()`，每次需要最新时调用 |
| L121-194 `pollAndMergeSessions / startSessionPolling / stopSessionPolling` | 3s 轮询 | 全删，改 visibilitychange 触发 refresh |
| L196-205 `manualSessionSync()` | push + pull | 简化为只 pull |
| L207-213 `visibilitychange` | hidden 时拉 | 改成 **visible 时**拉 |
| L227-234 `createSession()` | push 创建 | POST `/api/sessions` |
| L236-241 `switchSession()` | push active | PATCH `/api/sessions/:id` { active: true } + localStorage |
| L243-251 `deleteSession()` | push 删除 | DELETE `/api/sessions/:id` + refresh |
| L1070-1171 `send()` 流式输出 | 末尾 syncSessionsToServer | 拆为 4 步：POST user → POST 占位 → 流过程不写 → PATCH 占位 |
| L1418-1419 `seedMemory` 流式末尾 | 同上 | 同上 |

**active_id 持久化：**
- 用 `localStorage.setItem('fm-active-id', id)`
- 启动时读：`var activeSessionId = localStorage.getItem('fm-active-id') || null`
- 切会话时写：`localStorage.setItem('fm-active-id', id)`
- refreshSessions 后如果 activeSessionId 不在 sessionsView 中，回退到 sessionsView[0]?.id

**乐观更新 vs 强制 refresh：**
- 简单 mutation（create / switch / delete）：操作后立即乐观更新 sessionsView + render，**不**强制 GET refresh
- 流式输出结束：PATCH 成功后乐观更新该 session 的最后一条 message，**不**强制 GET refresh
- visibilitychange → visible：强制 GET refresh（应对多设备竞态）
- 手动 ↻ 按钮：强制 GET refresh

**步骤：**
1. 备份当前 app.js（cp 到 app.js.bak）—— **改动过大，必须有回滚**
2. 替换 `var sessions = []` 为 `var sessionsView = []`
3. 加 localStorage 读写 helper
4. 改写 `createSession / switchSession / deleteSession` → API call + 乐观更新
5. 改 `fetchSessionsFromServer` 为 `refreshSessions`
6. 删 `syncSessionsToServer / syncSessionsDebounced / pollAndMergeSessions / startSessionPolling / stopSessionPolling`
7. 改 `manualSessionSync` → 简化为只 refresh
8. 改 `visibilitychange` handler → visible 时 refresh
9. 改 `send()` 流式输出为 4 步原子化
10. 改 `seedMemory` 流式末尾同步

**验证：**
- 浏览器刷新 → session 列表仍在（GET 拉服务端）
- 浏览器关闭再开 → session 仍在
- 切到 /field 切回 → session 仍在
- 流式输出过程中切走 → 切回时 user message 已在，assistant message 为空占位
- LAN 设备打开 → 看到完整 sessions（默认第一个）

---

## Task 4: 验证

**验证流程：**

1. `cargo build -p dse-server` 通过
2. 启动服务（kill 旧进程 + 启动新）
3. curl 验证所有 endpoint：
   ```bash
   curl http://localhost:5100/api/sessions
   curl -X POST http://localhost:5100/api/sessions -H 'Content-Type: application/json' -d '{}'
   curl -X PATCH http://localhost:5100/api/sessions/<id> -H 'Content-Type: application/json' -d '{"active":true}'
   curl -X DELETE http://localhost:5100/api/sessions/<id>
   curl -X POST http://localhost:5100/api/sessions/<id>/messages -H 'Content-Type: application/json' -d '{"role":"user","content":"hi"}'
   curl -X PATCH http://localhost:5100/api/sessions/<id>/messages/1 -H 'Content-Type: application/json' -d '{"content":"hello"}'
   ```
4. playwright 真实复现切走切回场景
5. 写一个并发测试：两个 client 同时 PATCH → 验证不丢

---

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| 改动过大，前端可能出 bug | 备份原 app.js，渐进式替换 |
| atomic rename 在某些 FS 不原子 | 文档里写明假设同一文件系统（同 ext-server cwd） |
| 多设备并发 PATCH | 服务端单写者（Mutex），顺序处理 |
| 流式输出占位消息被用户看到"空"状态 | UI 上明确显示"生成中..." |
| 旧 sessions.json 不含新字段 | 用 `#[serde(default)]` 兼容（旧文件已有此设计） |

---

## 不在本次范围

- ❌ WebSocket / SSE 推送（轮询 + visibilitychange 已够用）
- ❌ 加密 / 鉴权（当前是内网工具，信任边界在 LAN）
- ❌ 消息搜索（messages 在 sessions.json 里，可以 grep；不做索引）
- ❌ 历史消息归档（用户没有这个需求）
- ❌ seed_memory / recall 流式末尾同步 —— 沿用 send() 的 4 步模式（已在 Task 3 范围）
