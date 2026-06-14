# ext-server 架构

> ext-server 是 field-memory 的 HTTP/前端外挂。它把核心引擎包成 OpenAI 兼容的 chat endpoint + 控制台 + 3D 可视化。核心引擎本身是零 HTTP/前端依赖的（见 `docs/design.md`），所有 web 相关逻辑都在这里。

> 2026-06-14 — sessions 持久化重构（真理源迁服务端）

---

## 核心立场

**服务端是唯一真理源。** 浏览器只是 view。

无论是会话列表、记忆库状态、ECG 快照，所有可见状态都在服务端有一份权威数据。客户端拿到的是 view，不是真理。

这个立场和核心引擎的"系统拒绝分类与判断"一脉相承——会话状态本来就该是**服务端的状态机**，不是浏览器里的数组。

## 服务端真理源

```rust
pub struct AppState {
    pub engine: Arc<Mutex<DseEngine>>,
    pub libraries: Arc<Mutex<HashMap<String, (usize, usize)>>>,
    pub active_library: Arc<Mutex<String>>,
    /// Server-side source of truth for sessions.
    pub sessions: Arc<Mutex<SessionsData>>,
}
```

启动时一次性 `load_sessions_from_disk()` 灌入 `Arc<Mutex>`，之后 GET 直接读内存快照。**所有 mutation 走同一个 helper：**

```rust
pub fn mutate<F, R>(state: &Arc<Mutex<SessionsData>>, f: F) -> Result<R, String>
where F: FnOnce(&mut SessionsData) -> Result<R, String> {
    let mut data = state.lock().map_err(|e| format!("lock poisoned: {e}"))?;
    let result = f(&mut data)?;
    save_sessions_to_disk(&data)?;   // 原子写
    Ok(result)
}
```

任何 mutation 路径都必须走 `mutate`，强制 atomic 落盘。没有 mutation 能漏写。

## 原子写

```rust
pub fn save_sessions_to_disk(data: &SessionsData) -> Result<(), String> {
    let path = data_path();
    let tmp = path.with_extension("json.tmp");
    let s = serde_json::to_string_pretty(data)?;
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(s.as_bytes())?;
        f.sync_all()?;                    // fsync 落盘
    }
    std::fs::rename(&tmp, &path)?;       // POSIX 原子 rename
    Ok(())
}
```

`rename` 在同一文件系统上是原子的。旧文件在 rename 完成前始终完整，永远不会有半截 JSON。

## API 表面

```
GET    /api/sessions                       → 拿全部 sessions（list 内存快照）
POST   /api/sessions                       body: { title? }   → 创建
PATCH  /api/sessions/{id}                 body: { active?, title? }
DELETE /api/sessions/{id}
POST   /api/sessions/{id}/messages         body: { role, content, time?, memoryCtx? } → 追加，返 idx
PATCH  /api/sessions/{id}/messages/{idx}   body: { content?, memoryCtx? } → 流结束一次性更新
```

错误格式统一：`{ ok: false, error: msg }`（HTTP 200），前端 apiCall 用 `kind: 'server'` 捕获。

## 流式输出原子化（4 步）

之前的设计是 fire-and-forget 全量 POST，**流最后一笔 assistant message 经常丢失**。现在的协议：

1. **发消息时**：await POST `/api/sessions/{id}/messages` {role:"user", content, time} → user message 落盘
2. **流开始时**：await POST `/api/sessions/{id}/messages` {role:"assistant", content:"", memoryCtx:null} → 拿到 placeholder `idx`（这一步必须成功才有地方 PATCH）
3. **流过程中**：纯 UI render，**不发任何请求**。前端 var streamingDraft 只在 UI 层用，不写真理源
4. **流结束时**：fire-and-forget PATCH `/api/sessions/{id}/messages/{idx}` {content:fullText, memoryCtx}

**关键不变量**：user message 和 assistant 占位都已落盘后才开始 streaming。即使流过程中切走 tab，下次回来至少看到 "user msg + 空 assistant 占位" 的结构——而不是丢得什么都不剩。

## 前端契约

- **`var sessions = []` 是 cache，不是真理源**——每次启动 `refreshSessions()` 从服务端拉
- **`activeSessionId` 用 `localStorage` 记忆**——每设备独立，多设备不被强行同步
- **每次 mutation 立即 fire-and-forget POST**，cache 乐观更新
- **没有 3 秒 polling**——`visibilitychange` 在 visible 时触发一次 `refreshSessions()` 即可
- **手动 ↻ 按钮 = `refreshSessions()`**——拉服务端最新

## 为什么不再需要 polling

之前 3 秒 polling 是为了多设备同步。但 fire-and-forget POST + 单写者 Mutex 让"最后写入赢"问题消失。多设备要看到对方改动，按 ↻ 手动刷一次就行——对个人 LAN 工具完全够用。

如果未来需要实时多设备，加 SSE / WebSocket 推送即可，架构已经预留了位置（GET 返内存快照，可直接包成 SSE）。

## 不在本架构范围

- ❌ WebSocket / SSE 推送（polling + visibilitychange 已够）
- ❌ 鉴权 / 加密（信任边界在 LAN）
- ❌ 消息搜索索引
- ❌ 历史消息归档
- ❌ 多用户 / RBAC

## 教训（详见 AGENTS.md）

- 第 11 条：会话真理源必须在服务端
- 第 12 条：atomic rename 防半截写
