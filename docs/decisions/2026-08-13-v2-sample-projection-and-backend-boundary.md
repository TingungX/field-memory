# ADR：v2 Sample 投影、覆盖责任与 backend 边界

Status: accepted
Owner: field-memory
Last updated: 2026-08-13
Scope: v2 的 Density/Sample 投影、空区传播、Response 闭合、方向矩运输，以及 physics reference 与语义生产 backend 的边界
Related code: 尚无 v2 实现
Related docs: [v2 理论基石](../design/field-memory-v2-foundations.md)、[v2 权威、分辨率与维度 ADR](2026-08-12-v2-authority-resolution-and-dimension.md)、[384D 生产语义维度 ADR](2026-08-12-v2-semantic-dimension-384.md)、[v2 实现契约（待接受）](../specs/field-memory-v2-implementation-contract.md)、[v2 验证计划](../specs/2026-08-10-field-memory-v2-validation-plan.md)

## 背景

v2 的因果链已经确定为

```text
EventCoordinate -> DensitySite -> Density -> Sample -> Response -> EventCoordinate
```

但早期实现草案把 DensitySite 近似直接复制成 Sample，并用一个紧支撑核、
一个 coverage 半径和一个 `MIN_RAW_ABSORPTION` 阈值同时承担物理密度、覆盖和
数值可达性。这会混淆三个不同问题：

1. 当前 EventCoordinate 的相对 density 结构应该如何被有限预算表达；
2. 整个方向空间如何由有限 Sample 承担覆盖责任；
3. 一次传播的有限离散结果何时足以定义闭合 Response。

本 ADR 把这三个问题拆开。它冻结架构边界和不可替换的语义；具体常数、求积
阶数、tie-break 与数据结构仍必须在
[v2 实现契约](../specs/field-memory-v2-implementation-contract.md) 中机械化，
该契约在接受前不得开始 v2 动力学实现。

## 已接受决策

### 1. Sample 是 density 的残差投影，不是 DensitySite 的复制

DensitySite 是从当前 EventCoordinate 几何中提取尺度化结构的中间产物；它不承接
传播，也不直接产生 Response。Sample 必须在 density 建立之后生成：

\[
  S_{t,K}=\mathcal P_K(\rho_{t,K}),
  \qquad \rho_{t,K}=M_{t,K}p_{t,K}.
\]

\(\mathcal P_K\) 只读取 density 的相对分布特征和固定的 Sample 预算 \(K\)。
实现应从当前有限表示尚未表达的 density 残差中选择或细化 Sample 候选，并以
版本化的 density-only 表示误差作为停止条件。整体缩放 \(\rho\) 不得改变
Sample 的相对几何；绝对量只在 Sample 生成时固化为 Sample density 与 mass。

因此，下列做法均不再是 v2 的合法实现：

- 为每个 DensitySite 建立一个同名 Sample；
- 依据某次 Response、传播误差或 EventContent 决定 Sample 数量或位置；
- 在 Sample 交互时回看原始 density、DensitySite 或 Event 列表以补回未表达的差异。

Sample 的有限预算表达的是 density 的变化特征，而不是 Event 行数。相同
density 在同一版本和同一预算下必须得到相同的 Sample 场（包括确定性 tie-break）。

### 2. 物理 density/coupling 核保持紧支撑；coverage 是独立的软分区责任

物理 density 与 Sample-to-geometry coupling 使用同一个归一化、旋转等变、紧
支撑的 kernel family。该 kernel 负责“结构质量在哪里”以及“Sample 对哪些
几何单位承担多少结构责任”，不能为了让空区有数值而偷偷加入全局 density floor。

Sample coverage 则是整个 Sample 集合共同导出的非负 partition of unity：

\[
  \phi_j(u)\ge 0,\qquad \sum_j\phi_j(u)=1.
\]

coverage 可以采用与物理核相同的 Wendland profile，以复用局部性和实现，但
它在概念上不是 density/coupling 核的同义词。coverage 的半径、有效 volume、
图边和分辨率责任由 Sample 集合的几何及版本化投影规则导出，不能由 Response
误差反推。

覆盖用的 broad carrier 不是额外的背景质量、真空质量或人为补偿项。Carrier
只承担 partition 中的覆盖责任；它的 Sample mass 必须来自已经拟合的 physical
density 与 coupling 投影，不能凭空给空区增加 \(\rho>0\)。因此无 Event 的方向
可以保持零 density，仍由 coverage/transport graph 经过；“可传播”不等于
“该处存在物理质量”。

这一拆分保留两条同时成立的要求：场定义域始终是完整方向空间；物理 density
仍可在局部 Event 支撑之外为零。coverage 不得被解释成场本身，也不得成为
第二套会影响 Response 的 density。

### 3. 删除任意 `MIN_RAW_ABSORPTION`；以可达性和离散收敛定义闭合

一次单源传播必须先产生 raw ledger：

\[
  I_0=I_{res}+\sum_j a_{raw,j}.
\]

不再使用与维度、尺度或 source budget 无关的绝对门槛（例如
`MIN_RAW_ABSORPTION = 1e-6`）把“小但真实的吸收”判作不存在。只要离散算法
给出严格正的总 raw absorption

\[
  A_{raw}=\sum_j a_{raw,j}>0,
\]

就按同一 source 内的比例闭合：

\[
  a_j=I_0\frac{a_{raw,j}}{A_{raw}},\qquad
  \mathbf M_j=I_0\frac{\mathbf M_{raw,j}}{A_{raw}}.
\]

\(A_{raw}=0\) 才表示该 source 当前不可闭合，必须报告为未解决状态，不得把
预算静默转给其他 source、写入残余对象或跳过该 source。

由于比例闭合会放大极小的数值差异，\(A_{raw}>0\) 不是充分的数值质量证明。
可闭合的 source 还必须通过版本化的离散细化检查：在固定 SampleField 下，
至少比较受控步长/运输细化后的归一化 Response 分布与方向矩；若物理 coupling
求积也参与本次计算，则同时使用其已冻结的求积收敛证据。比较对象是闭合前后
不受总量尺度影响的 normalized distribution，而不是任意绝对吸收阈值。

这条规则使“空区可传播”和“没有物理吸收就不能闭合”同时成立：coverage/graph
负责可达性，physical density 决定是否发生吸收，离散收敛决定结果是否可信。

### 4. Response 的方向矩沿 graph edge 携带

传播的标量载荷和方向矩是两种不同的量。标量 flux 通过 graph edge 以反对称
数值通量交换，满足源、边和 Response 的标量守恒。方向矩不得在每个 Sample
重新由“作用点到 Sample 中心”的端点径向方向重算；那会把经过的路径历史
丢掉，并在弯折或细分 graph 时产生不可接受的方向差异。

离散状态因此必须为沿途方向矩提供明确的 edge transport：

- edge 离开上游 Sample 时，携带该次离散 flux 对应的切向载荷；
- 到达下游 Sample 时，沿该 edge 的球面最短测地线平行运输，或按契约中等价
  的 edge tangent 规则更新；
- Sample 吸收的方向矩与同一标量吸收使用同一份 flux/责任分解，并在 coupling
  回分配时继续使用相同的平行运输规则；
- cut locus 不是可随意选方向的 fallback。具有非零数值权重的 edge/coupling
  必须满足契约规定的 cut-locus margin，零测集只可作为显式诊断边界。

端点径向向量可以用于作用点附近的初始化观测，但不能充当跨 graph 的累计
方向矩定义。自然相的“无自力”只约束 source Sample 对自身的方向矩分量为零，
不改变标量吸收和其他 edge 的方向运输。

### 5. 两个 backend 的职责不可互换

`physics_reference_s2` 是连续几何与运输的 oracle/reference backend。它在
\(S^2\) 上验证测地线、coverage、局部纯吸收、守恒、方向矩运输及细化趋势，
用于发现离散算法错误和建立可审计的参考解。它不是生产 embedding 的低维替代。

`semantic384` 是当前生产语义版本（\(D=384\)）上的有限、由 density 派生的
Sample graph operational model。给定固定的 projection identity、\(K\) 与
版本化求积，它必须满足 v2 的因果链、非负性、标量预算守恒、旋转/重标记可观测
不变量和事件步顺序；但 K=64 等有限预算不被宣称为 \(S^{383}\) 上的局部连续
网格，也不承诺已有的 S² continuum convergence 命题在 384 维自动成立。

因此：

- S² 通过不能直接证明 semantic384 的 density fidelity 或长期动力学正确；
- semantic384 的 graph 实现不能反过来修改 S² 连续 reference 的定义；
- 两者共享 algebraic ledger、Sample 投影语义、coupling/Response 闭合和事件
  步边界，但各自记录 backend/space identity，禁止 artifact 互冒。

### 6. 对称退化比较重建 observable，并允许版本化 gauge

当 density 具有连续对称性或 rank-deficient support 时，单个有限 DensitySite
代表或 Sample 中心可能不存在唯一、连续且严格旋转等变的选择。实现不得把任意浮点遍历顺序
伪装成物理方向，也不得因一个合法 gauge 选择而把整个表示判为失败。

在这类退化 fixture 上，验收对象是由 Sample 场重建的 observable：density/
coverage 重建、总质量、coupling、Response 标量分布、方向矩（在对称性要求的
分量上）以及事件步后的坐标变化。Sample 中心本身只在版本化 gauge 约束下
比较；gauge 必须有稳定 identity、明确 tie-break，并写入 FieldVersion/fixture
artifact。对称副本的比较应先对齐同一版本 gauge 或比较 gauge-invariant
observable，不能用某个任意中心的逐点坐标差替代理论判定。

## 不选择的方案

### 直接 Site -> Sample

它把中间拟合单位误认为交互单位，无法表达“Sample 分辨 density 变化特征”，
还会让增加 DensitySite 数量直接改变交互源数和质量分配。故不接受。

### 用全支撑核或 density floor 填满空区

这会把 coverage 的可达性偷换成物理质量，改变场的本体并在 Event 稀疏时制造
虚假 Response。v2 采用紧支撑 physical density，空区由 coverage/graph 运输经过。

### 用固定 raw 吸收下限决定能否闭合

固定绝对阈值依赖单位、维度和数值尺度，无法区分真实低密度吸收与数值噪声；
它也会使同一场仅因预算或尺度变化而改变物理语义。v2 改用 `A_raw > 0` 加离散
细化收敛。

### 每到一个 Sample 都按作用点径向方向重算方向矩

这不是路径运输，而是 endpoint shortcut；在弯折 graph、细分和旋转副本上会
产生不同的 Response 方向。方向矩必须沿 edge 携带。

### 把 semantic384 当成高维连续网格

在固定有限 K 下，这一说法没有可验证的 \(S^{383}\) 局部网格含义。语义生产
backend 的承诺限定为有限 density-derived graph 的操作语义与结构不变量；若
未来需要高维 continuum theorem，必须另立 ADR 和实验，不得从本决定推断。

## 后果与实现门禁

正面后果：Sample 的生成、coverage 的责任、physical mass 的位置以及 Response
闭合各自只有一个职责；空区传播不再需要伪造 density；S² 的严格 oracle 与
384D 的可用生产图可以并行发展而不互相冒充。

代价是实现必须显式维护 density-only residual projection、coverage partition、
graph edge moment、raw/closed 双账本和 backend identity，不能用一个简单的
“每个点一个 Sample”循环替代。细化收敛也会增加初始化和诊断成本，但这是
比例闭合在低 raw absorption 下保持可审计所必需的数值条件。

implementation contract 在接受前必须进一步冻结并测试：

1. \(\mathcal G_K\)、density-only residual error、Sample 候选及所有 tie-break；
2. physical kernel 的归一化、compact support、coupling 与 coverage profile；
3. graph 构造、edge flux、parallel transport、cut-locus margin 与细化协议；
4. `A_raw=0`、`A_raw>0` 和归一化 Response 收敛时的精确错误/结果 schema；
5. 对称退化的 gauge identity、observable 比较和 fixture artifact；
6. `physics_reference_s2` 与 `semantic384` 的 FieldVersion、artifact 和测试
   命令互斥校验。

只有上述内容写入并接受实现契约后，才解除
`blocked:implementation-contract-missing`，开始 v2 动力学框架实现。
