# ADR：接受 v2 当前生产语义维度 384

Status: accepted
Owner: field-memory
Last updated: 2026-08-12
Scope: v2 当前生产语义环境维度、投影身份与重新验证边界
Related code: `experiments/v2_dimension_fidelity/`
Related docs: [生产维度保真实验报告](../reports/2026-08-12-v2-dimension-fidelity.md)、[生产维度实验协议](../specs/2026-08-12-v2-dimension-fidelity-experiment.md)、[v2 权威、分辨率与维度 ADR](2026-08-12-v2-authority-resolution-and-dimension.md)、[v2 理论基石](../design/field-memory-v2-foundations.md)

## 背景

生产语义维度必须由真实 embedding 的独立保真实验决定，不能从
`physics_reference_s2`、旧实现或计算便利性推断。预注册 formal 实验已经在
冻结的 BGE-M3 表示、corpus、划分、spherical PCA、门禁与选择规则下完成；
384、512、768 通过，384 是候选网格中第一个通过的维度。

用户已明确接受 384 作为当前生产语义维度。

## 已接受决策

### 1. 当前生产语义环境维度为 384

后续 v2 accepted implementation contract 应把语义环境维度冻结为

\[
D_{\rm semantic}=384,
\qquad \mathcal D=S^{383}.
\]

实验 artifact 中的对象名仍可保留为 `semantic_candidate_d384`，用于和
`physics_reference_s2`、`native_embedding_reference_1024` 区分；“candidate”
描述实验空间中的对象类型，不再表示这项架构决定尚未接受。

### 2. 已接受的是完整表示身份，不是整数 384

本决定绑定以下已验证事实：

| 项目 | 冻结值 |
|---|---|
| source embedding | Ollama `bge-m3:latest`, 1,024D |
| model digest | `7907646426070047a77226ac3e684fbbe8410524f7b4a74d02837e43f2146bab` |
| dataset SHA-256 | `ed5a9836f277ea8242fce0bd477363bb21e81d606226dc607b92554d7b7d9901` |
| projection family | Build-only uncentered spherical PCA；投影后 L2 normalize |
| projection bundle archive SHA-256 | `2ba52a192937c9d4f8b6ab34980cf733539c9417af1b4db7c5c9ae636d80dd68` |
| projection bundle content SHA-256 | `17f2932d84ce59107e1f080ddd9279e30390fc6822f41a89f75b72a5cd8cbc11` |
| target projection | 上述 bundle 的前 384 个已冻结分量 |

不得用另一张随机矩阵、重新拟合的 PCA、不同模型 tag 内容或只因输出宽度同为
384 的其他 embedding 替代它。implementation contract 和持久化版本必须能识别
这组表示身份；不能只保存 `dimension = 384`。

### 3. 384 是候选网格最小值，不是逐整数数学下界

formal 候选网格从 256 跳到 384，因此本实验没有证明 257–383 全部失败。
当前架构采用预注册的版本化档位，故选择 384；若以后目标变成寻找逐整数最小
维度，必须另建 refinement protocol、运行新实验并以新 ADR 变更本决定。

### 4. 重新验证边界

以下任一事实改变，都不得沿用本 ADR 的保真结论：

- source embedding 模型或模型 digest；
- corpus、数据划分或表示用途；
- projection family、训练集合或 projection bundle；
- production kernel 对几何保真的要求；
- 已接受的门禁或误差预算。

发生变化时，应重新运行维度保真实验并用新 ADR 决定是否继续采用 384。
若没有压缩候选通过，`native_embedding_reference_1024` 是 provisional
no-compression fallback，不得降低门禁迁就低维。

## 非决策

本 ADR 只接受生产语义表示的维度与投影身份。它不接受尚未冻结的 density
kernel、分辨率算子 \(\mathcal G_K\)、Sample 构造、finite-volume 离散、readout
细节或长期动力学，也不解除 accepted implementation contract 缺失造成的实现
No-Go。`physics_reference_s2` 继续只作为严格动力学 reference backend。

## 后果

- implementation contract 必须引用本 ADR，并把 384D 表示身份写进版本与
  artifact schema；
- 生产持久化不得把不同 embedding/projection 身份的 EventCoordinate 混入同一场；
- 维度问题已经闭合，下一阶段只需讨论和冻结尚未接受的 v2 动力学实现契约；
- 在 implementation contract 被 accepted 前，仍不得开始 v2 动力学框架实现。

### 后续状态（2026-08-13）

上述 No-Go 是本 ADR 接受时的阶段门，不是永久禁止。独立
[`field-memory v2 实现契约`](../specs/field-memory-v2-implementation-contract.md)
现已 accepted；它完整引用并冻结本 ADR 的 384D identity，因此独立 v2 框架实现
已经解锁。此状态说明不改写本 ADR 当时的非决策边界：维度实验本身仍不证明
density、transport 或长期动力学正确。
