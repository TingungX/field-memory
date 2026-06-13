# AGENTS.md — DSE-Memory

项目级 agent 指令。不替代任何上级 agent session 配置。

## 始终适用

1. **使用中文沟通**。代码、命令、报错保留原文。
2. **优先简单直接可维护的方案**。避免过度设计。
3. **保持与现有代码风格一致**。非必要不重构无关代码。
4. **禁止硬编码**密钥、Token、私密 URL。
5. **禁止提交非 Rust 构建产物** — `/target`、`*.sled/`、`Cargo.lock`（`Cargo.lock` 由于 workspace 有二进制 crate，**已加入 git 跟踪**，不要从 `.gitignore` 中移除）。

## 项目结构

```
MEMORY/
├── Cargo.toml                 # workspace，3 个 member
├── crates/
│   ├── dse-core/              # 核心引擎 (library)
│   │   └── src/*.rs           # 每文件一职责
│   ├── dse-cli/               # 命令行 demo (binary)
│   └── dse-server/            # HTTP 聊天服务器 (binary)
│       └── src/static/index.html  # 前端单页
├── docs/
│   ├── design.md              # v2 架构文档（改动前必读）
│   └── superpowers/plans/     # TDD 实现计划
└── .gitignore
```

## 工程规约

- **5 个配置参数** — `DseCoreParams` 只增不减需要计划更新。
- **一文件一职责** — 不跨模块泄漏职责（physics 不写 cycle 逻辑，cycle 不写 recall 逻辑）。
- **`engine.rs` 只做编排** — 算法改动在 `physics.rs` / `cycle.rs` / `recall.rs`。
- **`lib.rs` 的 re-export 是公开 API** — 改动需要同步更新全部 downstream。

## 测试规约

- **内联 `#[cfg(test)] mod tests`** — 单元测试和实现写在一起。
- **`tests/integration.rs`** — 跨模块端到端测试。
- **测试命名**：`test_<功能或行为>_<预期结果>`。
- **依赖 `DummyEmbedProvider` 的测试天然不稳定** — 标记 `#[ignore]` 并写注释，不要"修正"数学来曲线救国。接入真实 `EmbedProvider` 才能确定性地通过。

## 输出规约

- 先说做了什么，再解释为什么。
- 列出修改文件。
- 非平凡改动同时列出 plan / public API / 持久化格式变更。

## 调试 / 修复规约

- **先找根因再改代码**。不要用 `unwrap_or_default` 吞 panic，不要注释代码路径。
- **Plan 优先**。如果发现计划文件有内部矛盾，先改计划，单独提交，再实现。
- **无法验证的测试标记 `#[ignore]` + 注释原因**。不删除——它们记录了设计意图。

## 已记录教训

以下教训来自 Phase 1 的真实调试。修改相似代码前必读。

1. **`lib.rs` 的 re-export 不可引用不存在的模块**。workspace scaffold 阶段先做骨架或推迟 re-export 到目标模块创建时。
2. **`bincode 1.x` 中 `bincode::Error` 是 `Box<bincode::ErrorKind>`**。`PersistError` 用 `#[from] bincode::Error` 会编译失败，改用 `Serialization(String)` 手动 map。
3. **`sled::Error::Unsupported` 不接受字符串参数**。改用自定义 `MissingData(&'static str)` 变体。
4. **`use` 列表必须包含全部引用类型**。Rust 的 dead-code 检查不覆盖类型推导路径。
5. **`Vec.remove()` 后不能复用之前绑定的 `n`**。索引越界。移除后立即 `return` 而不是 `break`。
6. **`DummyEmbedProvider` 基于哈希，不是语义**。相关中文文本可能产出正交向量。断言密度增长"同一主题提到多次→密度更高"需要真实 `EmbedProvider`。
7. **`cos_sim(正交方向) = 0 → impact = 0 → effective_direction = 事件原方向**。测试"锚点拉动事件方向"的用例如果输入正交方向，数学上不可满足。

## 有疑问时

1. 先读 `docs/design.md` 理解 why，再动 what。
2. 读 `docs/superpowers/plans/` 中最新的计划文件获取 task 上下文。
3. 至少读 3 个 `crates/dse-core/src/` 中的同层文件理解风格。
4. 如果改动影响 public API 或持久化格式，在同一个 commit 中更新 `docs/design.md`。

