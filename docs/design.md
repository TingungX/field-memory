# DSE-Memory 架构设计文档

> Dynamic Semantic Evolutionary Memory — 动态语义演化记忆架构
> 2026-06-13

## 核心公理

概念（Key）的意义由其关联的事件（Value/经历）的空间密度分布动态赋予。

## 设计目标

抛弃传统的静态向量数据库（Vector DB）与 RAG 模式，构建一个与模型并行的独立认知状态机。纯外挂势能场 + 上下文欺骗协议，不碰模型参数。

## 关键决策记录

| 决策 | 选择 | 理由 |
|---|---|---|
| 路线 | 纯外挂势能场 + 上下文欺骗 | 闭源 API 限制，不碰模型参数 |
| 独立性 | 独立服务，不嵌入 llm-proxy | 项目隔离，DSE 是未来 agent 聊天软件的记忆内核 |
| 交付形态 | Rust library crate + CLI demo | 记忆消化是纯计算，做成 library 最干净 |
| Attention | 借鉴 QK^T → softmax(·/τ) → ×V 计算范式 | 不需要 Transformer 的 Embedding/多头/位置编码 |
| 事件定义 | 任何产生认知 Gap 的经历 | 不限于冲突，Gap = 预期与现实偏差 |
| 锚点涌现 | 从事件流自动创建，有生命周期 | 防止新概念无法进入系统 |
| 层间关系 | 投影关系，不是传递关系 | 召回时分层独立 Attention → 加权融合 |
| 解码方式 | 分层解码，不合并坐标系 | 不同层的向量物理意义不同，合并会互相污染 |
| Gap 判定 | 小模型语义理解，不硬编码 | 语言无限，关键词有限 |
| 写入方式 | 双管线（快:本地小模型 / 慢:云端LLM核对） | 实时性 + 准确性 |
| 记忆范围 | 全域（工程+生活） | 跨域关联是势能场的天然优势 |
| 反刍等级 | Light/Medium/Heavy 三档 | 云端核对定级，势能场约束模增长预算 |
| 初始灌注 | 模型自生成 + 用户干预 | 非白纸启动，三层免疫优先级 |

---

## 第1段：全局结构

### Crate 组织

```
dse-memory/
├── Cargo.toml                  # workspace
├── crates/
│   ├── dse-core/               # 核心库：数据模型 + 势能场引擎 + 召回引擎
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── model/          # 数据模型：Layer, Anchor, Event, Seed
│   │   │   ├── field/          # 势能场计算：密度、伸缩阻力、引力
│   │   │   ├── recall/         # 召回引擎：Attention、Spread Activation、高斯核
│   │   │   ├── digest/         # 事件消化：L1→L2 压缩、L2→L3 凝聚、L3→L4 投影
│   │   │   ├── init/           # 初始灌注：自生成 + 用户干预
│   │   │   ├── evolution/      # 演化动力学：回忆加固、时间坍缩、范式转移
│   │   │   └── monitor/        # 认知心电图：Loss 计算、加速度检测
│   │   └── Cargo.toml
│   │
│   ├── dse-store/              # 存储层：向量索引 + 持久化
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── index.rs        # HNSW 向量索引 + 倒排索引
│   │   │   ├── persist.rs      # 持久化（sled）
│   │   │   └── migration.rs    # 数据版本迁移
│   │   └── Cargo.toml
│   │
│   ├── dse-embed/              # 嵌入桥接：对接外部 embedding 服务
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   └── provider.rs     # 统一接口：OpenAI / 本地模型 / 自定义
│   │   └── Cargo.toml
│   │
│   └── dse-cli/                # CLI demo：验证整条链路
│       ├── src/
│       │   └── main.rs
│       └── Cargo.toml
│
├── docs/
│   └── design.md               # 本设计文档
└── tests/
    └── integration/
```

### 核心依赖

| 用途 | Crate | 原因 |
|---|---|---|
| 线性代数 | `nalgebra` | 成熟的 Rust 线性代数库 |
| 向量索引 | `hora` 或 `usearch` | HNSW 实现，O(log n) 召回 |
| 持久化 | `sled` | 嵌入式 KV 存储，纯 Rust，无外部依赖 |
| 序列化 | `serde` + `bincode` | 高性能二进制序列化 |
| 时间处理 | `chrono` | 时间戳、时间衰减计算 |
| 日志 | `tracing` | 结构化日志，认知心电图输出 |

### 公共类型约定

```rust
/// 所有向量统一为 f32 动态长度向量
type Vector = Vec<f32>;

/// 模长与方向分离
struct MagnitudeDirection {
    direction: Vector,  // 单位向量
    magnitude: f32,     // 模长
}

/// 时间戳精度等级（时间坍缩的量化）
enum TimePrecision {
    Second,     // L1: "2025-03-17 15:04:22"
    Hour,       // L2: "昨天下午"
    Day,        // L2: "上周三"
    Month,      // L2/L3: "去年三月"
    Year,       // L3: "2024年"
    Epoch,      // L3/L4: "很久以前"
    None,       // L4: 无时间
}
```

---

## 第2段：数据模型

### 核心设计原则

- 每层都有概念/事件/时间三个维度（4×3 矩阵结构）
- 向量的模长与方向分离
- 模长决定深刻性和影响力，方向决定语义指向
- 模长的均衡位由层决定：L1低、L3高、L4极低
- 伸缩阻力：偏离均衡位越远，增长/压缩越难

### 层级物理属性

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Layer {
    Working,    // L1
    Episodic,   // L2
    Semantic,   // L3
    Orthogonal, // L4
}

struct LayerPhysics {
    /// 模长均衡位
    equilibrium_magnitude: f32,
    /// 概念:事件 的均衡数量比
    concept_event_ratio: f32,
    /// 伸缩阻力系数 α
    stiffness_alpha: f32,
    /// 时间衰减系数 λ
    decay_lambda: f32,
    /// 高斯核宽度 σ
    recall_sigma: f32,
    /// Attention 温度 τ
    attention_tau: f32,
}

impl LayerPhysics {
    fn for_layer(layer: Layer) -> Self {
        match layer {
            Layer::Working => Self {
                equilibrium_magnitude: 0.5,
                concept_event_ratio: 1.0 / 50.0,
                stiffness_alpha: 2.0,
                decay_lambda: 0.1,
                recall_sigma: 0.3,
                attention_tau: 0.5,
            },
            Layer::Episodic => Self {
                equilibrium_magnitude: 2.0,
                concept_event_ratio: 1.0 / 5.0,
                stiffness_alpha: 1.5,
                decay_lambda: 0.01,
                recall_sigma: 0.5,
                attention_tau: 1.0,
            },
            Layer::Semantic => Self {
                equilibrium_magnitude: 10.0,
                concept_event_ratio: 5.0,
                stiffness_alpha: 3.0,
                decay_lambda: 0.001,
                recall_sigma: 0.8,
                attention_tau: 1.0,
            },
            Layer::Orthogonal => Self {
                equilibrium_magnitude: 0.01,
                concept_event_ratio: 1.0,
                stiffness_alpha: 5.0,
                decay_lambda: 0.0,
                recall_sigma: 2.0,
                attention_tau: 3.0,
            },
        }
    }
}
```

### L1 感觉/工作层

```rust
struct WorkingLayer {
    concepts: Vec<WorkingConcept>,
    events: Vec<WorkingEvent>,
    timeline: SessionTimeline,
}

struct WorkingConcept {
    id: ConceptId,
    label: String,
    md: MagnitudeDirection,
    physics: LayerPhysics,
    mention_count: u32,
}

struct WorkingEvent {
    id: EventId,
    text: String,
    tool_calls: Vec<ToolCall>,
    timestamp: DateTime<Utc>,
    concept_ids: Vec<ConceptId>,
}

struct SessionTimeline {
    session_id: SessionId,
    start_time: DateTime<Utc>,
    pivots: Vec<TimelinePivot>,
}

struct TimelinePivot {
    time: DateTime<Utc>,
    from_concept: ConceptId,
    to_concept: ConceptId,
}
```

### L2 情景记忆层

```rust
struct EpisodicLayer {
    concepts: Vec<EpisodicConcept>,
    events: Vec<EventSnapshot>,
    temporal: Vec<TemporalAnchor>,
}

struct EpisodicConcept {
    id: ConceptId,
    labels: Vec<String>,
    md: MagnitudeDirection,
    physics: LayerPhysics,
    event_ids: Vec<EventId>,
    promotion_score: f32,
}

struct EventSnapshot {
    id: EventId,
    concept_ids: Vec<ConceptId>,
    gap: f32,
    valence: Valence,
    context: String,
    emotion: Emotion,
    magnitude: f32,
    direction: Vector,
    original_timestamp: DateTime<Utc>,
    time_precision: TimePrecision,
    anchor_pull: f32,
    recall_count: u32,
}

struct TemporalAnchor {
    timestamp: DateTime<Utc>,
    precision: TimePrecision,
    concept_ids: Vec<ConceptId>,
    local_density: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Valence { Positive, Negative, Neutral }

#[derive(Debug, Clone, Copy, PartialEq)]
enum Emotion {
    Satisfied, Frustrated, Confused,
    Anxious, Angry, Sad, Happy,
    Touched, Disgusted, Nostalgic,
    Relieved, Lonely, Excited,
    Neutral,
}
```

### L3 语义/图景层

```rust
struct SemanticLayer {
    concepts: Vec<AnchorKey>,
    absorbed_events: Vec<AbsorbedEvent>,
    temporal: Vec<TrajectorySegment>,
}

struct AnchorKey {
    id: ConceptId,
    label: String,
    direction: Vector,
    magnitude: f32,
    physics: LayerPhysics,
    density: u32,
    valence_bias: f32,
    trajectory: Vec<TrajectoryPoint>,
    origin: AnchorOrigin,
    acceleration: f32,
    value_ids: Vec<EventId>,
    suppressed_seeds: Vec<SeedId>,
}

struct TrajectoryPoint {
    t: DateTime<Utc>,
    direction: Vector,
    magnitude: f32,
    density_snapshot: u32,
}

struct AbsorbedEvent {
    id: EventId,
    anchor_id: ConceptId,
    contributed_magnitude: f32,
    timestamp: DateTime<Utc>,
}

struct TrajectorySegment {
    anchor_id: ConceptId,
    start: TrajectoryPoint,
    end: TrajectoryPoint,
    curvature: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AnchorOrigin {
    InitUserExplicit,
    InitModelSynthesized,
    InitInferred,
    PromotedFromL2,
    ParadigmShiftRevived,
}
```

### L4 边缘子空间

```rust
struct OrthogonalLayer {
    concepts: Vec<SeedConcept>,
    events: Vec<SeedEvent>,
    temporal: Vec<DormantStamp>,
}

struct SeedConcept {
    id: SeedId,
    orthogonal_direction: Vector,
    shadow_ptr: AnchorSnapshot,
    original_magnitude: f32,
    physics: LayerPhysics,
    defeated_by: ConceptId,
    defeat_reason: String,
    activation_signal: f32,
}

struct SeedEvent {
    id: EventId,
    direction: Vector,
    shadow_ptr: EventId,
}

struct DormantStamp {
    seed_id: SeedId,
    dormant_since: DateTime<Utc>,
}

struct AnchorSnapshot {
    id: ConceptId,
    label: String,
    direction: Vector,
    magnitude: f32,
    density: u32,
    valence_bias: f32,
    trajectory: Vec<TrajectoryPoint>,
    value_ids: Vec<EventId>,
}
```

### 层间关联索引

```rust
struct DseIndex {
    anchor_hnsw: HnswGraph<ConceptId>,
    anchor_events: HashMap<ConceptId, Vec<EventId>>,
    event_anchors: HashMap<EventId, Vec<ConceptId>>,
    seeds: Vec<SeedConcept>,
    concept_lookup: HashMap<String, ConceptId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ConceptId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct EventId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SeedId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SessionId(u64);
```

---

## 第3段：势能场计算引擎

### 伸缩阻力

```rust
struct PotentialField {
    config: FieldConfig,
}

impl PotentialField {
    /// 伸缩阻力：偏离均衡位越远，阻力指数增长
    fn stiffness(&self, current_magnitude: f32, physics: &LayerPhysics) -> f32 {
        let deviation = (current_magnitude - physics.equilibrium_magnitude).abs();
        (physics.stiffness_alpha * deviation).exp()
    }

    /// 有效模长（时间衰减）
    fn effective_magnitude(&self, magnitude: f32, elapsed: Duration, physics: &LayerPhysics) -> f32 {
        let t = elapsed.as_secs() as f32 / 86400.0;
        magnitude * (-physics.decay_lambda * t).exp()
    }

    /// 模增长量（受伸缩阻力约束）
    fn constrained_growth(&self, current: f32, delta: f32, physics: &LayerPhysics) -> f32 {
        let resistance = self.stiffness(current + delta, physics);
        delta / resistance
    }

    /// 模压缩量（防抹除）
    fn constrained_shrink(&self, current: f32, delta: f32, physics: &LayerPhysics) -> f32 {
        let target = (current - delta).max(0.001);
        let resistance = self.stiffness(target, physics);
        delta / resistance
    }

    /// 密度增量 = f(gap)，受伸缩阻力约束
    fn density_increment(&self, gap: f32, current_density: u32, physics: &LayerPhysics) -> u32 {
        let base_increment = (gap * 2.0).ceil() as u32;
        let density_magnitude = current_density as f32 * 0.1;
        let resistance = self.stiffness(density_magnitude, physics);
        ((base_increment as f32) / resistance).ceil() as u32
    }

    /// 事件在各命中锚点间的模分配（引力竞争）
    fn allocate_magnitude(
        &self,
        event_magnitude: f32,
        anchors: &[(ConceptId, f32, u32)], // (id, cosine_sim, density)
    ) -> Vec<(ConceptId, f32)> {
        let scores: Vec<f32> = anchors.iter()
            .map(|(_, sim, density)| (*density as f32) * sim)
            .collect();
        let max_score = scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exp_sum: f32 = scores.iter().map(|s| (s - max_score).exp()).sum();
        scores.iter().zip(anchors.iter())
            .map(|(s, (id, _, _))| {
                let weight = (*s - max_score).exp() / exp_sum;
                (*id, event_magnitude * weight)
            })
            .collect()
    }
}
```

### 万有引力（层间传导）

```rust
impl PotentialField {
    /// L3 锚点对 L2 事件的引力权重
    fn anchor_gravity_on_event(&self, anchor: &AnchorKey, physics_l2: &LayerPhysics) -> f32 {
        let effective_mag = self.effective_magnitude(
            anchor.magnitude,
            Utc::now().signed_duration_since(anchor.trajectory.last().unwrap().t),
            &LayerPhysics::for_layer(Layer::Semantic),
        );
        effective_mag * (anchor.density as f32).powf(0.5)
    }

    /// L3 锚点对 L2 时间坍缩的拉住效应
    fn time_collapse_rate(&self, event_time: DateTime<Utc>, anchor: &AnchorKey, base_rate: f32) -> f32 {
        let trajectory_density = anchor.trajectory.iter()
            .filter(|tp| (event_time - tp.t).num_days().abs() < 30)
            .map(|tp| tp.density_snapshot as f32)
            .sum::<f32>();
        base_rate / (1.0 + trajectory_density)
    }

    /// L3 锚点对 L1 概念 salience 的影响
    fn anchor_influence_on_working(&self, anchor: &AnchorKey, current_salience: f32) -> f32 {
        let gravity = self.anchor_gravity_on_event(anchor, &LayerPhysics::for_layer(Layer::Working));
        current_salience + gravity * 0.1
    }
}
```

### 范式转移判定

```rust
impl PotentialField {
    /// 锚点加速度 = 轨迹弯折程度
    fn anchor_acceleration(&self, anchor: &AnchorKey) -> f32 {
        if anchor.trajectory.len() < 2 { return 0.0; }
        let n = anchor.trajectory.len();
        cosine_distance(&anchor.trajectory[n-2].direction, &anchor.trajectory[n-1].direction)
    }

    /// 种子激活信号累积
    fn accumulate_activation_signal(&self, seed: &mut SeedConcept, anchor: &AnchorKey, negative_feedback_weight: f32) {
        let drift = self.anchor_acceleration(anchor);
        seed.activation_signal += drift * negative_feedback_weight;
    }

    /// 判定范式转移
    fn should_paradigm_shift(&self, seed: &SeedConcept, dominant: &AnchorKey) -> bool {
        let seed_ready = seed.activation_signal >= self.config.seed_activation_threshold;
        let anchor_declining = if dominant.trajectory.len() >= 2 {
            let n = dominant.trajectory.len();
            dominant.trajectory[n-1].magnitude < dominant.trajectory[n-2].magnitude
        } else { false };
        seed_ready && anchor_declining
    }
}
```

### 冲突降解

```rust
impl PotentialField {
    fn resolve_conflict(&self, anchor_a: &AnchorKey, anchor_b: &AnchorKey, query: &Vector) -> ConflictResolution {
        let score_a = (anchor_a.density as f32) * cosine_similarity(query, &anchor_a.direction) * anchor_a.magnitude;
        let score_b = (anchor_b.density as f32) * cosine_similarity(query, &anchor_b.direction) * anchor_b.magnitude;
        let (winner, loser) = if score_a >= score_b { (anchor_a, anchor_b) } else { (anchor_b, anchor_a) };
        let new_seed = self.orthogonalize(loser, winner);
        ConflictResolution { winner: winner.id, loser: loser.id, new_seed }
    }

    /// Gram-Schmidt 正交化
    fn orthogonalize(&self, loser: &AnchorKey, winner: &AnchorKey) -> SeedConcept {
        let dot_lw = dot(&loser.direction, &winner.direction);
        let dot_ww = dot(&winner.direction, &winner.direction);
        let projection_scale = dot_lw / dot_ww;
        let orthogonal_direction: Vector = loser.direction.iter()
            .zip(winner.direction.iter())
            .map(|(l, w)| l - projection_scale * w)
            .collect();
        let norm = l2_norm(&orthogonal_direction);
        let normalized: Vector = orthogonal_direction.iter().map(|v| v / norm).collect();
        SeedConcept {
            id: SeedId::new(),
            orthogonal_direction: normalized,
            shadow_ptr: AnchorSnapshot::from(loser),
            original_magnitude: loser.magnitude,
            physics: LayerPhysics::for_layer(Layer::Orthogonal),
            defeated_by: winner.id,
            defeat_reason: format!("conflict: density {} vs {}", loser.density, winner.density),
            activation_signal: 0.0,
        }
    }
}
```

### 向量工具函数

```rust
fn cosine_similarity(a: &Vector, b: &Vector) -> f32 {
    let dot_ab: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a = l2_norm(a);
    let norm_b = l2_norm(b);
    if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot_ab / (norm_a * norm_b) }
}

fn cosine_distance(a: &Vector, b: &Vector) -> f32 { 1.0 - cosine_similarity(a, b) }
fn l2_norm(v: &Vector) -> f32 { v.iter().map(|x| x * x).sum::<f32>().sqrt() }
fn dot(a: &Vector, b: &Vector) -> f32 { a.iter().zip(b.iter()).map(|(x, y)| x * y).sum() }

fn softmax_with_temperature(scores: &[f32], tau: f32) -> Vec<f32> {
    let scaled: Vec<f32> = scores.iter().map(|s| s / tau).collect();
    let max = scaled.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = scaled.iter().map(|s| (s - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    exps.iter().map(|e| e / sum).collect()
}

fn gaussian_kernel(distance: f32, sigma: f32) -> f32 {
    (-distance * distance / (2.0 * sigma * sigma)).exp()
}

fn moving_avg_direction(current: &Vector, target: &Vector, rate: f32) -> Vector {
    current.iter().zip(target.iter()).map(|(c, t)| c + rate * (t - c)).collect()
}

fn normalize(v: &Vector) -> Vector {
    let norm = l2_norm(v);
    if norm == 0.0 { v.clone() } else { v.iter().map(|x| x / norm).collect() }
}
```

---

## 第4段：召回引擎

### 单层 Attention 召回

```rust
struct RecallEngine {
    field: PotentialField,
    index: DseIndex,
    config: RecallConfig,
}

struct RecallConfig {
    layer_weights: LayerWeights,
    spread_decay: f32,
    spread_max_hops: u32,
    spread_activation_threshold: f32,
    max_results: usize,
    working_exact_match_threshold: f32,
}

struct LayerWeights {
    episodic: f32,   // 0.3
    semantic: f32,   // 0.6
    orthogonal: f32, // 0.1
}

struct RecallItem {
    layer: Layer,
    concept_id: ConceptId,
    event_ids: Vec<EventId>,
    score: f32,
    mode: RecallMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RecallMode {
    Attention,
    SpreadActivation,
    WorkingCorrection,
}
```

### L3 召回（概念级，不展开事件）

```rust
impl RecallEngine {
    fn recall_semantic(&self, query: &Vector, store: &SemanticLayer) -> Vec<RecallItem> {
        let physics = LayerPhysics::for_layer(Layer::Semantic);
        let scores: Vec<(ConceptId, f32)> = store.concepts.iter()
            .map(|anchor| {
                let dist = cosine_distance(query, &anchor.direction);
                let proximity = gaussian_kernel(dist, physics.recall_sigma);
                let effective_mag = self.field.effective_magnitude(
                    anchor.magnitude,
                    self.time_since_last_trajectory_point(anchor),
                    &physics,
                );
                (anchor.id, effective_mag * proximity)
            })
            .filter(|(_, score)| *score > 0.01)
            .collect();
        let raw_scores: Vec<f32> = scores.iter().map(|(_, s)| *s).collect();
        let weights = softmax_with_temperature(&raw_scores, physics.attention_tau);
        scores.into_iter().zip(weights.iter())
            .map(|((id, _), &w)| RecallItem {
                layer: Layer::Semantic, concept_id: id, event_ids: vec![], score: w, mode: RecallMode::Attention,
            })
            .filter(|item| item.score > 0.01)
            .collect()
    }
}
```

### L2 召回（事件级，受 L3 引力加权）

```rust
impl RecallEngine {
    fn recall_episodic(&self, query: &Vector, store: &EpisodicLayer, semantic: &SemanticLayer) -> Vec<RecallItem> {
        let physics_l2 = LayerPhysics::for_layer(Layer::Episodic);
        store.events.iter()
            .map(|event| {
                let dist = cosine_distance(query, &event.direction);
                let proximity = gaussian_kernel(dist, physics_l2.recall_sigma);
                let effective_mag = self.field.effective_magnitude(event.magnitude, self.time_since_event(event), &physics_l2);
                let anchor_boost = event.concept_ids.iter()
                    .filter_map(|cid| semantic.concepts.iter().find(|a| a.id == *cid))
                    .map(|anchor| self.field.anchor_gravity_on_event(anchor, &physics_l2))
                    .fold(1.0, f32::max);
                (event, effective_mag * proximity * anchor_boost)
            })
            .filter(|(_, score)| *score > 0.005)
            .map(|(event, score)| RecallItem {
                layer: Layer::Episodic, concept_id: event.concept_ids[0], event_ids: vec![event.id], score, mode: RecallMode::Attention,
            })
            .collect()
    }
}
```

### L4 召回（暴力遍历，高门槛）

```rust
impl RecallEngine {
    fn recall_orthogonal(&self, query: &Vector, store: &OrthogonalLayer) -> Vec<RecallItem> {
        let physics_l4 = LayerPhysics::for_layer(Layer::Orthogonal);
        store.concepts.iter()
            .map(|seed| {
                let dist = cosine_distance(query, &seed.shadow_ptr.direction);
                let proximity = gaussian_kernel(dist, physics_l4.recall_sigma);
                (seed, seed.original_magnitude * proximity * seed.activation_signal)
            })
            .filter(|(_, score)| *score > 0.1)
            .map(|(seed, score)| RecallItem {
                layer: Layer::Orthogonal, concept_id: seed.shadow_ptr.id, event_ids: vec![], score, mode: RecallMode::Attention,
            })
            .collect()
    }
}
```

### Spread Activation 链式扩散

```rust
struct ActivationNode {
    concept_id: ConceptId,
    activation: f32,
    hops: u32,
    path: Vec<ConceptId>,
}

impl RecallEngine {
    fn spread_activation(&self, start: ConceptId, store: &DseStore) -> Vec<ActivationNode> {
        let mut visited: HashSet<ConceptId> = HashSet::new();
        let mut frontier: Vec<ActivationNode> = vec![ActivationNode {
            concept_id: start, activation: 1.0, hops: 0, path: vec![start],
        }];
        let mut result: Vec<ActivationNode> = vec![];
        while let Some(current) = frontier.pop() {
            if visited.contains(&current.concept_id) { continue; }
            visited.insert(current.concept_id);
            if current.activation < self.config.spread_activation_threshold { continue; }
            if current.hops >= self.config.spread_max_hops { continue; }
            result.push(current.clone());
            let neighbor_anchors = self.find_neighbor_anchors(current.concept_id, &store.index);
            for (neighbor_id, anchor) in neighbor_anchors {
                if visited.contains(&neighbor_id) { continue; }
                let dist = cosine_distance(&self.get_anchor_direction(current.concept_id, store), &anchor.direction);
                let proximity = gaussian_kernel(dist, 0.5);
                let next_activation = current.activation * anchor.magnitude * proximity * self.config.spread_decay;
                let mut path = current.path.clone();
                path.push(neighbor_id);
                frontier.push(ActivationNode { concept_id: neighbor_id, activation: next_activation, hops: current.hops + 1, path });
            }
            frontier.sort_by(|a, b| b.activation.partial_cmp(&a.activation).unwrap());
        }
        result
    }
}
```

### 三层联合解码

```rust
struct DecodedMemory {
    value_context: ValueContext,           // 被动层1
    associative_context: AssociativeContext, // 被动层2
    recall_context: Option<RecallContext>,   // 主动层
}

struct ValueContext {
    concept_vectors: Vec<ConceptVector>,
    valence_signals: Vec<ValenceSignal>,
}

struct AssociativeContext {
    triggered_concepts: Vec<TriggeredConcept>,
}

struct RecallContext {
    events: Vec<RecalledEvent>,
    trajectories: Vec<TrajectorySummary>,
    spread_paths: Vec<Vec<ConceptId>>,
}

impl RecallEngine {
    /// 被动层1：价值初始化（L3 概念方向 + 偏好）
    fn decode_value_context(&self, store: &DseStore) -> ValueContext { /* ... */ }

    /// 被动层2：即时关联召回（概念级，不展开事件）
    fn decode_associative_context(&self, user_input_concepts: &[String], store: &DseStore) -> AssociativeContext { /* ... */ }

    /// 主动层：recall_memory（完整事件 + 轨迹 + 链式扩散）
    fn decode_recall_context(&self, query: &Vector, store: &DseStore) -> RecallContext {
        let l2_items = self.recall_episodic(query, &store.episodic, &store.semantic);
        let l3_items = self.recall_semantic(query, &store.semantic);
        let l4_items = self.recall_orthogonal(query, &store.orthogonal);
        let w = &self.config.layer_weights;
        let all_items: Vec<RecallItem> = [
            l2_items.into_iter().map(|mut i| { i.score *= w.episodic; i }),
            l3_items.into_iter().map(|mut i| { i.score *= w.semantic; i }),
            l4_items.into_iter().map(|mut i| { i.score *= w.orthogonal; i }),
        ].concat();
        // Spread Activation + 组装结果 ...
    }

    /// L1 即时纠偏（精确匹配，不经过 Attention）
    fn working_correction(&self, current_action: &Vector, store: &WorkingLayer) -> Option<WorkingEvent> { /* ... */ }
}
```

---

## 第5段：事件消化管线

### L1 → L2 语义压缩

```rust
struct DigestPipeline {
    field: PotentialField,
    config: DigestConfig,
}

impl DigestPipeline {
    fn compress_working_to_episodic(&self, working: &WorkingLayer, episodic: &mut EpisodicLayer) -> Vec<EventId> {
        for event in &working.events {
            let direction = embed_event(&event.text);
            let gap = /* 小模型语义理解，不硬编码 */;
            let magnitude = self.field.constrained_growth(0.0, gap * 5.0, &LayerPhysics::for_layer(Layer::Episodic));
            let (valence, emotion) = /* 小模型判定 */;
            let snapshot = EventSnapshot { /* ... */ };
            let concept_ids = self.assign_episodic_concepts(&snapshot, episodic);
            episodic.events.push(snapshot);
        }
    }

    /// 锚点涌现：无法匹配已有概念时创建新 L2 概念
    fn assign_episodic_concepts(&self, snapshot: &EventSnapshot, episodic: &mut EpisodicLayer) -> Vec<ConceptId> {
        // 密度场自适应阈值：概念密集区域阈值更高
        let matches = /* 余弦相似度匹配 */;
        if matches.is_empty() {
            // 创建新 L2 概念（锚点涌现）
            let new_concept = EpisodicConcept { /* ... */ };
            episodic.concepts.push(new_concept);
        } else {
            // 匹配成功 → 关联 + 方向微调
        }
    }
}
```

### L2 → L3 密度凝聚

```rust
impl DigestPipeline {
    fn consolidate_episodic_to_semantic(&self, episodic: &EpisodicLayer, semantic: &mut SemanticLayer, index: &mut DseIndex) -> ConsolidationResult {
        for event in &episodic.events {
            let anchor_matches = /* 找最近锚点 */;
            if anchor_matches.is_empty() { continue; }
            let allocations = self.field.allocate_magnitude(event.magnitude, &anchors);
            for (anchor_id, allocated_mag) in &allocations {
                // 密度增量 + 模增长 + 方向微调 + 偏好偏移 + 轨迹记录
            }
        }
    }

    /// L2 概念晋升到 L3
    fn promote_episodic_concepts(&self, episodic: &mut EpisodicLayer, semantic: &mut SemanticLayer) -> Vec<ConceptId> {
        // 条件：被回忆N次 / 密度超阈值 / 单次显著Gap
    }
}
```

### L3 → L4 正交投影 / L4 → L3 范式转移

```rust
impl DigestPipeline {
    fn suppress_to_orthogonal(&self, conflict: ConflictResolution, semantic: &mut SemanticLayer, orthogonal: &mut OrthogonalLayer) { /* ... */ }
    fn paradigm_shift(&self, seed: &SeedConcept, dominant: &AnchorKey, orthogonal: &mut OrthogonalLayer, semantic: &mut SemanticLayer) -> ParadigmShiftResult {
        // 1. 通过 shadow_ptr 恢复种子为完整锚点（无损）
        // 2. 原主导推入 L4
        // 3. 恢复的锚点写入 L3
        // 4. 记录轨迹突变
    }
}
```

### 回忆加固 + 时间坍缩

```rust
impl DigestPipeline {
    fn reconsolidate(&self, recalled: &[RecallItem], query: &Vector, episodic: &mut EpisodicLayer, semantic: &mut SemanticLayer) {
        // L2: recall_count+1, 模增大, 时间精度锐化
        // L3: 密度+1, 模增大, 方向微调(rate=1/density), 偏好微调
    }

    fn time_collapse(&self, episodic: &mut EpisodicLayer, semantic: &SemanticLayer) {
        // 未被回忆的事件时间精度逐步模糊
        // 被 L3 锚点轨迹拉住的事件，坍缩更慢
        // 模越大 → 时间精度保持越久
    }
}

fn sharpen_precision(p: TimePrecision) -> TimePrecision {
    match p {
        TimePrecision::Epoch => TimePrecision::Year,
        TimePrecision::Year => TimePrecision::Month,
        TimePrecision::Month => TimePrecision::Day,
        TimePrecision::Day => TimePrecision::Hour,
        TimePrecision::Hour => TimePrecision::Second,
        TimePrecision::Second => TimePrecision::Second,
        TimePrecision::None => TimePrecision::Epoch,
    }
}
```

---

## 第6段：初始灌注

### 免疫优先级

| 来源 | 初始模长 | 特性 |
|---|---|---|
| 用户手写价值观 | 15.0 | 最高锚定，最难被后续事件推翻 |
| 模型自生成 | 8.0 | 中等锚定，可被足够多反面事件修正 |
| 历史推断 | 3.0 | 最低锚定，最容易演化 |

```rust
struct InitEngine {
    embed_provider: EmbedProvider,
    config: InitConfig,
}

impl InitEngine {
    fn initialize(&self, input: &InitInput, store: &mut DseStore) -> InitResult {
        // 1. 用户手写价值观 → 最高模锚点
        // 2. 模型自生成概念 → 中等模锚点 + 预建关联
        // 3. 历史推断偏好 → 低模锚点
        // 4. 建立HNSW索引 + 写入初始轨迹点
    }
}
```

### 模型自生成 Prompt 模板

```
你正在初始化一个动态语义演化记忆系统（DSE-Memory）。
系统将在未来的对话中持续学习用户偏好和价值观。
你需要基于你的对齐能力和以下用户背景，生成一份初始认知流形。

用户背景：{{USER_CONTEXT}}

生成 20-40 个核心概念，覆盖：
- 工程哲学（代码质量、架构、测试、重构等）
- 技术偏好（语言、框架、工具链等）
- 协作与沟通（反馈风格、文档习惯、辩论偏好等）
- 决策倾向（速度vs质量、保守vs激进、实用vs理想等）
- 生活节奏（作息、工作强度、休息方式等）
- 情绪模式（压力反应、愉悦触发、挫败恢复等）
- 人际偏好（社交风格、独处需求、信任建立方式等）
- 审美与表达（语言风格、幽默方式、叙述偏好等）
- 价值观底线（绝对不能接受的事、最看重的事等）

特别注意：生活领域的概念和工程领域的概念会产生跨域关联。
```

---

## 第7段：演化动力学

### 回忆加固

- 模增大（受伸缩阻力约束）
- 方向微调（rate = 1/density → 认知偏见的物理来源）
- 时间精度锐化
- 密度+1

### 虚拟反刍（分级）

| 等级 | 触发条件 | 变体来源 | 变体数 | 模增长预算 |
|---|---|---|---|---|
| None | score < 0.3 | — | 0 | 0 |
| Light | 0.3 ≤ score < 0.5 | 数学噪声 | 3-5 | 小 |
| Medium | 0.5 ≤ score < 0.75 | 数学 + 本地小模型 | 10-20 | 中 |
| Heavy | score ≥ 0.75 | 数学 + 小模型 + 云端LLM | 30-50 | 大（受势能场硬约束） |

反刍定级由云端核对 + 势能场共同决定。势能场约束模增长预算：当前模越大的锚点，反刍可分配的预算越少（防偏激）。

### 时间坍缩

```
effective_elapsed = actual_elapsed × anchor_pull / (1 + magnitude_preserve)
```

- 锚点拉住效应：关联的高密度锚点越多，坍缩越慢
- 模的保持系数：模越大，时间精度保持越久

### 范式转移

触发条件：种子 activation_signal 超过阈值 AND 主锚点有效模在衰减（持续的、趋势性的偏离，不是单次事件）。执行时通过 shadow_ptr 无损恢复。

### 演化周期主循环

```rust
impl EvolutionEngine {
    fn evolve_cycle(&self, store: &mut DseStore, monitor: &mut CognitiveEcg) -> EvolutionReport {
        // 1. 虚拟反刍（高Gap + 被回忆标记的事件）
        // 2. 时间坍缩
        // 3. 概念衰退扫描
        // 4. 范式转移检测 + 执行
        // 5. 种子激活信号累积
    }
}
```

---

## 第8段：认知心电图

### 三大核心 Loss

| Loss | 数据来源 | 异常表现 | 含义 |
|---|---|---|---|
| 即时纠错 Loss | L1 高Gap事件频率 | 骤升/高频波动 | 用户在疯狂打脸 |
| 价值观偏离 Loss | 最近事件方向 vs L3锚点重心 | 持续上升 | 模型在偏离主价值观 |
| 范式转移 Loss | 最强种子激活信号 / 对应主锚点有效模 | 骤升/骤降 | 范式转移即将发生/刚发生 |

```rust
struct CognitiveEcg {
    events: Vec<MonitorEvent>,
    metrics: EcgMetrics,
    snapshots: Vec<EcgSnapshot>,
    config: EcgConfig,
}

enum MonitorEvent {
    FastWrite { .. }, SlowReview { .. }, RuminationExecuted { .. },
    EventCompressed { .. }, EventConsolidated { .. }, ConceptPromoted { .. },
    AnchorEmergence { .. }, Reconsolidation { .. }, TimeCollapse { .. },
    ConceptDormant { .. }, AccelerationSpike { .. },
    ParadigmShiftDetected { .. }, ParadigmShiftExecuted { .. },
    ConflictResolved { .. }, RecallExecuted { .. }, SpreadActivation { .. },
    EvolutionCycleCompleted { .. },
}

enum EcgAlert {
    CorrectionLossSpike { current, baseline },
    DriftLossRising { slope, current, drifted_anchor },
    ParadigmShiftLossDrop { before, after },
    AccelerationAnomaly { anchor_label, acceleration },
}
```

---

## 第9段：DseEngine 总控 + 公共 API

```rust
pub struct DseEngine {
    store: DseStore,
    field: PotentialField,
    recall: RecallEngine,
    digest: DigestPipeline,
    init: InitEngine,
    evolution: EvolutionEngine,
    writer: WriteScheduler,
    ecg: CognitiveEcg,
    embed: EmbedProvider,
    config: DseConfig,
}

impl DseEngine {
    // 生命周期
    pub fn new(config: DseConfig) -> Self;
    pub fn load(config: DseConfig) -> Result<Self, DseError>;
    pub fn save(&self) -> Result<(), DseError>;

    // 初始化
    pub fn initialize(&mut self, user_statements: Vec<UserStatement>, user_context: &str) -> InitResult;
    pub fn inject_user_statement(&mut self, statement: UserStatement) -> ConceptId;

    // 写入（双管线）
    pub fn on_user_input(&mut self, user_text: &str, tool_calls: &[ToolCall]) -> EventId;
    pub fn process_pending_reviews(&mut self);
    pub fn on_session_end(&mut self);

    // 召回（三层解码）
    pub fn decode_value_context(&self) -> ValueContext;
    pub fn decode_associative_context(&self, user_input_concepts: &[String]) -> AssociativeContext;
    pub fn decode_recall_context(&self, query: &Vector) -> RecallContext;
    pub fn recall_memory(&self, query_text: &str) -> RecallContext;
    pub fn working_correction(&self, current_action: &Vector) -> Option<WorkingEvent>;
    pub fn decode(&self, query: &Vector) -> DecodedMemory;

    // 演化
    pub fn evolve(&mut self) -> EvolutionReport;

    // 监控
    pub fn ecg_report(&self) -> EcgReport;
    pub fn ecg_timeseries(&self) -> EcgTimeseries;
}
```

### 使用示例

```rust
let config = DseConfig::default();
let mut engine = DseEngine::new(config);

// 初始化
engine.initialize(
    vec![UserStatement { text: "我极其看重代码的长期语义一致性".into(), weight: 1.0, valence: Valence::Positive }],
    "一个注重代码质量的工程师",
);

// Session
let value_ctx = engine.decode_value_context();
engine.on_user_input("帮我用 Python 写一个快速脚本", &[]);
let assoc = engine.decode_associative_context(&["Python".into()]);
let recall = engine.recall_memory("快速脚本的最佳语言选择");

// 空闲期
engine.process_pending_reviews();
let report = engine.evolve();
let ecg = engine.ecg_report();

// 结束
engine.on_session_end();
engine.save()?;
```

---

## 第10段：完整架构总览 + 待办

### 数据流全景

```
用户输入 → FastPipeline(本地小模型) → L1写入 + 回忆戳 + 即时回忆
                ↓ (低置信度/高Gap)
         SlowPipeline(云端LLM) → 核对修正 + 反刍定级
                ↓
         反刍执行(按等级, 受势能场约束)
                ↓
         消化管线 L1→L2→L3→L4
                ↓
         认知心电图 (三大Loss + 快照 + 告警)
                ↓
         持久化 (sled)

三层解码:
  被动层1: decode_value_context()  → L3概念向量 + 偏好
  被动层2: decode_associative_context() → L3+L2概念联想
  主动层:  recall_memory()         → L2事件 + L3轨迹 + 链式扩散
```

### 设计原则总结

| 原则 | 实现方式 |
|---|---|
| 模-方向分离 | 所有向量存储为 (direction, magnitude)，衰减只作用于模 |
| 指数伸缩阻力 | stiffness = exp(α · \|current - equilibrium\|)，防抹除 + 防偏激 |
| 密度即偏好 | 冲突降解时密度大者胜出 |
| 势能场统一 | 模长、数量、时间精度三个约束都由同一势能场驱动 |
| 层间引力传导 | L3 → L2 → L1，上层概念向下施加万有引力 |
| 分层解码 | 被动1/被动2/主动，各层独立 τ/σ |
| 双管线写入 | 快管线(本地小模型) + 慢管线(云端LLM核对+反刍定级) |
| 小模型驱动 | Gap/回忆戳/即时回忆不硬编码，由小模型语义理解决定 |
| 锚点涌现 | 概念锚点从事件流自动创建，有生命周期 |
| 非白纸启动 | 模型自生成 + 用户干预，三层免疫优先级 |
| 全域记忆 | 工程域 + 生活域，跨域关联是势能场天然优势 |
| 反刍分级 | Light/Medium/Heavy，云端核对定级，势能场约束模增长预算 |
| 无损范式转移 | shadow_ptr 保留完整锚点快照，L4→L3 恢复无损 |

### 待办事项

**Phase 1 — 核心闭环（验证核心假设）**

- [ ] `dse-core` 基础数据模型实现
- [ ] `PotentialField` 势能场计算
- [ ] `RecallEngine` 单层 Attention 召回 (先做 L3)
- [ ] `dse-embed` 对接至少一个 embedding provider
- [ ] `dse-cli` 最小 demo：初始化 → 写入 → 召回 → 验证模型行为是否真的变了

**Phase 2 — 完整管线**

- [ ] `DigestPipeline` L1→L2→L3→L4 完整消化
- [ ] `FastPipeline` 本地小模型事件解构
- [ ] `RecallEngine` 三层解码 + Spread Activation
- [ ] 锚点涌现 + 晋升 + 衰退
- [ ] 回忆加固

**Phase 3 — 高级特性**

- [ ] `SlowPipeline` 云端 LLM 核对 + 反刍定级
- [ ] 虚拟反刍（三档等级）
- [ ] 范式转移检测 + 执行
- [ ] `CognitiveEcg` 三大 Loss + 快照 + 告警
- [ ] 时间坍缩 + 锚点轨迹

**Phase 4 — 工程化**

- [ ] `dse-store` 持久化 (sled) + 增量保存
- [ ] HNSW 向量索引集成
- [ ] 数据版本迁移
- [ ] 性能基准测试
- [ ] 并发安全（异步演化 vs 同步召回）

**待验证的核心假设**

- [ ] Pre-fill 注入是否真的能"扭转潜意识"
- [ ] 小模型 Gap 估计的准确性
- [ ] 势能场参数（α、λ、σ、τ）的合理初始值
- [ ] 反刍的实际效果 vs 数学数据增强的替代效果
- [ ] 范式转移在实际使用中是否真的会发生

