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

密度越高，锚点越难被推动（stiffness 高）且越难忘（damping 低）。

## 架构

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

## 设计亮点

| 亮点 | 说明 |
|---|---|
| 无 LLM 判定 | Event 输入只有 `text → embedding → direction`，不调用 LLM 做 gap/情绪/概念分类 |
| 唯一演化入口 | `RelaxationCycle.run()` 是场状态变化的唯一下降。没有额外的 reconsolidate / time_collapse / scan_dormancy 函数 |
| 四层 = 四个参数组 | L1-L4 没有独立数据结构。stiffness + damping 的取值自然决定一个 anchor 是"短期工作记忆"还是"长期核心信念" |
| 召回 = 写入同广播 | 查询和事件走同一个 `impact()` 函数，回忆加固只是查询作为轻事件走了步轻松弛 |
| 范式转移 = 唯一结构变换 | shadow_anchor 保留完整快照，L4 到 L3 的恢复是无损的 |
| 5 个参数 | 向量维度、时间窗口、阻尼系数、刚度系数、收敛阈值。所有内部参数由此推导 |

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
