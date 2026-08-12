# field-memory v2 生产维度保真实验

Status: active
Owner: field-memory
Last updated: 2026-08-12
Scope: 在真实 BGE-M3 embedding 上测量降维造成的方向几何、邻域与静态读出损失；不实现或验证 v2 动力学
Related code: `experiments/v2_dimension_fidelity/`
Related docs: [v2 权威、分辨率与维度 ADR](../decisions/2026-08-12-v2-authority-resolution-and-dimension.md)、[v2 理论基石](../design/field-memory-v2-foundations.md)、[v2 动力学验证计划](2026-08-10-field-memory-v2-validation-plan.md)

## 1. 决策问题与边界

本实验回答：以当前真实 embedding 模型的 `native_embedding_reference_1024`
为基准时，field-memory 的生产方向空间最低可以压缩到多少环境维度，而仍保留
足够的角几何、局部邻域和静态相关性。

本实验不运行 DensitySite、Sample、transport、Response、scatter、Exp、自然
坍缩或外界反坍缩。它不证明 v2 物理正确，也不使用旧 feasibility harness 的
观察性 V2Field。本文严格区分三类空间：v2 动力学验证使用的
`physics_reference_s2`（严格 reference backend）；本实验的
`native_embedding_reference_1024`（BGE-M3 原生语义基准）；以及投影得到的
`semantic_candidate_d3` 等候选。`semantic_candidate_d3` 只是语义降维候选，
不是 \(S^2\) reference backend；它和其他 semantic candidate 接受完全相同的
保真门禁，不能因维度较低或 reference 易于计算而获得豁免。

## 2. 冻结输入

### 2.1 数据与 embedding

- 数据：`experiments/v2_feasibility/dataset_scale.json`；正式实验固定
  SHA-256 `ed5a9836f277ea8242fce0bd477363bb21e81d606226dc607b92554d7b7d9901`。
- 规模：6,611 memories、2,161 queries；实验只读取已冻结 JSON，不从当前
  dirty worktree 重建 project chunks。
- embedding：本机 Ollama `bge-m3:latest`，环境维度 1,024，模型 digest
  `7907646426070047a77226ac3e684fbbe8410524f7b4a74d02837e43f2146bab`。
- Ollama 版本：正式 run 固定为 `0.21.2`；变化时必须产生新的 experiment id，
  不得复用旧结论。
- 所有向量先验证有限且非零，再做 L2 归一化。正式 cache 必须同时校验数据
  SHA、模型名、模型 digest、环境维度和 schema；不匹配时显式失败。

### 2.2 Build/holdout 划分

projection 只能读取 Build memories，不能读取 query embedding。memory group
定义为：

- LoCoMo：`locomo:<source_sample>`；
- project：`project:<source_path>`。

设 `split_seed=20260812`，计算
`SHA256("<seed>:<group>") / 2^256`；值小于 `0.8` 的 group 进入 Build，
其余进入 heldout。所有 queries 都是 Active probe。heldout memory probe 按
`SHA256("probe:<seed>:<memory_id>")` 排序取前 512 条；不足时全部使用。

## 3. 冻结 projection family

### 3.1 候选生产映射：spherical PCA

对归一化 Build memory 矩阵 \(X\) 计算不减均值的二阶矩

\[
Q=X^TX,
\]

按特征值降序取前 \(d\) 个特征向量 \(W_d\)，并定义

\[
y_d(x)=\frac{xW_d}{\|xW_d\|_2}.
\]

不减均值使投影保持原点与纯方向语义。特征值按降序稳定排序，特征向量以
绝对值最大的坐标为 pivot 固定符号。对每个候选边界 (d)，必须满足
\(\lambda_{d-1}-\lambda_d>10^{-12}\max(\lambda_0,1)\)；否则该边界子空间不
唯一，整次实验作为无效 artifact 停止，而不是任取一个基。正式 artifact
保存完整 1,024 项特征谱、projection archive SHA 与按 metadata/dtype/shape/raw
bytes 计算的稳定 content SHA。只有这一 family 参与“最小生产候选维度”的
hard gate。

### 3.2 对照：Gaussian random projection

对 seeds `20260812, 20260813, 20260814` 生成标准高斯矩阵，取前 \(d\) 列，
投影后重新归一化。它只回答结果是否依赖 corpus-adaptive subspace，不参与
生产维度选择，也不能取三个 seed 中最好者冒充结果。为控制计算量，它只在
维度 `3,32,128,512` 上重复第 4.1 节的冻结全局 pair 指标，不运行全量邻域
或 ground-truth readout。

### 3.3 候选维度

正式维度为 `3,8,16,32,64,128,256,384,512,768`；1,024 维 identity 是
`native_embedding_reference_1024`，不是降维候选。任何缺失维度都会令 formal
artifact 无效。

## 4. 冻结观测量

### 4.1 全局角几何

formal run 的 100,000 个 pair 必须从 heldout memories 的全集中按 `pair_seed=20260812` 无放回抽取无序、非自身 pair；projection 只在 Build memories 上拟合，pair 评估不能把 Build 行重新混入以规避 holdout。artifact 保存 heldout pair index 序列的 SHA-256。

preflight 只作 smoke：固定从全部 memories 按同一规则抽取 2,000 个 unique unordered pair，用于检查 runner、cache 与指标链路，不作任何生产维度结论，也不能替代 formal heldout pair artifact。

上述 formal pair artifact 报告：

- cosine 绝对误差 mean、p50、p95、p99、max；
- angular 绝对误差 mean、p50、p95、p99、max，单位 rad；
- reference 与 projected cosine 的 Spearman 相关；
- 按 reference cosine 五分位分层的同类指标。

### 4.2 局部邻域

以 `native_embedding_reference_1024` 的 1,024 维 exact cosine 为语义基准：

- 所有 query 对 6,611 memories 的 native-neighbor recall@10、recall@50；
- 512 个 heldout memory probe 对全部 memories（排除自身）的 recall@10；
- native top-10 邻居 pair 的 angular error p95；
- query top-10 Jaccard 作为诊断，不单独 gate。

### 4.3 冻结相关性读出

对数据集中已有 `relevant_ids`，比较 projected 与 native identity 的 exact dense
cosine Recall@5。报告 overall、LoCoMo、project 三个群体的原始值与比值。
`relevant_ids` 按集合语义计算；冻结数据中同一 query 的重复 ID 只计一次，并在
artifact 记录被折叠的重复条目数。exact top-k 在相同 cosine 时按 memory 在冻结
输入中的索引升序打破平局。
这是静态 representation 检查，不是 RAG 或 v2 recall 验收。

## 5. 预注册 hard gate

每个 spherical-PCA 候选必须同时满足：

| 指标 | 通过条件 |
|---|---:|
| 非有限或零投影行 | 0 |
| global cosine absolute error p95 | `<= 0.05` |
| global cosine Spearman | `>= 0.98` |
| native top-10 pair angular error p95 | `<= 0.10 rad` |
| query native-neighbor recall@10 mean | `>= 0.90` |
| query native-neighbor recall@50 mean | `>= 0.95` |
| heldout-memory native-neighbor recall@10 mean | `>= 0.90` |
| Recall@5 / native Recall@5，overall | `>= 0.95` |
| Recall@5 / native Recall@5，LoCoMo | `>= 0.95` |
| Recall@5 / native Recall@5，project | `>= 0.95` |

选择规则是候选列表中通过全部 hard gate 的最小维度。没有候选通过时，结论
必须为 `no_compressed_production_candidate`；只表示在当前冻结 BGE-M3/corpus
下采用 `native_embedding_reference_1024` 的 provisional no-compression fallback，
不是永久接受的生产维度或 \(S^2\) reference backend。后续生产准入仍须由新的
冻结实验与 accepted implementation contract 共同确认。不允许看结果后调阈值、
增加 corpus 泄漏或把 random projection 最佳 seed 换入。

## 6. 受控执行

顺序固定：

1. `preflight`：LoCoMo/project 各按
   `SHA256("preflight-query:<query_id>")` 取 16 个 query，先纳入其全部
   `relevant_ids`，再按 `SHA256("preflight-memory:<memory_id>")` 补足 128
   memories；维度 `3,16,32,64`、一个 random seed；pair smoke 固定为全部
   可见 memories 中的 2,000 个 unique unordered pairs，仅验证执行链路；
2. 校验 preflight artifact；
3. `formal`：完整数据、全部维度和三个 random seeds；
4. 离线校验 formal artifact；
5. 才能撰写报告。

一次只运行一个 Python 进程，并设置：

```bash
OMP_NUM_THREADS=1
OPENBLAS_NUM_THREADS=1
VECLIB_MAXIMUM_THREADS=1
NUMEXPR_NUM_THREADS=1
```

preflight wall time 上限 10 min、RSS 2 GiB；formal wall time 上限 60 min、RSS
4 GiB、cache 与 artifact 合计不超过 1 GiB。任何 embedding/model/cache mismatch、
NaN、零向量、schema 错误、超时或资源超限都立即停止，不自动降规模或重试。

## 7. Artifact 与结论边界

formal JSON 至少记录：dataset/model/Ollama/git/code/cache SHA；Python、NumPy、
SciPy 版本；文本数、group split、probe IDs 与 pair indices hash；native reference
的 `identity` 声明、归一化契约与向量 hash；projection family、dimension、seed、
bundle hash、retained energy；全部指标、阈值、逐项 pass 与最终选择；每阶段
wall time 与进程资源峰值。

离线 validator 必须从保存的 metric 重新计算每项 gate 与最小维度，拒绝缺失
维度、最好 seed cherry-pick、`native_embedding_reference_1024` 非 identity 或
input provenance 不符。
最终报告必须使用下列措辞边界：

- `dimension fidelity passed` 只表示当前冻结 BGE-M3/corpus/mapping 下的有限
  representation 指标通过；
- 不表示 v2 transport、守恒、局部性、坍缩或动态平衡通过；
- corpus、embedding model digest 或 projection bundle 改变后必须重新实验。
