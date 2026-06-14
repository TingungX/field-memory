# init 重构计划 — 从标签列表到认知地形

## 目标

重写 init 流程：从手动列举 5-10 个"用户偏好"标签，变为 LLM 从用户意图描述中自动提取 40-60 个概念维度，构建有拓扑结构的密集锚点场。

## 核心设计决策

### 1. LLM 是概念提取器，不是判断器

init 时 LLM 提取概念，这是场里唯一合法的 LLM 调用。这不是"运行时判断"，是"场的构建"——场诞生前它不存在，LLM 帮它获得初始结构。场运行后，所有行为从 impact() 涌现，不再调用 LLM。

### 2. 用户描述意图，LLM 展开地形

用户说"我希望你是什么样的"，而不是"你是什么样的"。一句话 → LLM 提取 40-60 个概念 → 每个概念 embedding → 密集锚点场。

### 3. 场是逐层沉积的

init 可以多次调用。每次追加 40-60 个锚点到已有场上。新锚点与已有锚点之间自然产生 impact 关系。场不是一次性浇筑，是逐步增长的多元地形。

### 4. 统一接口，废除 seed

`seed_memory` 和 `init` 做同一件事。统一为 `init`，seed_memory 从工具列表中移除。

### 5. 场能多大就多大

无人工上限。LLM 觉得这个领域有 80 个维度就 80 个。关键是每个锚点都有真实语义的 direction（来自 embedding）。

## 修改清单

### Phase 0: 核心引擎（零风险）

- [x] **`init.rs`** — 新增 `init_from_concepts(embed, concepts)` 接收 `Vec<(String, f32)>`（label + fundamentality），代替旧的 `&[(&str, u32)]`
  - fundamentality (0.0-1.0) → density 映射：`density = (fundamentality * 20.0).ceil() as u32`，范围 1-20
  - 旧 `init_anchors` 保留为内部兼容函数，标记 `#[deprecated]`
- [x] **`engine.rs`** — 新增 `DseEngine::init_from_descriptions(&mut self, descriptions: &[(String, f32)])` 
  - 旧 `init()` 保留但标记 `#[deprecated]`
- [x] **测试** — `init.rs` 新增测试：高 fundamentality → 高 density，低 → 低

### Phase 1: LLM 概念提取（低风险）

- [x] **`ext-server/llm.rs`** — 新增 `extract_concepts(backend_url, model, user_intent) -> Vec<(String, f32)>` 
  - 调用 LLM，prompt 要求从用户意图描述中提取 40-60 个核心概念
  - 输出格式：JSON array of `{"concept": "...", "fundamentality": 0.0-1.0}`
  - Prompt 要点：
    - 你正在为一个势能场记忆系统构建初始认知地形
    - 概念应该是该领域的基础维度，不是具体事实
    - fundamentality 越高表示该概念越核心、越不可绕过
    - 概念之间应该有足够的语义差异（不要列出近义词）
    - 不要包含用户偏好本身（如"喜欢Rust"），而是偏好背后的维度（如"类型安全直觉"）
- [x] **`ext-server/tools.rs`** — 重写 `seed_memory` 为 `init_field`
  - 工具名：`init_field`
  - 输入：`intent`（用户意图描述文本，如"我希望你是一个注重长期方案的系统程序员"）
  - 流程：`extract_concepts(intent)` → `engine.init_from_descriptions(concepts)` → relax
  - 保留锚点去重逻辑（已有标签的跳过）
  - 输出：新创建的锚点数、跳过的重复数、当前场总览
- [x] **`ext-server/tools.rs`** — 移除 `seed_memory` 和 `associate_memory` 工具
  - `seed_memory` → 由 `init_field` 替代
  - `associate_memory` → 功能被 `recall_memory` 覆盖，且语义不清
- [x] **测试** — 手动测试：输入"我希望你是注重长期方案的系统程序员"，验证概念提取和场构建

### Phase 2: 接口统一（低风险）

- [x] **`ext-server/routes.rs`** — `/seed` API 重命名为 `/init`
  - 请求体从 `{concepts: [...], events_per_anchor, relax_cycles}` 改为 `{intent: "..."}`
  - 旧 `/seed` 保留为重定向或兼容
- [x] **`ext-server/memory_routes.rs`** — seed handler 重命名为 init handler
- [x] **前端 `app.js`** — 斜杠命令 `/init` 触发 `init_field` 工具调用
  - `/init` 是用户侧入口，指导 LLM 调用 `init_field` tool
  - 移除 `/seed` 斜杠命令
- [x] **前端 `app.js`** — 更新 LLM system prompt 中的工具描述

### Phase 3: 场可视化适配（中风险）

- [x] **`field.js`** — 适配大规模场（50-200 锚点）的渲染
  - 锚点球体大小根据 density 缩放（densityRatio: 0.5 + 0.5 * density/maxDensity）
  - 大规模 dimming：anchors > 50 时低于 density 中位数的锚点变暗
  - 标签 top-K：anchors > 50 时默认只显示 top-20 by density 的标签
  - 连线简化：anchors > 80 时 rebuildConnections 只处理 top-30 by density
- [ ] **`app.js`** — 锚点列表适配大量数据（虚拟滚动或分页）（推迟到前端重构）

## 不在本次范围内

- 模型回复入记忆（之前的讨论，独立问题）
- recall 加强记忆的接入（consolidate_from_recall 调用，独立问题）
- damping/stiffness 接入 DseCoreParams 的 base 参数（当前硬编码，暂可接受）
- DummyEmbedProvider → 真实 embedding 的测试基础设施

## 验收标准

1. 用户输入一句意图描述 → 场自动生成 40-60 个锚点
2. 多次 init 追加锚点，不覆盖已有场
3. 场的 recall 区分度显著提升（cos_sim 分布更宽，不再全部挤在 0.4-0.6）
4. 3D 可视化能看到有结构的锚点分布，不是一团混沌
5. `init_field` 工具正常工作，`seed_memory` 和 `associate_memory` 完全移除
