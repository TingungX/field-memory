# DSE-Memory 架构设计文档 (v2 — 统一势能场)

Status: superseded
Owner: field-memory
Last updated: 2026-08-12
Scope: 冻结的 v1 Anchor/impact 实现设计；历史标题中的 “v2” 不再表示当前 field-memory v2
Related code: `crates/core/field-mem-core`
Superseded by: [field-memory v2 理论基石](design/field-memory-v2-foundations.md)

> 本文保留现有 v1 实现的历史设计，不是当前 v2 规范。v2 不沿用本文的
> AnchorKey、impact、stiffness/damping 或 RelaxationCycle 路径。

> Dynamic Semantic Evolutionary Memory
> 2026-06-13 — clean-slate revision

---

## 核心公理

**概念的意义由其关联事件的空间密度分布动态赋予。**

**记忆不是数据库，是松弛系统。状态不是"写入"，是连续平衡被扰动后的收敛结果。**

**没有判断，没有分类，只有扰动与收敛。**

---

## 第1段：数据模型

### Event

事件是场的唯一输入。没有 gap、valence、emotion、concept_ids。

```rust
struct Event {
    id: EventId,
    /// 语义方向：从 embedding 模型产出，参与场的所有计算
    direction: Vector,
    /// 原始文本：存储给人看的，不参与任何场的判定
    text: String,
    timestamp: DateTime<Utc>,
}
```

### AnchorKey

锚点是场的核心节点。两个力学参数由 density 自动推导。

```rust
struct AnchorKey {
    id: AnchorId,
    label: String,
    /// 当前语义方向：无数事件松弛演化的累积结果
    direction: Vector,
    /// 密度：命中次数，松弛副作用
    density: u32,
    /// 刚度：抗推能力。√density。密度越高越难推。
    stiffness: f32,
    /// 阻尼：回归速度。1/√density。密度越高回归越慢（越难忘）。
    damping: f32,
    /// 初始方向：用于范式转移复位
    origin_direction: Vector,
}
```

### ImpactTrace

事件广播到锚点的冲击记录。松弛过程的副产品，用于召回时回溯事件。

```rust
struct ImpactTrace {
    event_id: EventId,
    anchor_id: AnchorId,
    impact: f32,             // cos_sim × √density
    timestamp: DateTime<Utc>,
}
```

### SeedConcept (L4)

不参与松弛。只在范式转移时与 L3 互换。

```rust
struct SeedConcept {
    id: SeedId,
    orthogonal_direction: Vector,
    shadow_anchor: AnchorKey,    // 完整保留被推入前的状态（无损逆转）
    defeated_by: AnchorId,
    pressure_accumulated: f32,
}
```

### 四层 = 同一场的四种参数集

没有独立的四层数据结构。只有 `Vec<AnchorKey>`，锚点自身的 (stiffness, damping) 决定了它是 L1 态还是 L3 态。

| 态 | stiffness | damping | 物理意义 |
|---|---|---|---|
| L1 | 低（√density小） | 高 | 易推、快速回归 → 短期工作记忆 |
| L2 | 中 | 中 | 部分吸收 |
| L3 | 高（√density大） | 低 | 难推、几乎不回归 → 长期核心信念 |
| L4 | seed 不参与松弛 | — | 静默种子，范式转移时互换 |

---

## 第2段：势能场统一方程

### 核心量：impact

一个公式统治一切：

```
impact(anchor, event) = cosine_similarity(event_direction, anchor_direction)
                      × √anchor_density
```

| impact 控制 | 方式 |
|---|---|
| 吸收程度 | 高 impact → 锚点响应强 → 方向变化大 |
| 密度增长 | new_density = density + ⌈impact × 0.1⌉ |
| 召回排序 | impact 就是 recall score |
| 层间引力 | 高 stiffness 锚点 impact 大 → 在松弛中"拉住"事件方向 |
| 冲突结果 | 密度大者 impact 大 → 自然胜出 |

### 事件模长 = 场共振总量

```
event_magnitude = Σ impact(anchor_i, event)
```

- 高密度锚点 → impact 大 → 事件模长大
- 多个锚点共振 → 事件模长大
- 零共振 → 事件模长小

事件模长不存储为 Event 字段，广播时动态计算。

### 有效事件方向（层间引力的物理实现）

```
effective_dir = normalize(event_direction + Σ impact(anchor_i) × anchor_i.direction)
```

高密度锚点惯性大 → 把事件方向往自己那边拉 → 其他被扰动的锚点吸收的是"被偏转后的事件方向" → 方向相近的锚点被互相拉近。

### 松弛一步

```
push = impact × effective_dir
pull = damping × (anchor.last_direction - anchor.direction)
anchor.direction = normalize(anchor.direction + push + pull)
```

| 项 | 物理 |
|---|---|
| push | 事件扰动：把锚点往事件方向推 |
| pull | 阻尼回归：把锚点往原位拉 |
| stiffness | 隐含在 impact 里：高密度锚点 impact 大 → 事件的推力比锚点的惯性小 → 锚点几乎不动 |

### 累积扰动 → 决定松弛步数（等效反刍）

```
perturbation(anchor) = Σ recent_impacts(anchor)

steps = {
    > 10.0 → 50 步   (强扰动，等效深度反刍)
    > 3.0  → 15 步   (中等)
    > 1.0  → 5 步    (轻微)
    else   → 2 步    (几乎无感)
}
```

没有反刍等级、没有变体生成、没有云端定级。只有松弛步数的不同。

---

## 第3段：写入

### 流程图

```
用户原始文本
    │
    ├──→ embedding 模型 → direction → Event.direction
    │
    ├──→ 原文保留 → Event.text
    │
    └──→ 事件入队
```

### 松弛周期

```
relax_cycle()
    │
    ├── 1. 过滤窗口内事件 (event_window)
    │
    ├── 2. for each anchor:
    │     ├── 计算累积扰动
    │     ├── 决定松弛步数
    │     ├── 迭代松弛 (push + pull) 至收敛或步数耗尽
    │     ├── 密度更新
    │     └── 记录 ImpactTrace
    │
    └── 3. 范式转移检测
```

---

## 第4段：召回（Recall）

写入和读取用同一个广播机制。

```
写入：event → broadcast → anchors 响应 → 松弛改变状态
读取：query → broadcast → anchors 响应 → 返回结果（不改变状态）
```

### 被动层1（价值初始化）

不广播。直接读场状态。

```
session 开始时：
  取 density top-20 的锚点
  返回 {direction, density, label}
  → 用于向量注入到 Pre-fill / System Prompt
```

### 被动层2（即时关联）

浅探。

```
query 进入 → broadcast
  → 所有锚点计算 impact
  → 取 impact > threshold 的锚点
  → 返回 {label, impact}
  → 不展开事件
```

高密度锚点 impact 大，自然排前面。

### 主动层（recall_memory）

深探 + 事件回溯 + 方向相近锚点连带激活。

```
query 进入 → broadcast
  → 取 impact top-K 的锚点
  → 锚点 → ImpactTrace → event_id → Event.text → 返回给人
  → 方向相近的锚点因 impact 相似而连带出现在结果中
```

链式扩散不需要 BFS。方向被历史事件拉近的锚点，对同一 query 天然有相近 impact。

### 回忆加固

查询本身作为极轻事件，走一步轻松弛。

```
if impact(query, anchor) > threshold:
    anchor.density += 1
    anchor.direction = normalize(anchor.direction + impact_q × query.direction)
```

不需要显式的 reconsolidation 逻辑。

---

## 第5段：范式转移

唯一的结构变换（松弛做不到）。

```rust
/// 触发条件：方向相反 + 压强长期偏向一边
fn detect_paradigm_shift(a: &AnchorKey, b: &AnchorKey) -> bool {
    let divergence = cosine_distance(&a.direction, &b.direction);
    let a_pressure = cumulative_perturbation(a, window);
    let b_pressure = cumulative_perturbation(b, window);
    divergence > 0.8 && a_pressure > 3.0 * b_pressure && a.density > 5
}
```

执行：

```
正交投影 → shadow_anchor 保留完整状态（无损）→ 种子写入 → 原锚点从主表移除
```

恢复：

```
种子激活 → shadow_anchor 恢复 → 锚点重入主表 → 原主导锚点正交投影推入种子
```

---

## 第6段：初始灌注

不变。模型自生成 + 用户干预 → 创建初始高密度锚点。

```rust
InitPhase:
    for each concept_description:
        anchor.direction ← embedding(描述)
        anchor.density  ← initial_density (10-50)
        anchor.stiffness ← √density（自动）
        anchor.damping   ← 1/√density（自动）
```

高初始 density → 高 stiffness + 低 damping → 锚点几乎不动 → 基础价值观势阱。

用户干预部分初始 density 更高（15.0 对应的高 density 值）→ 免疫优先级。

---

## 第7段：认知心电图

纯观测。不产生告警，不驱动任何逻辑。

### 场张力

```
field_tension = Σ recent_impacts / |anchors|
```

- 高张力 → 近期事件大面积扰动 → 场处于"兴奋"状态
- 低张力 → 平静
- 骤升 → 用户突然连发高冲击事件

### 收敛度

```
convergence_rate = Σ cosine_distance(anchor_dir_current, anchor_dir_prev) / |anchors|
```

- 高速收敛 → 锚点大幅漂移 → 系统正在重塑
- 低速收敛 → 场接近平衡态

### 方向偏压

检测是否有某方向上的锚点集群在整体位移。

```
集体漂移 = 同向位移锚点数 / 总锚点数 > 阈值
```

### 快照

```
struct FieldSnapshot {
    timestamp, anchors[], tension, convergence, anisotropies[]
}
```

---

## 第8段：全局结构与公共 API

### 存储（四张表）

```
dse_store:
    anchors: Vec<AnchorKey>        # L1-L3 混合
    events:  Vec<Event>            # 事件流
    traces:  Vec<ImpactTrace>      # 松弛痕迹
    seeds:   Vec<SeedConcept>      # L4 种子
```

HNSW 可选（锚点 < 1000 时全量遍历）。持久化用 sled，四张表分别 bincode 序列化。

### DseEngine

```rust
pub struct DseEngine {
    anchors: Vec<AnchorKey>,
    events: Vec<Event>,
    traces: Vec<ImpactTrace>,
    seeds: Vec<SeedConcept>,
    embed: EmbedProvider,
    cycle: RelaxationCycle,
    ecg: CognitiveEcg,
    params: DseCoreParams,
}

/// 5 个核心参数
pub struct DseCoreParams {
    pub vector_dim: usize,              // 256
    pub event_window_secs: u64,         // 3600
    pub damping_base: f32,              // 0.5
    pub stiffness_base: f32,            // 1.0
    pub convergence_threshold: f32,     // 0.001
}
```

### 公共 API

```rust
impl DseEngine {
    // 生命周期
    pub fn new(params: DseCoreParams) -> Self;
    pub fn load(path: &Path) -> Result<Self>;
    pub fn save(&self, path: &Path) -> Result<()>;

    // 初始化
    pub fn init(&mut self, concept_descriptions: &[String]);
    pub fn inject_user_concept(&mut self, description: &str, initial_density: u32);

    // 写入
    pub fn on_user_input(&mut self, text: &str) -> EventId;

    // 演化（空闲期调用）
    pub fn relax(&mut self);

    // 召回（三层解码）
    pub fn value_init(&self) -> Vec<(&AnchorKey, f32)>;
    pub fn associate(&self, query: &str) -> Vec<(&AnchorKey, f32)>;
    pub fn recall(&self, query: &str) -> RecallResult;

    // 观测
    pub fn ecg_report(&self) -> EcgReport;
}
```

### 使用示例

```rust
let engine = DseEngine::new(DseCoreParams::default());

// 初始化
engine.init(&[
    "偏好 Rust 方案，对 Python 方案持怀疑态度",
    "极其看重长期语义一致性",
]);

// 使用
engine.on_user_input("proxy 项目又出 proxy 丢失的问题了");
engine.relax();
let recall = engine.recall("上次 apply_patch 的问题");
let ecg = engine.ecg_report();

engine.save(&path)?;
```

---

## 第9段：完整数据流

```
用户输入
    │
    ▼
embedding → direction
    │
    ▼
Event { direction, text, timestamp }    ← 一次性操作：不调用 LLM
    │
    ├──────────────────────────────────────┐
    │                                      │
    ▼                                      ▼
即时（不改变状态）：                 空闲期：
broadcast → anchors 算 impact      relax_cycle.run()
    │                                  │
    ├→ value_init: 读高密度锚点        ├→ 窗口内事件
    ├→ associate: impact top-N        ├→ 锚点累积扰动 → steps
    └→ recall: +事件回溯              ├→ 松弛迭代（push+pull）
                                       ├→ 密度更新 + ImpactTrace
                                       ├→ 范式转移检测
                                       └→ ECG 采样
```

---

## 第10段：新旧对账

### 删掉的

- gap、valence、emotion、concept_ids、recall_count、time_precision、anchor_pull
- WorkingLayer、EpisodicLayer 整层
- FastPipeline LLM 解构、SlowPipeline 云端核对
- compress、consolidate、suppress 等消化管线函数
- resolve_conflict、density_increment、allocate_magnitude 等显式分配函数
- 分层独立 recall、Spread Activation BFS、working_correction
- 时间坍缩、概念衰退、反刍变体生成、回忆加固
- RecallStamp、LocalDigest、RuminationLevel、Valence、Emotion、TimePrecision
- BehavioralSignal、SemanticVerifier、EcgAlert
- 16+ 散落配置参数

### 保留的

- Event 的三个字段（direction, text, timestamp）
- AnchorKey 的方向和密度
- 正交投影 + shadow_ptr（范式转移）
- 初始灌注（模型自生成 + 用户干预）
- embedding 模型（唯一外部依赖，只做方向）

### 新加入的

- 有效事件方向（层间引力的物理实现）
- ImpactTrace（松弛副作用，召回回溯用）
- RelaxationCycle（场唯一演化入口）
- stiffness + damping（由 density 推导的力学参数对）
- 5 个核心参数（全局收敛控制）

### 一句话

> **没有判断，没有分类。事件是石子投入水中，涟漪自会找到形状。**
