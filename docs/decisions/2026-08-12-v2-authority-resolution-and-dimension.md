# ADR：v2 权威、K 分辨率与生产维度

Status: accepted
Owner: field-memory
Last updated: 2026-08-12
Scope: v1/v2 权威边界、Sample 预算与场分辨率、reference/production 维度，以及已经接受的实现方向
Related code: 尚无 v2 实现
Related docs: [v2 理论基石](../design/field-memory-v2-foundations.md)、[v2 验证计划](../specs/2026-08-10-field-memory-v2-validation-plan.md)、[生产维度实验](../specs/2026-08-12-v2-dimension-fidelity-experiment.md)

## 背景

现有仓库的 v1 以 Anchor、`impact()` 与 `RelaxationCycle` 为核心；当前 v2
则以 EventCoordinate 对场的持久拟合、DensitySite/density 的即时重建以及
Sample-only 交互为核心。二者在状态、本体和动力学上不构成同一路径。

同时，旧理论把 DensitySite 角尺度 \(\ell\) 与 Sample 预算 \(K\) 当作两个
独立运行参数。这会允许同一预算表达任意细的结构，也让运行者在不改变场
版本的情况下悄悄改变物理分辨率。生产 embedding 的维度也尚无证据支持。

## 已接受决策

### 1. v2 是当前权威，v1 是冻结的独立实现

v2 不从 v1 的 Anchor/impact/RelaxationCycle 路径迁移或继承。现有 v1 仅作
维护；v2 必须使用独立模块、状态与持久化版本。v1 测试不能充当 v2 验收，
v2 也不得通过兼容层把 Anchor 重新变成场状态。

### 2. Sample 预算 K 定义场版本的物理分辨率

运行时不存在可独立传入的 DensitySite 尺度。对当前 EventCoordinate 几何
\(X_t\) 与固定预算 \(K\)，尺度由唯一、确定、旋转等变的分辨率算子导出：

\[
\ell_t^K=\mathcal G_K(X_t).
\]

其语义是在既定内禀 density 保真标准下，选择最多 \(K\) 个 Sample 能够
表达的最细尺度。概念形式为：

\[
\ell_t^K
=\inf\left\{\ell>0:\
N_{\rm req}\!\left(\mathcal D_\ell(\mathcal C_\ell(X_t)),
\varepsilon_{\rm repr}\right)\le K\right\}.
\]

\(\varepsilon_{\rm repr}\) 属于版本化表示契约，不是事件步中的可调物理
参数。实现契约必须冻结 \(\mathcal G_K\)、不可行边界和全部 tie-break。

因此 \(K\) 不是可以根据负载临时改变的硬件旋钮，而是场版本的一部分：

- 同一 Active 场内固定 \(K\)；
- 每相都从该相当前几何重新导出 \(\ell_t^K\)；
- 改变 \(K\) 是显式 resolution migration，会重建场投影；
- 不同 \(K\) 是不同分辨率的模型族，不能被描述为同一物理场上的纯数值细化；
- migration 不能倒推恢复已经在旧分辨率下丢失的历史几何。

### 3. Physics reference S² 与生产语义维度分离

v2 数学接口保持任意环境维度 \(D\)，方向空间为 \(S^{D-1}\)。
`physics_reference_s2` 对应三维环境向量，只用于具有解析几何和可严格审计
数值解的动力学 reference backend。真实 embedding 降到三维所得的
`semantic_candidate_d3` 虽然几何上也落在 \(S^2\)，却是另一种对象；它必须
接受和其他生产语义候选完全相同的保真门禁。低维 physics reference 上的
守恒、coverage 或动力学通过，不构成生产语义空间的保真证据。

降低生产语义维度不会破坏正确实现中的非负性、标量预算守恒或两相顺序；
这些是维度无关的结构不变量。但它可能系统性扭曲角距离、合并原本可区分的
邻域、改变 kernel density 与 coupling，继而改变 Sample 投影、Response 分布
和长期动力学。也就是说，低维的主要风险不是“方程不能运行”，而是系统严格
运行了一个已经失真的场。

生产维度必须通过冻结真实 embedding、投影方法、数据划分、指标、阈值与
artifact schema 的独立实验决定。没有候选维度通过时，结论必须是“不压缩”，
而不是降低阈值或把 `physics_reference_s2` 直接投入生产。

### 4. 已接受的实现方向

以下方向已经确认，但具体核函数、网格和数值算法仍须写入 accepted
implementation contract：

- density 与 Sample-to-geometry coupling 使用同一个归一化、旋转等变、
  紧支撑 kernel 导出，使结构质量的两侧边缘由同一表示自然闭合；
- 连续测地纯吸收模型是 reference，正式离散采用局部、守恒、反对称界面
  通量的 upwind finite-volume；
- 普适耦合尺度 \(\sigma\) 是场版本常量，以初始化参考均匀场从作用点到
  对点的平均光学厚度为 1 标定，运行期不得调节；
- readout 取外界相闭合后的非负标量 Sample Response，经同一 coupling 回到
  几何单位，再关联该单位的 Content；不得重算第二套 cosine 分数；
- 初始化区分 `Building` 与 `Active`。`Building` 可批量沉积初始 Event；进入
  `Active` 后，任何新增概念都必须走完整事件步；readiness probe 只读且
  必须验证吸收、coupling、Response 集中度与最大预测位移。

## 后果

实现者不能从 v1 代码猜测 v2，也不能把独立 \(\ell\) 暴露为运行配置。
维度实验先于生产 backend；`physics_reference_s2`、
`semantic_candidate_d3` 与 `native_embedding_reference_1024` 必须在 artifact
中明确区分。本文不批准尚未写出的具体 kernel profile、\(\mathcal G_K\)、
finite-volume mesh、容差或 CLI，它们仍由 implementation contract 门禁阻断。
