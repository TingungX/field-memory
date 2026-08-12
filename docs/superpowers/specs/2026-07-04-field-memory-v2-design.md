# field-memory v2 设计理念：场、密度峰、影响传播

Status: superseded
Owner: field-memory
Last updated: 2026-08-12
Scope: v2 思想演进历史稿；不作为当前 v2 实现规范或验收基线
Related code: 无；文中旧观察骨架不构成当前 v2 实现约束
Related docs: [当前 v2 理论基石](../../design/field-memory-v2-foundations.md)、[v2 权威、分辨率与维度 ADR](../../decisions/2026-08-12-v2-authority-resolution-and-dimension.md)
Superseded by: [当前 v2 理论基石](../../design/field-memory-v2-foundations.md)

> 本稿已 superseded，仅保留思想演进记录。文中的 anchor、固定/独立采样预算、
> 1024 维连续场及其他未被当前理论基石确认的表述，不得直接用于实现；v2 的
> 权威边界、K 派生尺度、S² reference 与生产维度实验以当前 ADR 和理论基石为准。

> Memory is not a database. It's a relaxation system.
> 记忆不是存储，是场的弛豫。场是唯一的本体。

本稿是对 field-memory 核心引擎的重新奠基。**不迁就现有框架**——`RelaxationCycle`、`paradigm.rs`、`push/pull`、`stiffness/damping`、`origin_direction`、显式合并/分裂逻辑，一律显式标废。一切从场本体重新推导。

---

## 0. 本体论

**场是唯一的本体。** 场是连续的势能分布，存在于方向空间（嵌入向量张成的空间）之中。

锚点和采样点都不是本体——它们是场在两个不同层面的**投影**：

- **锚点** = 场在**存储层**的离散投影。场无法被文件直接存储，所以用锚点群离散化它。
- **采样点** = 场在**交互层**的离散投影。场无法被扰动直接触碰（扰动是连续的，工程实现是离散的），所以用采样点群离散化交互界面。

两者互不知晓，只通过场间接耦合。它们不是两个独立的对象集合，是同一场的两副面孔。

---

## 1. 三层结构

### 1.1 锚点层（存储投影 / 慢 / 持久）

锚点是场的工程离散化，让场可存储、可恢复。

**锚点 = `{ id, label, direction, density }`**。仅此四字段。

- **direction** — 锚点在场中的方向。是慢变量，不是出生印记：平时不动，但会在场结构变化时跟随场缓慢更新（坍缩时的极端化、反坍缩时的回归）。这跟 v1「direction 几乎不动」不同——v2 里 direction 是活的慢变量。
- **density** — 锚点处的场密度。是锚点跟随场的**唯一主要方式**。density 增减 = 场在该处的势能增减。

**锚点没有以下逻辑（显式标废）**：

| 废弃项 | 理由 |
|---|---|
| `stiffness` / `damping` | 这是 v1 把锚点当弹簧质点的遗留。v2 锚点不 push/pull，无需弹性参数。 |
| `origin_direction` | direction 是慢变量而非出生印记，无需保留出生方向。 |
| 显式合并 `merge_anchors()` | 合并是判断，违背 no-judgment。方向近的锚点天然一起响应，合并只是观测表象，不是操作。 |
| 显式分裂 | 同上。锚点数量变化只通过 init 创建、density→0 移除。 |
| 范式转移 / `shadow_anchor` / `SeedConcept` | 见 §5。结构重组由影响传播自然涌现，不需要专门机制。 |

**锚点数量如何变化**：

- `init_field` 创建（场逐层沉积）。
- density → 0 时从场消失（移除）。
- 仅此两种。无合并、无分裂。

### 1.2 采样点层（交互投影 / 即时 / 不持久）

采样点是场在交互时的离散采样，是扰动与场交互的直接对象。

- 采样点群 = 连续场在若干方向上的离散化。
- **即时性**：采样点不进 sled，每次交互从当前场重新生成。场的状态完全由锚点群表达，采样点是运行时派生。
- **绝对密度由用户决定**（硬件预算：采样点总数），**相对密度由场决定**（采样点在场里的分布随场势能分布而变——势能高的区域采样点密，势能低的区域采样点疏）。

采样点"采样的是场，不是锚点"：单个采样点感知的是整个场在它那个位置的势能（所有锚点叠加），不归属于任何一个锚点。

**采样点在计算回路中的位置（关键）**：采样点不是扰动与锚点之间的可选中间抽象，是场作为交互对象这一面的**结构必需实现**。扰动不直接触碰锚点（存储面），也不先算连续场再反演（1024 维连续场不可计算）。扰动作用于采样点群（交互面），采样点响应结束后一次性反馈到锚点，场的改变即完成：

```
扰动 → 采样点群(交互投影) → 链式传播+响应 → 一次性反馈到锚点(存储投影) → 场已改变
```

锚点和扰动之间没有直接接触——它们只通过采样点间接耦合，这正是"采样点和锚点互不知晓，只通过场间接耦合"的精确实现。采样点层因此**不可删**：删了它，场就没有交互面，扰动无处可落。

### 1.3 调用层（编排）

扰动进 → 派发给采样点群 → 收集响应 → 一次性反馈到锚点 → 返回。

**强等价（读 = 写）**：不存在「仅输入」或「仅输出」。每一次 `perturb(query)` 既是写（扰动经采样点改变场）又是读（从采样点响应里提取返回）。记忆和回忆是同一次操作的两面。读出发生在反馈之前的采样点响应阶段，写发生在反馈步骤——但它们属于同一次 `perturb`，不可分离。

---

## 2. 时间

**时间由事件定义。** 事件来 = 时间前进一格。

- 坍缩/反坍缩是**每事件步**触发，不是每墙钟秒触发。
- v1 的 `event_window_secs`（墙钟时间窗）作废。时间离散单位是「事件步」，不是秒。
- 「记忆随时间衰退」的物理图像：长期不来事件的区域，每个事件步都坍缩一点，逐渐衰减。这是事件定义的时间，不是物理时间。

---

## 3. 坍缩 / 反坍缩（取代 RelaxationCycle）

v1 的 `RelaxationCycle::run()`（push/pull 改锚点方向）**整体作废**。`cycle.rs` 删除。

场唯一的演化机制是**坍缩 / 反坍缩**，发生在每个事件步。两者过程等同、源头不同。

### 3.1 公设：关联 = 角距离

> 两个场对象（center 与密度峰、或两个密度峰）的关联度，由它们的角距离唯一决定。距离近 = 关联强 = 收到的影响大。没有别的判据。

这是整个动力学的基础公设。它满足 no-classification：距离是纯几何量，不是分类标签。不定义「方向束」、不分组、不判断「是否同类」。

### 3.2 坍缩（默认，每事件步对每个锚点触发）

源头：场本身。每个锚点作为 center，对全场触发一次（即每个锚点都经历一次「以自己为中心的坍缩」）。

- **direction 往邻域场趋势极端化**：锚点 direction 朝其邻域（角距离近的其他锚点）density 加权的合成方向偏移、强化。长期不被扰动的锚点被邻域同化——这是「场把锚点同化」。
- **density 减小**：默认每个事件步衰减。
- 物理图像：长期不触及的锚点，被场的本底趋势吞噬，density 逐渐归零，从场消失。这是「遗忘」的自然实现。

### 3.3 反坍缩（被扰动触发）

源头：外部扰动（query / event）。扰动中心作为 center。

- **direction 往 center 方向转移**：锚点 direction 朝 center 方向偏移。可能是极端化（扰动方向与场趋势同向）或反极端化（扰动方向与场趋势反向）。
- **density 差序增大**：不均匀加，按扰动强度差序增大。近 center 的锚点 density 加得多。
- 物理图像：被扰动的区域反坍缩，采样点增多、锚点 density 上升、方向被扰动激活——这是「记忆强化」的自然实现。

### 3.4 坍缩与反坍缩的统一

两者都是「以某 center 为中心、向 center 方向极端化 + density 变化」。差别只在 center 的源头：

| | 坍缩 | 反坍缩 |
|---|---|---|
| center 源头 | 每个锚点自身（全场 N 个 center） | 外部扰动（1 个 center） |
| direction 转移 | 往邻域场趋势极端化 | 往 center 方向转移 |
| density 变化 | 减小 | 增大（差序） |

**实现载体**：坍缩/反坍缩的链式传播发生在采样点层（交互面），响应结束后一次性反馈到锚点（存储面）。§3 描述的「锚点 direction/density 变化」是反馈后的最终结果，不是锚点被直接操作。详见 §4。

---

## 4. 影响传播（核心计算图）

这是 v2 的理论核心。`perturb_around(center)` 的精确机制。

**载体是采样点，不是锚点。** 扰动触碰的是交互面（采样点群），不是存储面（锚点）。链式传播发生在采样点群上，采样点响应结束后一次性反馈到锚点。锚点不直接参与传播链——它们是被反馈更新的存储面。

### 4.1 链式传播，非广播

影响**不是**从 center 同时广播到所有采样点（并行汇聚）。影响是**链式传播**的：

1. 影响从 center 出发，先抵达角距离最近的采样点 s₁。
2. s₁ 吸收一部分影响（自身响应），削弱一部分（传给下一个）。
3. 削弱后的影响抵达下一个采样点 s₂。
4. s₂ 同样吸收、削弱、继续传播。
5. 链式传播，每个采样点对传播实时抑制。

**串行耦合**，不是并行汇聚。每个采样点响应的是「被前面所有采样点削弱过的影响」，不是同一个影响。

### 4.2 密度的双重作用（中继强度）

每个采样点的 density 决定两件事：

- **吸收量**：density 高的采样点吸收多 → 自身响应强（反坍缩明显）。
- **削弱量**：density 高的采样点削弱多 → 传给后面的影响衰减快（对传播实时抑制）。

采样点的 density 来自场——它是采样点所在位置的场势能（由锚点群叠加算出）。density 在 v2 里不是单标量，是「传播链上的中继强度」。这比 v1 的 `√density`（仅参与 impact 公式）信息密度高得多。

**密度作用域问题在此消解**：density 既不是「对单个锚点的效果」（per-anchor），也不是「对一束锚点的合成」（per-direction-group），而是**对传播过程本身的调制**（per-relay）。每个采样点的 density 决定它如何截获和放行影响。这比所有候选作用域都更本质。

### 4.3 Mexican hat 的物理来源

「在范围内增大、范围外减小」「随角度差递减、180° 归零」——这些不是独立的 Mexican hat 函数，是**传播衰减的自然涌现**：

- 近的采样点：收到影响强 → 反坍缩（density 增）。
- 远的采样点：影响传播衰减后变弱 → 不足以反坍缩 → 默认坍缩（density 减）。
- **范围** = 衰减到不足以反坍缩的角距离阈值。范围之内增大，范围之外减小。
- **180° 归零** = 影响传到反向采样点时已衰减光。

v2 不需要显式 Mexican hat 函数 f。差序效应是传播链的涌现属性。

### 4.4 精确计算图

```
perturb(query):
  # 1. 从当前场（锚点群）生成采样点群（即时投影）
  samples = sample_from_field(anchors, budget=用户给的绝对密度)

  # 2. 在采样点群上链式传播扰动
  center = { direction: query.direction, strength: query.strength }
  I = center.strength
  samples.sort_by(cos(center.direction, s.direction) 降序)

  for s in samples:
    received = I × cos(center.direction, s.direction)   # 方向相关性；反向归零
    if received < threshold: break                      # 范围之外，停止传播

    # s 的响应（反坍缩程度由 received 决定）
    s.response_density_delta = +g(received)             # 差序增大（记下，不立即写）
    s.response_direction_delta = 向 center.direction 极端化（步长 ∝ received / √s.density）

    # s 吸收并削弱影响（density 高的吸收多、削弱多）
    absorbed = h(s.density, received)
    I ← I − absorbed

  # 3. 未被波及的采样点执行默认坍缩响应
  for s in 未触及的 samples:
    s.response_density_delta = −默认衰减量
    s.response_direction_delta = 向邻域场趋势极端化

  # 4. 读出（强等价的读侧）——从采样点响应里提取返回
  readout = 取 response 最强的若干采样点 → 映射到附近锚点（label/direction/density）

  # 5. 一次性反馈到锚点（强等价的写侧）——采样点响应落回存储面
  for s in samples:
    将 s.response_density_delta 和 s.response_direction_delta 反馈到 s 所在区域附近的锚点

  # 采样点随之弃用（即时投影，扰动结束即消失；状态已落锚点）
  return readout
```

### 4.5 核心方程的重定义

v1：`impact(anchor, event) = cos_sim(anchor.direction, event.direction) × √anchor.density`

v2：impact 的定义从「anchor × event」重写为「center × sample」：

```
impact(center, s) = cos_sim(center.direction, s.direction) × center.strength
                   经传播链削弱后抵达 s 的量
```

`√density` 的形态保留，但落点改变：它现在描述 center 的强度（center.strength = √density），以及采样点作为中继的吸收/削弱能力（s.density 由场叠加算出）。核心方程的**形态不变、落点重写**——不增加参数（遵守 DseCoreParams 5 参数上限）。

### 4.6 工程代价

`perturb` 内部的链式传播是 O(K) 串行，无法并行：影响必须按角距离顺序遍历采样点，每个采样点的响应依赖前一个的削弱结果。这是链式传播的固有代价，不可消除。

外加两次 O(N) 遍历（N = 锚点数）：扰动开始时从锚点群生成采样点群，扰动结束时把采样点响应反馈到锚点。整体 O(N + K)。

---

## 5. 范式转移：作废，由传播涌现

v1 的 `paradigm.rs`、`shadow_anchor`、`SeedConcept`、正交化逻辑**整体作废**。

**理由**：v1 范式转移建立在「direction 被扰动推动、可反转」的前提上，依赖 `origin_direction` 复位。v2 里 direction 是慢变量、平时不反转，范式转移的全部机制失去物理基础。

**替代**：结构重组由影响传播自然涌现。

- 两个方向近、density 悬殊的锚点：高 density 的吸收影响多、削弱多，低 density 的收到被削弱的影响少 → 高 density 的反坍缩强化，低 density 的被同化坍缩 → 弱者自然衰减归零。
- 这是「概念被新范式吸收」的自然图像，不需要专门的范式转移机制。
- L4 dormant seeds / `shadow_anchor` 的「无损反转」能力**丧失**——v2 接受这个代价：场是单向弛豫系统，被吸收的概念不保留可逆影子。这是物理上的诚实（场没有记忆它的历史形态）。

---

## 6. Recall（读 = 写的读出侧）

v1 的三层 recall（value_init / associate / recall）**语义重审**：

recall 不再是独立的「查询路径」，而是 `perturb(query)` 的读出侧。同一个 perturb 既是写（扰动改变场）又是读（从响应里提取）。

**读出什么**：响应最强的若干采样点所在的场区域 → 这些区域附近的锚点（label、direction、density）。读出发生在反馈之前的采样点响应阶段，读出的是「被这次扰动激活的场结构」，不是独立的索引查找。

`ImpactTrace`（v1 的松弛副产物，用于回溯）**语义重审**：在 v2 里，传播链本身就是 trace——影响依次经过哪些采样点、被削弱多少、最终落在哪。这是天然的影响轨迹，无需单独的 trace 数据结构（待定：是否仍持久化，见 §7）。

---

## 7. 持久化

v1 sled 四张表：anchors / events / traces / seeds。

v2 变化：

| 表 | v2 状态 |
|---|---|
| `anchors` | 保留。结构简化为 `{id, label, direction, density}`。 |
| `events` | 保留。事件是场输入，仍需持久化（用于重放？待定）。 |
| `traces` | **重审**。传播链是运行时派生，是否持久化待定。倾向不持久化（场状态已由 anchors 完全表达，traces 是过程量）。 |
| `seeds` | **删除**。范式转移作废，无 seed 概念。 |

**采样点不持久化**：每次 load 后从当前场重新生成。场的持久化状态 = 锚点群，仅此。

**bincode 兼容性**：anchor 结构变化（删 stiffness/damping/origin_direction），需在 `persist.rs` 加 legacy fallback（v1 存储的状态加载时迁移）。

---

## 8. 与 v1 的对照

| 维度 | v1 | v2 |
|---|---|---|
| 本体 | 锚点是本体，场是叠加表象 | **场是本体**，锚点是存储投影 |
| 演化机制 | RelaxationCycle（push/pull 改 direction） | **坍缩/反坍缩**（影响传播改 density + 慢变 direction） |
| direction | 出生印记，几乎不动 | 慢变量，跟随场极端化/回归 |
| density 作用 | 仅 impact 公式里的 √density | **传播链中继强度**（吸收+削弱） |
| 影响方式 | 广播（每个锚点独立响应） | **链式传播**（顺序削弱） |
| 范式转移 | 专门机制 + shadow_anchor | **作废**，由传播涌现 |
| 合并 | （v1 未实现但概念上需要） | **不需要**，方向近天然一起响应 |
| 时间 | 墙钟秒（event_window_secs） | **事件步** |
| 读/写 | 分离的 recall / write 路径 | **强等价**，perturb 同时读写 |
| 采样点 | （v1 未明确） | **即时交互投影**，不持久化 |
| Mexican hat | （隐含在 impact） | **传播衰减涌现**，无独立函数 |

---

## 9. 不变的核心约束（来自 CLAUDE.md，仍遵守）

1. Memory is field state, not an entity's attribute.
2. The field is cognitive terrain, not a preference hashmap.
3. `impact = cos_sim × √density` 的**形态**不变，落点重写。不增加参数。
4. `DseCoreParams` 5 参数上限不变（`event_window_secs` 语义改为事件步，参数数不增）。
5. LLM 仅在 `init_field` 调用，运行时不调。
6. 场逐层沉积（多次 `init_from_descriptions`）。
7. No classification, no judgment——关联只由角距离决定，不分组、不分类、不判断。

---

## 10. 待澄清（下一步）

本稿是设计理念，非实现 spec。落地前需澄清：

1. **采样点的生成规则**：绝对密度（用户给总数）确定后，相对密度如何由场分布决定？势能高的区域采样点密——具体的撒点/重采样算法。
2. **坍缩的邻域场趋势**：邻域如何定义（角距离 K 近邻？）？density 加权合成方向的具体公式。
3. **传播阈值与衰减函数**：`threshold`、`g(received)`、`h(density, received)` 的具体形态。
4. **`perturb(query)` 的读出协议**：响应最强峰的选取、返回锚点的格式。
5. **events 表去留**：事件是否仍持久化、是否支持重放。
6. **bincode 迁移**：v1 → v2 anchor 结构的 legacy fallback 细节。

这些留给实现 spec（writing-plans 阶段）逐一落地。
