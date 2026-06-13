# AGENTS.md — DSE-Memory

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
- **依赖 `DummyEmbedProvider` 的测试天然不稳定** — 标记 `#[ignore]` 并写注释，接入真实 `EmbedProvider` 才能通过。

## 已记录教训

以下教训来自 Phase 1 的真实调试。修改相似代码前必读。

1. **`lib.rs` 的 re-export 不可引用不存在的模块**。先做骨架或推迟 re-export 到目标模块创建时。
2. **`bincode 1.x` 中 `bincode::Error` 是 `Box<bincode::ErrorKind>`**。改用 `Serialization(String)` 手动 map。
3. **`sled::Error::Unsupported` 不接受字符串参数**。改用 `MissingData(&'static str)`。
4. **`use` 列表必须包含全部引用类型**。dead-code 检查不覆盖类型推导路径。
5. **`Vec.remove()` 后不能复用之前绑定的 `n`**。移除后 `return` 不是 `break`。
6. **`DummyEmbedProvider` 基于哈希，不是语义**。相关文本可能产出正交向量。
7. **`cos_sim(正交) = 0 → impact = 0 → effective_direction = 事件原方向**。

