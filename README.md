# DSE-Memory

**Dynamic Semantic Evolutionary Memory** — 统一势能场记忆引擎。

> 概念的意义由其关联事件的空间密度分布动态赋予。
> 记忆不是数据库，是松弛系统。状态不是"写入"，是连续平衡被扰动后的收敛结果。
> 没有判断，没有分类，只有扰动与收敛。

## 核心理念

DSE-Memory 抛弃了传统的"向量数据库 + RAG"路线。系统中的每个概念（Anchor）和每个事件（Event）都活在同一张势能场里。回忆是**松弛过程**：查询向量广播到所有锚点，方向相近且密度高的锚点产生引力，场向查询方向收敛。记忆是系统的平衡态，不是查表。

核心方程只有一行：

```
impact(anchor, event) = cos_sim(anchor.direction, event.direction) x sqrt(anchor.density)
```

范式转移、认知心电图（场张力/收敛度/方向偏压）、召回引擎、持久化——全部由此推导。

## 架构

```
MEMORY/
├── crates/
│   ├── dse-core/              # 核心引擎 (library)
│   │   └── src/
│   │       ├── types.rs       # Event, AnchorKey, ImpactTrace, SeedConcept
│   │       ├── math.rs        # cosine_sim, normalize, l2, dot, add, scale
│   │       ├── embed.rs       # EmbedProvider trait + DummyEmbed
│   │       ├── physics.rs     # impact(), effective_direction(), relax_step()
│   │       ├── cycle.rs       # RelaxationCycle（唯一演化机制）
│   │       ├── recall.rs      # value_init / associate / recall
│   │       ├── init.rs        # 概念描述 → Anchor 创建
│   │       ├── paradigm.rs    # 范式转移检测 + 正交投影 + 恢复
│   │       ├── ecg.rs         # 场张力 / 收敛度 / 方向偏压（纯观测）
│   │       ├── engine.rs      # DseEngine 总控（公共 API）
│   │       └── persist.rs     # sled 持久化
│   │
│   ├── dse-cli/               # 命令行 demo
│   │   └── src/main.rs
│   │
│   └── dse-server/            # HTTP 聊天服务器
│       ├── src/
│       │   ├── main.rs        # axum 服务器，端口 3000
│       │   ├── routes.rs      # OpenAI + Anthropic 兼容 API
│       │   ├── llm.rs         # 后端 LLM 流式转发
│       │   └── static/
│       │       └── index.html # 聊天前端（单页，纯净风）
│       └── Cargo.toml
│
└── docs/
    └── design.md              # v2 架构设计文档
```

## 配置：5 个参数

```rust
pub struct DseCoreParams {
    pub vector_dim: usize,            // 向量维度（默认 32）
    pub event_window_secs: u64,       // 松弛时间窗口（默认 3600s）
    pub damping_base: f32,            // 全局阻尼系数（默认 0.5）
    pub stiffness_base: f32,          // 全局刚度系数（默认 1.0）
    pub convergence_threshold: f32,   // 松弛收敛阈值（默认 0.001）
}
```

## 快速开始

```bash
# CLI demo：初始化 → 写入 → 松弛 → 召回 → ECG → 持久化
cargo run -p dse-cli

# 启动聊天服务器（需要配置 LLM 后端）
LLM_BACKEND=http://localhost:11434/v1/chat/completions \
LLM_MODEL=qwen2.5:0.5b \
cargo run -p dse-server

# 运行全部测试
cargo test
```

打开 `http://localhost:3000` 进入聊天界面。支持 OpenAI 和 Anthropic 两种 API 格式同时接入。

## 项目状态

**Phase 1 MVP 完成。** 全部 27 个单元测试 + 1 个集成测试通过。三个 crate 均可编译运行。

两个测试标记为 `#[ignore]`，原因是 `DummyEmbedProvider` 基于哈希而不是语义，无法满足需要语义相似性的测试预期。接入真实 `EmbedProvider` 后即可启用。

## 技术栈

| 组件 | 选择 | 理由 |
|---|---|---|
| 语言 | Rust 2021 | 零抽象、安全、高密度计算场景无需 GC 暂停 |
| HTTP | axum | 异步、类型安全路由、SSE 原生支持 |
| 持久化 | sled | 嵌入式 KV 存储，纯 Rust，无外部依赖 |
| 序列化 | serde + bincode | 高性能二进制序列化 |
| 前端 | 静态 HTML/CSS/JS | 零构建依赖、单文件部署 |

## 许可协议

Copyright (C) 2026 Tingung <TingungX@outlook.com>

GNU Affero General Public License v3.0 (AGPL-3.0)

如果你将本引擎作为网络服务提供修改版本，必须向服务用户提供修改后的源代码。

## 参考文档

- `docs/design.md` — v2 精简架构设计（统一势能场的完整逻辑推导）
- `docs/superpowers/plans/2026-06-13-dse-phase1.md` — Phase 1 实现计划（14 个 TDD 任务）
- `AGENTS.md` — 项目级 agent 指令

