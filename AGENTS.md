# AGENTS.md — field-memory

## 设计意图

field-memory 抛弃了"向量数据库 + RAG"的传统路线。核心立场是：

- 记忆不是静态存储，是系统被事件扰动后收敛到的**平衡态**
- 概念的意义由关联事件的**空间密度**动态赋予，不由硬编码标签决定
- 系统拒绝分类与判断。没有 Gap、Valence、Emotion、RecallStamp。所有行为从**一个物理量**涌现

核心方程：

```
impact(anchor, event) = cos_sim(anchor.direction, event.direction) * sqrt(anchor.density)
```

范式转移、ECG、召回、持久化全部由此推导。

## 核心元素

| 结构体 | 角色 | 关键字段 |
|---|---|---|
| `Event` | 输入到场的唯一信号 | direction, text, timestamp |
| `AnchorKey` | 记忆场的节点 | direction, density, stiffness, damping, origin_direction |
| `ImpactTrace` | 松弛副产品，召回用 | event_id, anchor_id, impact, timestamp |
| `SeedConcept` | L4 静默种子 | orthogonal_direction, shadow_anchor, defeated_by |

锚点的 `stiffness` 和 `damping` 由 `density` 自动推导：

```
stiffness = sqrt(density)
damping   = 1 / sqrt(density)
```

## 文件结构

```
field-memory/
├── Cargo.toml
├── crates/
│   ├── core/
│   │   └── field-mem-core/    # 核心引擎 — 纯记忆逻辑，零外部依赖
│   │       └── src/*.rs
│   ├── ext-cli/               # 外挂：命令行 demo
│   └── ext-server/            # 外挂：HTTP 聊天服务器
├── docs/
│   ├── design.md
│   └── superpowers/plans/
└── AGENTS.md
```

核心引擎 `field-mem-core` 保持纯粹，不引入任何 HTTP、前端、鉴权、业务逻辑。外挂模块通过 Cargo workspace 引用核心，各自独立演进。

## 设计亮点

| 亮点 | 说明 |
|---|---|
| 无 LLM 判定 | Event 输入只有 `text -> embedding -> direction`，不调用 LLM |
| 唯一演化入口 | `RelaxationCycle.run()` 是场状态变化的唯一下降 |
| 四层 = 四个参数组 | stiffness + damping 自然区分短期/长期记忆，无独立层结构 |
| 召回 = 写入同广播 | 查询和事件走同一个 `impact()` 函数 |
| 范式转移 = 唯一结构变换 | shadow_anchor 保留完整快照，恢复无损 |
| 5 个参数 | 向量维度、时间窗口、阻尼系数、刚度系数、收敛阈值 |

## 工程规约

- **5 个配置参数** — `DseCoreParams` 只增不减需要计划更新。
- **一文件一职责** — 不跨模块泄漏职责。
- **`engine.rs` 只做编排** — 算法改动在 `physics.rs` / `cycle.rs` / `recall.rs`。

## 测试规约

- **内联 `#[cfg(test)] mod tests`** — 单元测试和实现写在一起。
- **`tests/integration.rs`** — 跨模块端到端测试。
- **依赖 `DummyEmbedProvider` 的测试天然不稳定** — 标记 `#[ignore]` 并写注释。

## 已记录教训

1. **`lib.rs` 的 re-export 不可引用不存在的模块**。先做骨架或推迟 re-export。
2. **`bincode 1.x` 中 `bincode::Error` 是 `Box<bincode::ErrorKind>`**。改用 `Serialization(String)` 手动 map。
3. **`sled::Error::Unsupported` 不接受字符串参数**。改用 `MissingData(&'static str)`。
4. **`Vec.remove()` 后不能复用之前绑定的 `n`**。移除后 `return` 不是 `break`。
5. **`DummyEmbedProvider` 基于哈希，不是语义**。

