# field-memory v2 验证计划：从不变量到大场动力学

Status: active
Owner: field-memory
Last updated: 2026-08-13
Scope: v2 的 DensitySite、density、Sample、守恒运输、Response 反馈与事件步验证；不验证旧 v1 引擎或 retrieval 质量
Related code: 独立 `crates/core/field-mem-v2`、`crates/ext-v2-cli` 及其测试；当前实现边界见 Phase 0 handoff
Related docs: [v2 实现契约](field-memory-v2-implementation-contract.md)、[v2 Phase 0 handoff](2026-08-13-v2-phase0-handoff.md)、[v2 理论基石](../design/field-memory-v2-foundations.md)、[v2 权威、分辨率与维度 ADR](../decisions/2026-08-12-v2-authority-resolution-and-dimension.md)、[生产语义维度 384 ADR](../decisions/2026-08-12-v2-semantic-dimension-384.md)、[Sample 投影、覆盖责任与 backend 边界 ADR](../decisions/2026-08-13-v2-sample-projection-and-backend-boundary.md)、[生产维度保真实验](2026-08-12-v2-dimension-fidelity-experiment.md)、[v2 可行性规模验证报告](../reports/2026-08-09-v2-feasibility.md)、[v2 设计草案](../superpowers/specs/2026-07-04-field-memory-v2-design.md)

## 1. 目的与边界

本计划验证的是 v2 作为一个守恒、局部、可随事件长期演化的**大场动力学**，而不是把它同传统 RAG 做第二次读取质量比较。现有可行性报告已证明旧观察骨架在 6,611 条 memory 上存在容量和动力学问题；它不能作为本计划的实现基线或验收替身。

本计划只接受理论基石已经确立的因果链：

\[
EventCoordinate \to DensitySite \to Density \to Sample \to Response \to EventCoordinate.
\]

尤其不允许为了让测试通过而让外部扰动、DensitySite、原始 density、CoordinateLog 或 EventContent 直接改写 EventCoordinate。测试中的 `physics_reference_s2` backend 固定使用 \(S^2\) 与合成 density；这不是将三维环境向量当作生产场，而是连续几何、守恒运输与细化趋势的可审计 reference。生产维度必须先通过独立真实 embedding 保真实验，不能由本计划的物理测试倒推。

`semantic384` 则是当前冻结投影上的有限 density-derived Sample graph operational
model：它验证因果链、账本、gauge-observable、持久化和事件步边界，不把固定 \(K\)
的结果报告为 \(S^{383}\) continuum convergence。本计划只定义 v2 动力学验证方案，
不把任何待实现的离散公式伪装成已确认理论。生产维度实验使用独立 runner、artifact
与结论，不混入 Phase 0–4。

## 2. 术语、观测量与通过规则

### 2.1 四个不可混用的规模量

每份结果 JSON、每条阶段摘要和每个失败报告必须同时记录下列四类量；禁止用其中一个代替另一个。

| 量 | 记号 | 含义 | 不是什么 |
|---|---:|---|---|
| DensitySite 数 | \(N_{site}=|C_{t,K}|\) | 当前几何在 \((K,X_t)\) 派生尺度下的可区分单位数 | 独立配置、Event 行数或 Content 重数 |
| Sample 预算 | \(K\) | 场版本的物理分辨率上限；实际 \(|S_{t,K}|\le K\) | 可按负载改变的硬件旋钮、绝对 density 或参与单位数 |
| Physical Sample 结构权重 | \(w_{src,j}=m_j/\mathcal M\) | Physical Sample \(j\) 在本相冻结场中代表的绝对结构份额，\(\mathcal M=\sum_{j\in\mathcal P}m_j\)；规范化当前场中 \(\mathcal M=N_{site}\) | Carrier 权重、Response 回分配份额或 Event 行数 |
| 有效参与度 | \(N_{eff}=1/\sum_i r_i^2\) | 本次 closed Response 回分配到几何单位的集中程度，\(r_i=a_i/\sum_k a_k\) | raw Event 数、K、Nsite 或 source weight |

`w_src` 是前向 density-to-Physical-Sample 投影的结构权重，`r_i` 是吸收后由
Physical Sample Response 回分配得到的闭合 Response 份额；两者必须并列记录，绝不
能互换。`w_src` 可在无吸收时存在；若 \(\sum_i a_i=0\)，则结果必须写
`N_eff: null` 并注明“无吸收/无参与”，不得填零或无穷大。对于带有 signed
direction-moment 的阶段，`r_i` 只使用非负标量吸收量 \(a_i\)，不使用向量范数或
带符号分量。

每个 SampleField 还必须显式区分以下不属于“规模量”的两个责任：

- transport coverage \(\phi\) 在全体 \(\mathcal T=\mathcal P\cup\mathcal C\) 上构成
  partition of unity，定义承运 volume 与 graph；
- physical responsibility \(\chi\) 只在 Physical Sample 集 \(\mathcal P\) 上定义，
  定义 density、mass、吸收、Response 与 scatter coupling。Carrier 集 \(\mathcal C\)
  只有 \(\phi\)，严格为零 physical mass，不是低 density 或 background mass。

任何 artifact 若只写一个未标注责任的 `coverage`/`volume`/`density` 字段，均不足以
证明 v2c 的表示边界；必须标明 `transport_*` 或 `physical_*`。

### 2.2 数值判定

实现必须集中定义一组与浮点类型匹配的容差，写入每个 artifact 的 `tolerances` 字段；测试体不得各自散落 magic number。建议使用：

- 单位向量、coverage 和标量预算的绝对误差 `ε_abs`；
- 旋转、平行运输和坐标位移的角误差 `ε_angle`；
- 连续参考解/细化趋势的相对误差 `ε_rel`；
- 所有比较同时给出实测误差、阈值和最大误差所在 fixture/sample。

在实现选择 `f64` 前，计划默认用 `f64`；如果核心改用 `f32`，只可整体调整并记录容差推导，不能逐例放宽阈值。

除明确标为“诊断趋势”的大规模观测外，性质与机制测试均为 hard pass：任何一次超过阈值即失败。只有已经由理论给出上下界或单调性的量才能成为趋势门槛；其余斜率只报告、不据此判失败。禁止查看结果后再挑选子集或修改基准。

### 2.3 需要的标准 fixtures

固定 seed 生成并版本化以下 fixture family；每个 fixture 以名称、维度、\(K\)、坐标、density 参数和 seed 表示，并记录由 \(\mathcal G_K\) 得到的 `derived_scale`，不得把它作为输入参数。每个 fixture object 还必须显式包含 `backend_kind`、`space_kind` 与 `embedding_identity`；当前全部为 `physics_reference` / `physics_reference_s2` / `null`，包括 direct 与 analytic fixture，禁止由 runner 根据 origin 猜测或补全这些身份。

| family | 用途 |
|---|---|
| `empty` | 无 Event 的初始化边界；不得进入正常 Sample 生成或事件步 |
| `vacuum_transport` | 直接在 Sample 层构造的零 physical density 诊断场；可保留 Carrier transport coverage，只验证 raw transport，不代表 \(\rho=Mp\) 的正常场 |
| `uniform_ray_oracle` | `origin=analytic_reference` 的 S2 连续 ray oracle；固定 \(\rho=1\)、\(\sigma=1/\pi\)、source \(q\) 与切向 ray directions，验证整条角路径 residual \(e^{-1}\)；不构造 state/Sample graph，也不进入 readiness、activation 或 persistence |
| `single_site` | 单一 density 结构、均匀路径的指数吸收参考 |
| `symmetric_pair` / `antipodal` | 对称吸收、零一阶方向矩、对点边界 |
| `uniform_shell` | 常 density、精确细分不变性与 K 检验 |
| `local_cluster` | 作用点附近高 density、远区低 density的局部性 |
| `rotated_copy` | 对每个非平凡 fixture 施加固定正交旋转后的副本 |
| `large_balanced` | K=64 至 1,024 的分辨率版本；实际结构单位数由 \(\mathcal G_K\) 与当前几何导出，用于 \(N_{eff}\) 与大场位移律 |
| `large_skewed` | 同一 K 版本、输入几何与派生 Nsite，但 Response 集中，用来证明 Nsite 或 K 不可替代 \(N_{eff}\) |

随机 fixture 只能补充这些确定性 fixture，不能替代它们。所有随机测试使用同一 PRNG 算法、固定 `FM_V2_SEED` 和显式派生子 seed；失败时打印可单独重放的 seed。

`uniform_ray_oracle` 的已提交 shape 是 `dimension=3`、
`backend_kind="physics_reference"`、`space_kind="physics_reference_s2"`、
`embedding_identity=null`、`K=null`、`density={kind:"uniform", value:1}`、
`sigma=1/pi`、`source={q, initial_total_influence}` 与逐条物化的
`rays[]={ray_id, omega, normalized_angular_weight,
expected_path_length_rad, expected_optical_thickness, expected_raw_residual}`。
四条 ray 的 normalized angular weight 都是 `0.25`；每条独立的 residual 为
\(e^{-1}\)，而 `ray_aggregate` 的离散加权 residual
\(\sum_r w_r\,residual_r\) 也必须为 \(e^{-1}\)。它只向 S2 continuous ray
reference 提供机械 aggregate diagnostic；没有 Event、DensitySite、physical Sample
或 finite graph，不能被 runner 升格为 state fixture 或任一 lifecycle/persistence
输入。

## 3. 受控执行协议

### 3.1 前置条件与实现就绪门禁

截至 2026-08-13，accepted contract 已解锁独立 v2 实现，代码检查点
`0aa9b45228ff60a8de2cbbf8b21c477b82eb3feb` 已完成 numeric/geometry/version/
event/persistence/kernel 与 geometry-only resolution primitive，并通过 33 个串行 lib
tests 及 strict clippy。Sample projection、\(\phi/\chi\)、transport、Response、两相
step、CLI 命令和 artifact runner 尚未实现，所以 **Phase 0 仍未通过**。精确完成边界、
已知风险和续接顺序以 [Phase 0 handoff](2026-08-13-v2-phase0-handoff.md) 为准；任何
实施者不得把该 primitive checkpoint 当成阶段解锁 artifact。

implementation contract 的 artifact schema 还必须同时冻结 `backend_kind`、`space_kind`、embedding/projection provenance 与生产/参考空间互斥校验。

本文是验证计划，不是可直接执行的 implementation handoff。开始编写 v2 框架前，固定路径 `docs/specs/field-memory-v2-implementation-contract.md` 必须存在且标记为 `Status: accepted`，并至少冻结：独立 v2 crate/module/file manifest 与持久化版本；`f64`/生产维度/单位；\(\mathcal G_K\)、\(\mathcal C_{\ell_t^K}\)、physical kernel density、\(\mathcal P_K\) 的唯一算法及 tie-break；transport coverage \(\phi\)、physical responsibility \(\chi\)、两类 volume/quadrature/coupling 与 Carrier 零质量规则；唯一离散 transport、edge-carried moment 与连续 reference；readout 与 `Building | Active` readiness probe；版本化 fixture 数据与全部容差；artifact schema、runner target 和精确命令。不得重新讨论 v1/v2 是否共用路径，也不得暴露独立 runtime \(\ell\)。

若该 accepted 契约不存在，任何实施代理——无论能力强弱——都必须返回 `blocked:implementation-contract-missing`。若文件存在但 status 不正确、任一必需章节/数值/schema/命令缺失，或者契约内部与本理论、验证计划冲突，则返回 `blocked:implementation-contract-incomplete`。此时只允许搭建不含算法选择的数学 primitive、账本数据类型或验证器骨架；不得自行选默认公式、用 TODO/ignore 把阶段伪装成完成，或宣布 Phase 0 已可运行。架构选择不能由实施者写进“测试 README”后自行批准。

在开始 Phase 0 前，实施者必须从 accepted contract 逐项核对并原样镜像到测试 README：K-to-scale 分辨率算子、DensitySite 构造器、physical kernel density、\(\phi\)/\(\chi\) Sample 生成器、两类 volume、Carrier 零物理质量、同核 physical coupling、edge-carried moment 的离散运输/吸收实现、Response 回分配，以及自然坍缩与外界反坍缩的 phase contract；同时记录契约文件的 SHA-256。实施者不得新增、修改或“合理化”其中的选择。外界 readout 在外界 Response 形成后、该相 scatter/Exp 反馈前完成；两相更新结束后，才在外界作用点 \(q\) 原位 append Event。若任一项尚未决定或测试 README 与契约 hash/内容不一致，必须按上述 incomplete 状态停止；不得用临时默认参数跨阶段推进。

生产 backend 必须同时引用 accepted 的
[生产语义维度 384 ADR](../decisions/2026-08-12-v2-semantic-dimension-384.md) 及其通过
[生产维度保真实验](2026-08-12-v2-dimension-fidelity-experiment.md) 校验的 formal
artifact，并原样冻结 384D 目标、embedding model digest、projection family 与
bundle content hash。formal artifact 通过只提供实验证据，不能代替用户接受的
架构决定；同为 384D 的其他映射也不得借用该结论。未来若新实验没有压缩候选
通过，只能实现 `physics_reference_s2` backend，不能把它标为生产语义空间。
所有 artifact schema 还必须包含 `backend_kind` 与 `space_kind`：物理验证固定为 `backend_kind=physics_reference`、`space_kind=physics_reference_s2`；语义生产 artifact 必须记录 embedding/projection provenance，不能用缺少这些字段的 \(S^2\) artifact 冒充生产空间。

测试 harness 必须提供以下稳定接口（名称可按 Rust 模块风格调整，但语义不可缺失）：

```text
derive_resolution(events, K) -> DerivedResolution
build_sites(events, derived_resolution) -> DensitySites
fit_density(sites, derived_resolution) -> Density
project_samples(density, K) -> SampleField
run_transport(sample_field, source) -> TransportResult
apply_event_step(state, external_event?) -> EventStepResult
```

每个**单源** `TransportResult` 至少暴露冻结 SampleField、raw 预算 `(I0, Ires, a_raw, M_raw)`、closed 预算 `(a, M)`、`transport_coverage`/`transport_volume`、`physical_responsibility`/`physical_volume`/`physical_density`、Sample role count、source id/预算及 transport diagnostics。raw 预算始终验证 \(I_0=I_{res}+\sum_{j\in\mathcal P} a_{raw,j}\)。当 \(A_{raw}=\sum_{j\in\mathcal P} a_{raw,j}>0\) 时，closed Response 必须在该源内部按已吸收比例重分：

\[
a_j=I_0\frac{a_{raw,j}}{A_{raw}},\qquad
\mathbf M_j=I_0\frac{\mathbf M_{raw,j}}{A_{raw}},\qquad
\sum_{j\in\mathcal P}a_j=I_0.
\]

当 \(A_{raw}=0\) 时不得定义 closed Response、不得执行 scatter/Exp，也不得启动正常动力学。\(A_{raw}>0\) 时还必须通过固定 SampleField 的 normalized raw Response 细化比较，才可 closed；不得用 `MIN_RAW_ABSORPTION` 一类绝对阈值代替。自然相包含多个正质量 Physical Sample 源时，每个源都必须保留独立 raw/closed 子账本，然后才能按 \(w_{src}\) 聚合；Carrier 不作自然源。不允许先聚合 raw 残余再统一闭合。任一正质量自然源出现 \(A_{raw}=0\) 或细化失败都阻断整个事件步，不能丢弃该源或把其预算重分给其他源；外界源同样阻断。`EventStepResult` 还必须暴露两相各自的 SampleField、phase 顺序、每个几何单位的 `r_i`、反馈载荷与位移、自然/外界两类贡献、新 Event 在 \(q\) 的插入记录和所有四类规模量。

### 3.2 一次只运行一个阶段

禁止 `cargo test` 全量并行、禁止同时跑两个 K 场版本、禁止把 benchmark 与性质测试并发运行。阶段顺序固定：

1. Phase 0：解析、结构与性质测试；
2. Phase 1：单机制测试；
3. Phase 2：组合事件步；
4. Phase 3：大场梯度；
5. Phase 4：可选 1,024 压力确认。

只有当前 phase 的全部 hard pass、artifact 校验通过且摘要已写入日志，才可启动下一 phase。任一测试失败、超时、panic、资源超限、artifact 缺字段或结果不确定，立即停止本轮：不重试、不自动降规模、不跳过失败测试，也不继续后续 phase。修复后从失败 phase 的开头重新执行，并保存新的 run id；旧 artifact 不覆盖。

建议由一个串行 wrapper 实施，不由人工拼接命令。其逻辑为：

```text
for phase in requested_phases_in_order:
    create isolated run directory
    launch exactly one phase with fixed seed and resource caps
    require exit status 0 and validate phase artifact schema
    on any non-pass: record stop reason; exit non-zero
```

### 3.3 资源、超时与日志

每个 phase 独立进程、单测试线程，建议默认额度如下；实施时如环境限制不同，必须先更新本计划和 wrapper 的常量，再运行。

| phase | 最大 wall time | CPU time | 最大 RSS | 磁盘 artifact 上限 |
|---|---:|---:|---:|---:|
| 0 性质 | 5 min | 4 min | 1 GiB | 100 MiB |
| 1 单机制 | 10 min | 8 min | 2 GiB | 250 MiB |
| 2 组合事件步 | 15 min | 12 min | 3 GiB | 500 MiB |
| 3 64–512 大场 | 30 min/规模点 | 25 min | 4 GiB | 500 MiB/规模点 |
| 4 1,024 可选 | 60 min | 50 min | 6 GiB | 1 GiB |

日志根目录固定为 `.codex/logs/v2-validation/<UTC-run-id>/`，每个 phase 至少产生：`command.txt`、`environment.json`（Rust/Cargo 版本、git SHA、OS、限制、seed）、`stdout.log`、`stderr.log`、`result.json`、`summary.md` 和 `status.json`。`status.json` 必须包含 `pass | fail | timeout | resource_limit | invalid_artifact | skipped`；只有 `pass` 可解锁下一 phase。测试输出在控制台只显示首个错误及末尾摘要，完整诊断保存在该目录。

wrapper 必须设置并记录：

```bash
export FM_V2_SEED=20260810
export RUST_TEST_THREADS=1
export RAYON_NUM_THREADS=1
```

同时由受控子进程施加 wall-clock watchdog、CPU/RSS 监控和文件大小配额。不能仅依赖 `cargo test` 的退出码来证明没有超时或内存溢出。若宿主不支持某一 `ulimit`，wrapper 应明确报告能力缺失并停止，不得静默忽略该限制。

## 4. Phase 0：解析、账本与性质测试

本阶段只验证可由对象契约和局部 reference model 判断的性质；不得依赖长序列或随机大场。

| 编号 | 测试 | 输入与操作 | 通过条件 |
|---|---|---|---|
| P0-01 | 状态解析/归一化 | 所有 fixture 及持久化 round-trip | Coordinate 都为有限单位向量；Content 未被修改；非法维度、NaN、负 density 显式报错 |
| P0-02 | DensitySite 去重 | 同一 Coordinate 上挂 1、2、N 个不同 Content | `N_site`、site geometry、density、Sample field 完全相同；可读 Content 集合增加 |
| P0-03 | K 派生尺度单调 | 同一支撑集上按 K=64/128/256/512 构建不同分辨率场版本 | K 增大时 `derived_scale` 只变细或不变、`N_site` 不下降、重建/离散误差不增；每个 site 有可审计的跨版本映射；API 无独立 scale 输入 |
| P0-04 | 全空间/双责任 | 从非空 sparse field 生成 Sample；另直接构造带 Carrier 的 `vacuum_transport` SampleField | normalized measure 下 \(\phi_j\ge0\)、\(\sum_{j\in\mathcal T}\phi_j=1\)、所有 transport volume 为正且和为 1；Physical 支撑上 \(\chi_j\ge0\)、\(\sum_{j\in\mathcal P}\chi_j=1\)，零 physical-density 区所有 \(\chi=0\)。每个 Carrier 的 \(m=\rho=c=a=\mathbf M=0\)，但可有 transport coverage；S² artifact 另验 transport physical-area 之和为 \(4\pi\)。非空场中无 Event 的方向仍有 \(\phi\) coverage；`empty` 必须返回初始化未完成，不能用未定义的 \(p=\rho/M\) 生成正常 Sample |
| P0-05 | density 单位协变 | 在脱离正常 \(\rho=M p\) 规范的隔离测试中同时缩放 \(\rho,\mu_i,m_j,c_{ji}\mapsto c(\cdot)\)，固定一个 K 场版本 | 明确标记该变换不是另一个合法正常物理状态；Sample geometry 与 `derived_scale` 不变、责任比例不变；恢复规范后 \(\int\rho=N_{site}\) |
| P0-06 | K 模型族的正确边界 | `uniform_shell` 与 `local_cluster` 的 K=64/128/256/512 梯度 | 不要求不同 K 的 Response 同值；每个 K 版本各自 raw/closed 闭合。仅 `physics_reference_s2` 要求 K 增大时派生尺度、最大 Sample 直径和 density/测地积分误差不增并报告向连续 reference 的趋势；`semantic384` 只验有限 operational invariant，不伪造 \(S^{383}\) 细化结论 |
| P0-07 | 旋转等变 | 旋转 EventCoordinate、作用点和所有可旋转输入 | 对无连续对称性的 fixture，sites 与 \(\phi/\chi\) SampleField 在允许的稳定置换后只做同一旋转；density/coverage 重建、Carrier 零质量、标量 Response、edge-carried 方向矩及更新坐标满足同一等变关系。对 `uniform_shell` 等具有连续稳定子群的 fixture，不比较任意有限 Sample 中心，而比较重建场与 gauge-invariant observable；误差不超过 `ε_angle` |
| P0-08 | 当前态充分性 | 相同当前 Coordinate/Content、不同 CoordinateLog/history | 从 DensitySite 至 EventStepResult 的所有数值与 phase trace 相同 |
| P0-09 | Sample 充分性/中介性 | 生成同一 \(\phi,\chi,m,\rho,c\) SampleField 后改变不可见原始 density/sites；审计函数调用 | transport 结果不变，且 transport 的依赖图只读取 SampleField、source 与 versioned backend rule |
| P0-10 | physical coupling 可行性 | 对所有标准 fixture 构造带局部支撑的 \(c_{ji}\) | 非负 physical coupling 同时满足几何单位边缘 \(\mu_i\) 与 Physical Sample 边缘 \(m_j\)；Carrier 没有 \(c\) row。若给定 \(\chi\) 下不可行，必须显式失败并报告违反的集合/边缘，不能退化成非局部或不守恒 fallback |
| P0-11 | 初始化门禁 | `empty`、`single_site`、未达门槛的 skewed 场与满足预注册门槛的 `large_balanced` | 未完成初始化的状态不得进入正常两相事件步；达到基于代表性作用点 `N_eff`/集中度、实际 source \(A_{raw}>0\) 及 normalized raw Response 细化稳定的冻结门槛后才允许进入，且不得通过 `MIN_RAW_ABSORPTION`、运行期阻尼或降低预算绕过门禁 |

P0-06 的用意是避免把不同 K 场版本误报成同一物理状态，也避免用“模型族不同”掩盖内部不守恒。每个 K、每个 phase 下必须单独验证 raw 账本

\[
\left|I_0-I_{res}-\sum_{j\in\mathcal P} a_{raw,j}\right|\le\varepsilon_{abs},
\]

并且仅在 \(A_{raw}>0\) 且 normalized raw Response 细化通过时验证 closed 账本
\(\left|I_0-\sum_{j\in\mathcal P}a_j\right|\le\varepsilon_{abs}\)。

## 5. Phase 1：单机制测试

本阶段一次只开启一个机制。默认关闭自然坍缩、外界反坍缩或新 Event 写入中与当前测试无关的 phase，并在 artifact 写明关闭项。

| 编号 | 机制 | 设计 | 通过条件 |
|---|---|---|---|
| P1-01 | 真空运输 | 直接构造含 Carrier coverage、零 physical density 的 `vacuum_transport` SampleField，给定任意方向 source | 只验证 raw：`sum a_raw=0`、`I_res=I_0`；Carrier 无 \(\chi,m,\rho,Response\)；closed Response、scatter、Exp 与正常动力学均不启动，且无虚假方向矩/位移；不得把该诊断 fixture 当成从空 Event 场生成的正常 Sample |
| P1-02 | 均匀吸收 | `uniform_ray_oracle` 只作为 S2 continuous ray reference；`single_site`/`uniform_shell` 另行验证有限 Sample 吸收 | oracle 的每条 \(r\in[0,\pi]\) ray 必有 optical thickness \(1\) 与 raw residual \(I_0e^{-1}\)，四条 ray 的离散 normalized-weight aggregate residual 亦为 \(I_0e^{-1}\)；有限 Sample 的 raw 吸收按同一次扣减分配到 Physical Sample、非负且预算闭合；`A_raw>0` 且 fine/coarse normalized Response 收敛时，closed 按比例重分且 \(\sum_{j\in\mathcal P}a_j=I_0\) |
| P1-03 | \(\phi\)-\(\chi\) soft 吸收 | Carrier/Physical overlap 的单段传播 | 先按总 \(\kappa_S=\sum_{l\in\mathcal P}\chi_l\kappa_l\) 扣减一次，再按 owner \(\phi_j\) 到 Physical absorber \(\chi_l\kappa_l/\kappa_S\) 的同一责任分解入账；不得对重叠 Sample 重复吸收，也不得给 Carrier 产生 Response |
| P1-04 | 成对通量与 edge 方向矩 | 两/三 Sample 的内部界面、折线路径与同一场的细化副本 | 每一内部 scalar 通量成对反对称；所有 `U_j`、Physical `Q_l` 非负；\(\sum_{j\in\mathcal T}U_j+\sum_{l\in\mathcal P}Q_l\) 守恒。`W` 随 edge 平行运输且 \(\|W_j\|\le U_j\)；不得在下游以 source-to-center 径向 shortcut 重算 |
| P1-05 | 自力排除与对称方向矩 | 一般非径向 \(\phi/\chi\) 下的正质量 Physical source、`antipodal` 与旋转副本 | 自然源 Physical Sample 分配给自身的标量吸收仍进入预算，但方向矩按无自力公理严格置零；对点/完全对称 fixture 可有正吸收量但合计 \(\mathbf M=0\)；不得把 coverage 不对称变成虚假自力或任意方向 |
| P1-06 | Response 回分配 | 一个 Physical Sample 对 1、2、N 个几何单位的责任矩阵 \(c_{ji}\) | `c>=0`、几何单位列和为 \(\mu_i\)、Physical Sample 行和为 \(m_j\)，且 \(\sum_i\mu_i=\sum_{j\in\mathcal P}m_j\)；Carrier 没有 `c` row。平行运输回 Physical Sample 后的方向矩和等于原 \(\mathbf M_j\)；\(m_j=0\) 的行不发生除法或反馈 |
| P1-07 | 反馈重复不变 | 给一个几何单位增加重复 Content 后重跑 P1-06 | 该单位的载荷、位移和其他几何单位反馈均不变；不能按 Event 行数稀释或放大 |
| P1-08 | 纯自然坍缩 | 无 external event 的非均匀局部场，并另造一个正质量 Physical source 零吸收/细化失败反例 | 每个正质量 Physical Sample 源的 \(w_h^{src}=m_h/\sum_{k\in\mathcal P}m_k\) 非负且和为 1；Carrier 不作源。每个源分别完成 raw/closed 账本后再聚合；任一正质量源零吸收或细化失败时整相阻断且不重分预算；正常 fixture 的标量 `a` 始终非负，反馈方向矩严格为 \(-\mathbf M\)，且只来自冻结 Sample Response；不产生 Content 改写或直接 Event 路径 |
| P1-09 | 纯外界反坍缩 | 冻结自然 phase、向 `local_cluster` 输入一个 event | 外界 phase 的反馈方向矩严格为 \(+\mathbf M\)，且仅经其冻结 Sample 产生；相关区得到的反坍缩贡献高于远区，raw/closed 预算均闭合 |
| P1-10 | 自然/外界符号分离 | 在同一 fixture 分别运行 P1-08、P1-09，再按两相顺序执行 | 两 phase 的标量吸收均非负；自然贡献为 \(-\mathbf M\)、外界贡献为 \(+\mathbf M\)，不能把符号藏进负吸收；两相各自重建 Sample、各自审计预算和角运动界 |

P1-08 至 P1-10 的符号已冻结：自然相使用 \(-\mathbf M\)，外界相使用 \(+\mathbf M\)，标量吸收 \(a\) 从不带符号。两相必须分开记录、可独立重放，且不允许在最终反馈处再次翻转方向。每相的 SampleField 在该相 build 后冻结至该相 `run/scatter/Exp` 结束；外界相绝不可复用自然相的 SampleField。

## 6. Phase 2：组合事件步测试

Phase 2 只在 Phase 0 与 Phase 1 全部通过后执行。每次事件步必须保存可机读 phase trace：

```text
snapshot current EventCoordinates
-> build natural DensitySites / density / frozen SampleField
-> run natural transport -> closed Response -> scatter(-M) -> Exp existing EventCoordinates
-> rebuild DensitySites / density / frozen SampleField from intermediate EventCoordinates
-> run external transport -> closed Response -> readout from the external Response
-> scatter(+M) -> Exp existing EventCoordinates
-> append the new Event at q, last
```

自然相和外界相不是同一 SampleField 上的两个可交换 contribution，也不得在 scatter 前合并。每相的初始预算为 \(I_0=1\)：raw transport 按 \(1=I_{res}+\sum_{j\in\mathcal P} a_{raw,j}\) 记录；只有 raw absorption 为正且 normalized raw Response 的运输细化通过时，closed Response 才按比例重分至 \(\sum_{j\in\mathcal P}a_j=1\)，并分别执行 `scatter/Exp`。因此每相在该相冻结的可区分几何单位上都必须满足 \(\sum_i\Delta\theta_i^{phase}\le1\)。自然相后单位集合可能因完整重建而改变，完整事件只审计两个相位路径账本之和 \(L_{event}=L_C+L_A\le2\)，不伪造跨相位固定单位对应。在正常事件步中，任一正质量 Physical natural source 或外界 source 零吸收/细化失败都必须令整步以 `initialization_incomplete` 阻断；只在 P1-01 真空诊断中允许留下 raw transport record 而不启动 closed Response、scatter 或 Exp。

| 编号 | 测试 | 输入与操作 | 通过条件 |
|---|---|---|---|
| P2-01 | 两相顺序与重建 | 不低于 S64、且实际达到初始化门槛的最小非对称 fixture，带 external event | 自然相 `build/run/scatter/Exp` 完整结束后，必须从中间 EventCoordinates 重建外界 DensitySite/density/Sample；trace 证明外界未复用自然 Sample 或初始 density |
| P2-02 | readout 与新 Event 最后原位落位 | 对 P2-01 的 event 设置唯一 Content 和 source coordinate \(q\) | readout 读取外界 closed Response，严格早于外界 scatter/Exp；两个相位更新结束后才 append；新 Event 的 Coordinate 精确为 \(q\)，Content 原样保留；它未收到自己的当前步方向力，下一事件步才参与场构建 |
| P2-03 | 相位方向、零矩与自作用 | 含不对称、正质量 Physical source、对点与完全对称两个 fixture | 自然 scatter 使用 \(-\mathbf M\)，外界 scatter 使用 \(+\mathbf M\)；source 的自作用方向矩为零；对点/完全对称汇聚的总方向矩为零；\(\mathbf P_i=0\) 时位移为零，不允许任意补向。Carrier 从不作为 Response/scatter 行出现 |
| P2-04 | 两相预算与角运动界 | 近/远皆有 density 的 fixture | 自然/外界分别通过 raw 与 closed 账本；每相在本相几何单位上 \(\sum_i\Delta\theta_i\le1\)，两相路径账本之和 \(L_C+L_A\le2\)；不得假设重建前后单位一一对应，也不得在两相之间双计或混合 Sample Response |
| P2-05 | 局部性 | `local_cluster` 中在 q 处重复同一 event，记录角距离 ring | 在该预注册 fixture 上，近区外界反坍缩占优、远区自然坍缩占优，且外界贡献的 ring 统计满足预注册衰减门槛；失败表示当前运输/闭合没有实现所需行为，不得把该 fixture 结论冒充任意 density 上的普适单调定理 |
| P2-06 | 事件顺序 | 两个非共线 unit-budget external event 的 A→B 与 B→A；另测同方向对照 | 非共线事件允许并应报告顺序差异（读写同一步的结果）；同方向、同单位预算的对照在容差内一致；无 hidden history 时相同最终当前态的下一步响应一致 |
| P2-07 | 动态平衡 | 固定分布的事件流、固定步数、冷启动重复三次 | 总结构量、局部 density/Response、`N_eff` 与位移统计进入预注册的有界带，而非单调全场坍缩或无界增长；三个重复 run bitwise 或容差内相同 |
| P2-08 | Content/日志旁路 | 同 Coordinate 不同 Content、多份不同 CoordinateLog | 除可读 Content 集合和审计日志外，事件步数值结果完全一致 |

局部性使用**贡献分解**而非只看最终位移：一个远区最终变化小，可能是两种大贡献抵消，不能据此宣布外界影响局部。artifact 必须分别按自然相与外界相当时的几何单位输出 `scalar`、`moment`、feedback 与位移，另记录两相路径账本；若实现为了诊断追踪持久 Event 行，还必须把该映射与几何单位账本分开，不能把重复 Content 计入守恒和。不得生成“合并后才 scatter”的伪字段。

动态平衡门槛在开跑前按冻结的 density 单位、事件流和步数设置，例如：burn-in 后连续窗口内总量斜率、远区 floor 命中比例、`N_eff` 分位数和最大单点位移均有上界。门槛不得以“最终看起来稳定”替代；若当前理论尚不能给出这些数值上界，Phase 2 只能输出轨迹，不得宣称已验证动态平衡。

## 7. Phase 3：大场梯度与 \(O(1/N_{eff})\) 位移

### 7.1 规模矩阵

所有规模点使用相同 reference dimension、fixture family、source 强度和固定
seed；`physics_reference_s2` 是唯一物理 reference backend，\(K\) 是唯一外部指定的分辨率版本，`derived_scale` 与 `N_site` 必须由
当前几何导出。`large_balanced` 与 `large_skewed` 都要运行，避免把更大的 K
或更多 site 错当成更广参与。

| tier | K | 不同输入支撑点 | Nsite | 预期 Neff 情形 | 目的 |
|---|---:|---:|---|---|---|
| S64 | 64 | 256 | 运行时派生 | balanced / skewed | 最小大场基线 |
| S128 | 128 | 512 | 运行时派生 | balanced / skewed | 分辨率版本梯度 |
| S256 | 256 | 1,024 | 运行时派生 | balanced / skewed | 主趋势点 |
| S512 | 512 | 2,048 | 运行时派生 | balanced / skewed | 必跑的大场上界 |
| S1024 | 1,024 | 4,096 | 运行时派生 | balanced / skewed | 仅在 Phase 3 全通过且资源余量充足时执行 |

对每个规模点，raw Event 行数还要分别取“每个不同支撑点一个 Content”与
“每个支撑点两个 Content”。两者必须具有相同 `derived_scale`、Nsite、Sample
field、N_eff 和动力学，借此持续检查“内容重数不是几何质量”。

### 7.2 必测指标

每个规模点输出：`N_site`、K、`derived_scale` 及推导证据、`transport_sample_count`、`physical_sample_count`、`carrier_sample_count`、实际 transport Sample 最大直径、density-only 表示误差、仅在 `physics_reference_s2` 适用的测地积分/连续-reference 误差、每相 raw 预算误差/残余预算及 coarse/fine normalized Response 差、每相 closed 预算误差、`w_src`、`r_i`、`N_eff`、Response 份额的 Herfindahl 指数、近/远区贡献、每相和事件总计的平均/中位/最大角位移、每事件 wall time、峰值 RSS、artifact 大小及所有失败/跳过原因。

所有非真空 fixture 先验证一般大场上界。令每相 \(r_i=a_i/I_0\)、\(\Delta\theta_i=d(z_i,z_i')\)，则必须满足：

\[
\sum_i r_i\Delta\theta_i
\le
\frac{I_0}{N_{eff}}.
\]

这是闭合标量份额与无增益回分配给出的响应加权上界；它不等价于声明任意非均衡场的普通单点位移都按 \(1/N_{eff}\) 缩小。

`O(1/N_eff)` 只作为**上界**验证，不要求方向矩非退化，也不把它升级成 \(\Theta(1/N_eff)\)。在 `large_balanced` 的等权回分配条件下，按自然相、外界相分别预先检查所有 active 单位的 \(r_i\) 在容差内相等，且 \(N_{active}=|\{i:r_i>0\}|\approx N_{eff}\)。令 \(\Delta\theta_i\) 是一次相位中每个 active 几何单位的角位移，报告：

\[
\bar{\Delta\theta}
=\frac{1}{N_{active}}\sum_{i\;:\;r_i>0}\Delta\theta_i,
\qquad
C_{active}=N_{active}\bar{\Delta\theta}.
\]

hard pass 条件是每个规模点都满足 \(\bar{\Delta\theta}\le I_0/N_{active}\)，等价地 \(C_{active}\le I_0\)（加预注册数值容差）。对称抵消可以让位移更快下降甚至为零，这仍符合理论。64、128、256、512 的 log-log 斜率和 \(C_{active}\) 分布必须报告，但在没有额外方向矩下界前仅为诊断，不能因斜率偏离 \(-1\) 而失败。这里不以 Nsite、K 或 `w_src` 充当横轴。`large_skewed` 只验证一般响应加权上界；即使 Nsite/K 很大而 N_eff 较低，也不应被误判为违反大场律。

若位移律的冻结实现是

\[
z_i'=\operatorname{Exp}_{z_i}(\mathbf P_i),
\]

则额外检查每相 \(\Delta\theta_i^{phase}=\|\mathbf P_i^{phase}\|\)（在 injectivity radius 内）、\(\sum_i\Delta\theta_i^{phase}\le 1\)，以及两相各自单位集合上的路径账本之和 \(L_C+L_A\le2\)。该检查不假设重建前后几何单位一一对应，也不引入新的位移系数；场足够大时的温和演化必须从守恒分配和 `N_eff` 涌现。

## 8. Phase 4：受门禁控制的 1,024 压力确认

S1024 不是用来掩盖 512 以下失败的重试。只有 S64、S128、S256、S512 的预算、`physics_reference_s2` K/continuous-reference 趋势、局部性、动态平衡和位移律均通过，并且 wrapper 确认 S512 峰值 RSS 小于 4 GiB、wall time 小于 30 min 的 70%，才允许启动。否则 Phase 4 的 `status.json` 记 `skipped` 并附阻断 phase/指标；这不是 pass。`semantic384` 的独立规模确认只验证版本化 finite-graph operational gates，不借用该条件声称高维连续收敛。

Phase 4 依次运行 `large_balanced`、`large_skewed` 与重复 Content 对照，固定
\(K=1024\) 与 4,096 个不同输入支撑点，`derived_scale` 和 \(N_{site}\) 仍由
运行时按同一契约导出。沿用 Phase 3 已冻结的 seed、容差和所有物理参数。
它必须重新验证每相 raw/closed 预算、逐源自然账本、coupling 边缘、两相重建、最后落位、旋转/重复不变、每相角运动界、局部贡献分解及资源上限。不得因规模增大修改分辨率算子、kernel、coverage、闭合方式或通过门槛。

Phase 4 使用独立进程和独立 artifact 目录；任一 fixture 失败即停止剩余 S1024 项。通过 Phase 4 只说明 1,024 规模确认通过，不得覆盖或“修复”较小规模趋势。

## 9. Artifact、复核与结论格式

fixture manifest、`run_manifest.json`、`status.json` 与 `result.json` 的顶层 JSON
版本键统一为 `"schema_version": 1`。文中可称为 artifact schema version，但不得
再引入第二个顶层 artifact-version 键。

每一个 `result.json` 必须逐字段通过 committed
`experiments/v2_validation/artifact-schema-v1.json`；该 JSON Schema 是唯一可执行
字段表，本计划不再复制一个容易漂移的伪示例。尤其必须包含 schema 所列的
`document_kind/run_id/batch_id/tier/gates/sidecars`，使用 `derived_resolution` 而非
`derived_scale` object，并物化全部 tolerance/resource 子键。禁止以空字符串冒充
SHA、以零维度/零预算冒充真实 pass，或增加 schema 未知字段。

数值 0 只示意 schema，不能作为真实无数据成功。每个自然源的验证流必须逐
Physical Sample 包含 `sample_id`、`a_raw`、`M_raw`、`a_closed` 与 `M_closed`；
Carrier 不得出现 Response row。其 canonical SHA-256、行数和源级汇总必须一致。
P0–P2 的 `sample_ledger.mode=full`，保留全部逐源压缩 sidecar。P3–P4 为避免
\(O(K^2D)\) 方向矩把验证进程本身撑爆，使用 `mode=streamed_full`：每行在产生时
写入压缩 JSONL，不把全矩阵驻留内存；离线 validator 仍须完整解压重放全部行。
不接受只有 digest/汇总而无法离线重算的 `streamed_digest`。每个 phase 结束再运行
离线 artifact validator：检查 schema、有限数值、四类规模量与 \(\phi/\chi\) role
的定义、K-to-scale 推导证据、Carrier 零物理质量、自然相逐源权重与子账本可重算、
每个源的 `closure_factor=I0/A_raw`、raw Response 细化差、逐 Sample 标量/
edge-carried 方向矩缩放、聚合预算、seed、资源记录和 pass/fail 一致性。任一在线
或离线 validator 失败都属于该 phase 失败。

最终报告按 phase 给出：通过/失败/跳过、精确命令、git SHA、seed、日志目录、实际规模、最大误差、资源消耗和阻断后的最早失败项。结论必须区分：

- “某个有限 K 的离散账本守恒”；
- “`physics_reference_s2` 中不同 K 分辨率模型族对连续极限收敛”；
- “`semantic384` 的固定有限 graph 满足 operational invariant（不等于高维连续收敛）”；
- “大场在实际 N_eff 下呈现 \(O(1/N_eff)\) 单位位移”；
- “尚未验证/因失败而停止”。

不得以编译通过、单个 30-site demo、单个 seed、单个 K 或只看最终静态图代替上述任何一个结论。

## 10. 实施顺序与非目标

建议把测试与实现同模块放置：局部几何/运输单测在对应 Rust 模块的 `#[cfg(test)]`，跨模块事件步和规模 harness 放在独立 integration/experiment target。测试代码的实现顺序必须和本计划 phase 一致：先写 P0 与 P1 的 reference fixtures，再写 P2 的 phase trace，最后接入 P3/P4 受控 runner。任何使用 `DummyEmbedProvider` 的测试都不应证明几何或语义正确性；本计划的核心 fixtures 直接提供归一化 direction，避免 embedding 噪声混入物理判定。

本计划不做以下事情：不改变 v2 理论基石、不在 accepted implementation contract
之外另选 density estimator、\(\phi/\chi\) 投影或 graph 通量、不引入新的核心物理
参数、不迁移旧持久化格式、不重跑 LLM/retrieval benchmark，也不把初始化不足的
大场条件用运行期阻尼、小场特例、density floor 或 `MIN_RAW_ABSORPTION` 遮蔽。
残余的全局比例闭合、新 Event 在 \(q\) 最后落位及两相 Exp 已由理论基石冻结，
测试只能验证它们，不能另换终点律。初始化负责建立足够大的认知地形；正常动力学
的验收从 S64 起，并以 S512 为必过规模，而不是以 30 个对象的视觉 demo 为准。
