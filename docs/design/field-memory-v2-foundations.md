# field-memory v2 理论基石：Event、Sample 与尺度密度

Status: draft
Owner: field-memory
Last updated: 2026-08-13
Scope: v2 的场本体、Event 表示、density、Sample、守恒运输、两相事件步与大场动力学
Related code: 计划中的独立 `crates/core/field-mem-v2/`；现有 v1 实现不构成本理论的约束
Related docs: [v2 实现契约](../specs/field-memory-v2-implementation-contract.md)、[v2 权威、分辨率与维度 ADR](../decisions/2026-08-12-v2-authority-resolution-and-dimension.md)、[生产语义维度 384 ADR](../decisions/2026-08-12-v2-semantic-dimension-384.md)、[Sample 投影、覆盖责任与 backend 边界 ADR](../decisions/2026-08-13-v2-sample-projection-and-backend-boundary.md)、[v2 验证计划](../specs/2026-08-10-field-memory-v2-validation-plan.md)、[生产维度实验](../specs/2026-08-12-v2-dimension-fidelity-experiment.md)、[v2 设计草案](../superpowers/specs/2026-07-04-field-memory-v2-design.md)、[v2 可行性规模验证](../reports/2026-08-09-v2-feasibility.md)

## 0. 文档地位

本文不是实现 spec，而是 v2 当前已经确认的定义与尚未闭合的问题清单。它先回答“系统里的东西究竟是什么”，再允许我们讨论传播、损耗、平衡和实现。本文仍为 draft；未明确列为已确认的公式不得视为定论。

v2 是 field-memory 的当前理论权威；现有 Anchor/impact/RelaxationCycle 实现是
冻结的独立 v1 路径，不参与补全本文，也不构成 v2 的兼容目标。本文中的
开放项不能因“v2 权威”而被实现者自行猜测。accepted implementation contract
已经冻结第一版唯一数值算法、边界与验证命令；独立 v2 实现已解锁，但任何实现
分叉仍须先以新 ADR 和新 algorithm id 修改契约。

本文确认的结论优先于 2026-07-04 v2 草案中与之冲突的表述，尤其是：

- anchor 不是理论必需结构；
- density 不是写在 anchor 或 event 上的基础标量；
- Event 的数量不是场的质量；
- EventCoordinate 之间以及外部扰动与场之间，都不存在绕过 Sample 的直接作用路径。

旧草案仍作为思想演进记录保留，其中“场是唯一的本体”“关系只来自角距离”“所有交互经由 Sample”继续成立。旧稿中可直接写入的 anchor density、单相事件回路与未触及 Sample 的默认衰减不再成立；本文第 6、7 节重新定义当前采用的运输闭合与两相动力学。

---

## 1. 本体与当前态

### 1.1 场是唯一的本体

场存在于归一化 embedding 构成的方向空间

\[
\mathcal D=S^{D-1}
\]

中。Event 和 Sample 都不是场本身，而是工程上逼近、读取和更新场的表示。

场的定义域始终是完整的 \(\mathcal D\)。某个方向没有 Event，只表示那里当前没有内容落点，不表示场在那里不存在，也不禁止扰动经过或 EventCoordinate 向那里演化。

场满足**当前态充分性**：只要两个系统的当前场相同，它们面对同一后续扰动就必须给出相同的场响应。过去只能通过已经留在当前场中的结果产生作用，不能绕过当前场再次参与计算。

当前场的版本化配置包括 embedding 映射版本、环境维度 \(D\) 与 Sample
预算 \(K\)。同一 EventCoordinate 集在不同 \(K\) 下属于不同物理分辨率的
场版本；当前态充分性比较的“同一当前场”必须包含这些配置。

因此：

- 历史不是第二个隐藏的记忆系统；
- replay、日志或时间戳不能直接改变正常交互结果；
- 若相同的当前场因不同历史而产生不同场响应，要么当前场的表示不完整，要么实现引入了非法旁路。

### 1.2 Reference 维度与生产维度

核心几何必须支持任意有限 \(D\)。`physics_reference_s2` 在 \(S^2\) 上承担
连续几何和运输的 reference：它检验测地线、物理 density、纯吸收、守恒、方向矩
与离散细化是否对同一个连续参考收敛。它不是默认生产语义空间；其连续收敛结论也
不能直接迁移到高维 backend。

当前生产语义表示已经由独立 formal 实验与 accepted ADR 冻结为
\(D_{\rm semantic}=384\)，方向空间为 \(S^{383}\)。这个结论绑定已验证的
BGE-M3 model digest、Build-only uncentered spherical PCA 和 projection bundle
content hash；它不表示任意 384D 映射都合法。`semantic384` 是同一因果链下、由
density 派生的有限 Sample graph operational model：它必须满足版本身份、非负性、
标量账本、可观测量不变量和事件步顺序，但固定 \(K\) 不被宣称为 \(S^{383}\) 上的
局部连续网格，也不自动继承 \(S^2\) 的 continuum 命题。两个 backend 的 artifact
不得互冒。完整表示身份与 backend 边界见
[生产语义维度 384 ADR](../decisions/2026-08-12-v2-semantic-dimension-384.md) 和
[Sample 投影、覆盖责任与 backend 边界 ADR](../decisions/2026-08-13-v2-sample-projection-and-backend-boundary.md)。

### 1.3 理论对象与工程表示

理论上，我们关心的是当前场及其变化。工程上，我们持久化 Event 及其当前坐标，用它们拟合当前场：

\[
E_t \xrightarrow{\ \Phi\ } \widehat F_t
\]

其中 \(F_t\) 是理论上的场，\(E_t\) 是当前 Event 状态，\(\widehat F_t\) 是由有限表示得到的场拟合。\(E_t\) 不是与场并列的本体，也不是历史事件计数器。

---

## 2. Event：不变的 Content 与可变的 Coordinate

一个 Event 只有两个理论字段：

\[
e_i(t)=\big(c_i,d_i(t)\big)
\]

- \(c_i=\text{EventContent}_i\)：不可变的内容载荷；
- \(d_i(t)=\text{EventCoordinate}_i(t)\in\mathcal D\)：Event 在当前场中的坐标；在当前方向空间里，它就是归一化的 `direction`。

### 2.1 EventContent

EventContent 被记录后不再被场改写。它用于最终读出“这里承载了什么”，但不直接参与场的几何计算。

### 2.2 EventCoordinate

EventCoordinate 是 Event 与当前场发生关系的唯一几何入口。两个 Event 的关系不是一条持久化的语义边，而是由其当前角距离即时导出：

\[
\theta(d_i,d_j)=\arccos\big(\operatorname{clip}(d_i\cdot d_j,-1,1)\big)
\]

坐标改变，关系随之改变；不需要也不允许另一套独立关系成为场的事实来源。

### 2.3 CoordinateLog

EventCoordinate 的变化可以写入 append-only 的 `CoordinateLog`：

\[
L_i=\big[(t_0,d_i(t_0)),(t_1,d_i(t_1)),\ldots\big]
\]

它只服务于审计、解释、调试或离线重放。正常交互读取当前 \(d_i(t)\)，不能从日志中额外恢复影响，否则历史会绕过当前场。

### 2.4 内容重数不等于几何重数

当前 Event 状态是

\[
E_t=\{(c_i,d_i(t))\}_{i=1}^{n},
\]

而用于拟合场几何的是方向的支撑集

\[
X_t=\operatorname{supp}\{d_i(t)\}_{i=1}^{n}.
\]

\(X_t\) 是生成 Sample 时使用的有限几何依据，不是场的存在范围；场的定义域仍是完整的 \(\mathcal D\)。

若两个不同 EventContent 位于完全相同的 direction，它们是两个可读出的内容，但只是一个几何位置：

\[
\operatorname{supp}(X\cup\{d,d\})
=
\operatorname{supp}(X\cup\{d\}).
\]

因此数据库里有两行，不会令该位置的场影响自动从一变成二。重复事件若会强化场，强化必须来自它再次触发完整的 Sample 交互并改变了当前几何，不能来自原始计数相加。

---

## 3. 从 Event 拟合 Density，再生成 Sample

Event 与 Sample 仍构成持久层和交互层；DensitySite 与 density 是二者之间即时重建的场投影：

| 对象 | 生命周期 | 职责 |
|---|---|---|
| Event | 持久 | 保存不可变 Content 与当前 Coordinate，作为当前场的有限拟合 |
| DensitySite | 即时中间产物 | 从 EventCoordinate 几何中提取可区分的方向结构，用于重建 density |
| Density | 即时导出场量 | 定义在完整方向空间上，用于生成完整的 Sample 场 |
| Sample | 单次交互 | 以 \(\phi\) 承担全域运输、以 \(\chi\) 固化 Physical density/volume/mass/Response；Carrier 只有前者，成为本次交互唯一可访问的场拟合 |

完整链条是：

\[
EventCoordinate
\rightarrow DensitySite
\rightarrow Density
\rightarrow Sample
\rightarrow Response
\rightarrow EventCoordinate.
\]

DensitySite 不是 Sample，不承接扰动，也不产生 Response。它只负责从有限 EventCoordinate 拟合 density。真正的 Sample 在 density 建立之后生成；生成结束后，本次交互不能越过 Sample 再读取原始 density。

### 3.1 唯一合法的单相因果回路

设 \(K\) 为当前场版本固定的 Sample 预算。DensitySite 的角区分尺度不是
独立输入，而由该相开始时的 EventCoordinate 几何确定性导出：

\[
\ell_t^K=\mathcal G_K(X_t).
\]

任意一个自然或外界相位都必须先从该相位开始时的 EventCoordinate 重建
完整交互面：

\[
C_{t,K}=\mathcal C_{\ell_t^K}(X_t)
\]

\[
\rho_{t,K}=\mathcal D_{\ell_t^K}(C_{t,K})
\]

\[
S_{t,K}=\mathcal P_K(\rho_{t,K})
\]

\[
R_{t,K}^{\varepsilon}
=\mathcal I^{\varepsilon}
(S_{t,K},q,I_0)
\]

\[
E_t'=\mathcal B(E_t,R_{t,K}^{\varepsilon}).
\]

其中：

- \(\mathcal G_K\)：在版本化内禀保真标准下，导出最多 \(K\) 个 Sample 可表达的最细尺度；
- \(\mathcal C_{\ell_t^K}\)：从不同 EventCoordinate 的角几何生成 DensitySites；
- \(\mathcal D_{\ell_t^K}\)：仅由 DensitySite 的相对角距离和派生尺度重建 density；
- \(\mathcal P_K\)：只依据 density 的相对分布特征生成完整的 SampleField，把绝对
  density 固化到 Physical Sample，并以零物理质量 Carrier 补足运输 coverage；
- \(\mathcal I^{\varepsilon}\)：Sample 承接一个以 \(q\) 为作用点、预算为 \(I_0\) 的相位作用并产生 Response；\(\varepsilon=-1\) 表示自然坍缩，\(\varepsilon=+1\) 表示外界反坍缩；
- \(\mathcal B\)：Response 反馈到 EventCoordinate，EventContent 保持不变。

不存在从 Event、DensitySite、density 或外部扰动直接修改已有 EventCoordinate 的旁路。DensitySite、density 与 Sample 必须在 EventCoordinate 改变后全部重新计算；一次事件步的自然相与外界相因此不能共用同一份 Sample。完整的两相顺序见第 7 节。

\(K\) 在一个 Active 场版本内固定，不能因机器负载或事件内容临时改变。
修改 \(K\) 是显式 resolution migration，不是正常事件步，也不是对同一物理
场的无害数值加密。

### 3.2 Sample 的完整状态与采样功能

Sample 不是零体积的方向点，而是整个有限表示共同承担的两种责任。二者同属一个
SampleField，不是两套场，也不能互相代替：

- **运输 coverage \(\phi_j\)**：定义传播载荷当前由哪个 Sample 承接。它覆盖全部
  方向空间，包含 Physical Sample 与 Carrier；
- **物理责任 \(\chi_j\)**：只定义在 Physical Sample 上，定义已拟合的 physical
  density、质量、Response 与回分配由谁承担。它不属于 Carrier。

令 \(\mathcal T=\mathcal P\cup\mathcal C\) 分别为全体、Physical 与 Carrier
Sample 集。\(\phi\) 是 \(\mathcal T\) 上共同导出的 soft partition of unity：

\[
\phi_j(u)\ge 0,
\qquad
\sum_{j\in\mathcal T}\phi_j(u)=1,
\qquad
V_j^{\mathrm T}=\int_{\mathcal D}\phi_j(u)\,d\Omega.
\]

硬 coverage 只是 \(\phi_j(u)\in\{0,1\}\) 的特例，不是理论默认。coverage
不是单个 Sample 私有的物理密度半径；它是整个 Sample 集合的运输所有权，其几何
只由 density 的相对分布特征和版本化投影规则导出，不能由 Response 误差反推。

在 \(\rho_{t,K}(u)>0\) 的 physical 支撑上，Physical Sample 的责任满足：

\[
\chi_j(u)\ge0,
\qquad
\sum_{j\in\mathcal P}\chi_j(u)=1;
\]

在 \(\rho_{t,K}(u)=0\) 的方向，所有 \(\chi_j(u)=0\)。Physical Sample 的完整
物理状态为

\[
S_j^{\mathrm P}=
\bigl(s_j,\phi_j,V_j^{\mathrm T},\chi_j,V_j^{\mathrm P},m_j,\rho_j\bigr),
\]

其中

\[
V_j^{\mathrm P}=
\int_{\rho_{t,K}>0}\chi_j(u)\,d\Omega,
\qquad
m_j=\int_{\mathcal D}\chi_j(u)\rho_{t,K}(u)\,d\Omega,
\qquad
\rho_j=\frac{m_j}{V_j^{\mathrm P}}.
\]

Carrier 的完整状态只有 \((s_j,\phi_j,V_j^{\mathrm T})\)：它可以临时承接传播
载荷与方向矩，却严格满足 \(\chi_j\) 不存在、\(m_j=\rho_j=0\)。因此 Carrier
不是 background density、真空质量或另一个 Response 单位；它不作自然源、不吸收、
不产生 Response，也不进入 \(c/m\) 的回分配。其覆盖到 physical 支撑并不会给它
分配任何物理质量。

由 \(\chi\) 的 physical partition 与 \(\rho=M_{t,K}p\) 可得：

\[
\sum_{j\in\mathcal P}m_j
=\int_{\mathcal D}\rho_{t,K}(u)\,d\Omega
=M_{t,K}.
\]

该等式采用正常场的规范化 \(\rho=M_{t,K}p\)。若只为检验单位协变性而同时做 \(\rho\mapsto c\rho\)、\(\mu_i\mapsto c\mu_i\)，则总广延量变为 \(\mathcal M=cM_{t,K}\)；这种变换不是“同一 \(N_{\mathrm{site}}\) 的另一个合法正常场”，只用于证明表示与回分配不依赖单位选择。

因此 \(m_j\) 是 Physical Sample 所代表的绝对结构量，也是自然相分配场总预算的
广延权重。不能让每个 **Physical** Sample 各自获得一份固定总预算，否则增加 \(K\)
会改变场的动力学。

\(\rho_j\) 是 Physical Sample 产生 Response 的绝对内禀性质，不是在交互时对原始
density 的查询。生成完成后，SampleField 必须独立承担整个交互：若两个 density
在给定分辨率下生成相同的 \((\phi,\chi,m,\rho,c)\)，交互就必须把它们视为不可
区分。想保留更多差异只能提高 Sample 对 density 的拟合精度，不能在交互中开旁路
回看 density。

Sample 不持有 Content、Event 列表或历史。Sample 间的角距离、transport coverage、
邻域和传递关系均由整个 Sample 集合的几何导出；physical responsibility 则只由
同一已拟合 density 的局部结构导出。

### 3.3 全方向空间仍然存在

DensitySite 来自 \(X_t\)，但 \(X_t\) 不是场的边界。\(\rho_{t,K}\) 与后续交互必须定义在完整的 \(\mathcal D\) 上。无 Event 的方向可以具有低 density 或零 density，但不能因此成为不存在、不可采样或不可演化的禁区。

精确 coverage 边界没有本体地位，因此不强制采用硬 cell；但 \(\phi\) 的完整性、
transport volume 和 physical density 总量必须可计算。无 Event 的零-density 方向
仍由 \(\phi\) 和 transport graph 经过，只是所有 \(\chi\) 为零、不会发生物理吸收
或 Response。Carrier 负责这种承运覆盖，不能把它误写成该方向存在 \(\rho>0\)。

### 3.4 Anchor 的地位

该理论不需要 anchor。工程上若为了加速而引入 anchor、centroid、inducing point 或索引，它们只能是可重建缓存，不能持有独立场状态或接受扰动直接写入。

---

## 4. DensitySite：Event 几何的尺度化中间表示

### 4.1 定义与边界

给定场版本预算 \(K\) 与当前 EventCoordinate 支撑集，先导出角尺度，再生成
DensitySites：

\[
\ell_t^K=\mathcal G_K(X_t),
\qquad
C_{t,K}=\mathcal C_{\ell_t^K}(X_t),
\qquad
M_{t,K}=|C_{t,K}|.
\]

在派生尺度 \(\ell_t^K\) 下不可区分的 EventCoordinate 可以由一个
DensitySite 表达；超过该尺度的方向结构必须保持可区分。具体分辨率算子、
球面聚类或覆盖算法属于 implementation contract，理论要求：

- 只使用角距离，不读取 EventContent；
- 对完全重合的 Coordinate 去重；
- DensitySite 不成为持久类别或第二状态；
- 在同一 \(X_t\) 上增大 \(K\) 只能令派生尺度变细或不变，不能减少 DensitySite 数；
- 整体旋转 EventCoordinate 时，DensitySite 随之等变旋转。

DensitySite 是“密度基点”，不保证位于 density 峰值，因此不把它解释为密度峰或物理质点。

### 4.2 相对结构与绝对结构量

\(M_{t,K}\) 表示当前 Event 几何在预算 \(K\) 派生尺度下共有多少可区分的
方向结构。它不是 Event 行数，也不是预先规定的总质量。

由于方向空间紧致，在任意派生出的非零尺度下，\(M_{t,K}\) 存在几何上限。
Event 可以持续增加，但完全重复或在当前分辨率下不可区分的 Event 不会令
它无限增长。

---

## 5. Density：由 DensitySite 距离重建的场量

### 5.1 定义边界

Density 是定义在完整方向空间上的非负分辨率场：

\[
\rho_{t,K}:\mathcal D\rightarrow\mathbb R_{\ge0}.
\]

它只由 DensitySite 的相对角距离和从 \((K,X_t)\) 导出的尺度决定，不依赖
当前扰动或 Response：

\[
\rho_{t,K}=\mathcal D_{\ell_t^K}(C_{t,K}).
\]

例如可以令 \(r_k(u)\) 为方向 \(u\) 到第 \(k\) 个最近 DensitySite 的角距离，并用

\[
\rho(u)\propto
\frac{k}{\Omega(B(u,r_k(u)))}
\]

估计局部 density。该式只是历史候选，不是当前已接受的唯一 estimator。
当前接受的是：density 与 Sample-to-geometry coupling 必须由同一个归一化、
旋转等变、紧支撑 kernel family 导出。kernel profile 与离散归一化仍须由
implementation contract 冻结；它必须只使用角几何、定义于完整方向空间、
重复不变，并在增加分辨率时具有明确极限。

### 5.2 相对形状与绝对结构量

在固定场版本预算 \(K\) 与当前态下，density 分解为：

\[
\rho_{t,K}(u)
=
M_{t,K}\,p_{t,K}(u),
\qquad
\int_{\mathcal D}p_{t,K}(u)\,d\Omega=1.
\]

- \(p_{t,K}\) 表达 density 在各方向的相对形状；
- \(M_{t,K}=|C_{t,K}|\) 表达该分辨率下的绝对结构数量；
- 二者属于同一个 density，不需要二选一。

Sample 的几何排布与分辨率分配只由 \(p\) 的变化特征决定；绝对量 \(M\)
通过各 Sample 固化的 \(\rho_j\) 与 \(m_j\) 保留。因此整体缩放 density 不
改变 Sample 的相对排布。\(K\) 决定场版本能表达多细的结构，但仍不等于
绝对 density 或相位预算。

### 5.3 Sample 生成

给定 Sample 预算 \(K\)，\(\mathcal P_K\) 产生一个对相对 density
\(p_{t,K}\) 的有限区域拟合。由 **Physical Sample** 重建的相对 density 记作：

\[
\widehat p_S(u)
=\frac1{M_{t,K}}
\sum_{j\in\mathcal P}\rho_j\chi_j(u).
\]

Carrier 不参与 \(\widehat p_S\) 或表示误差；它只能保证承运 coverage 完整，不能用
额外质量把空区“拟合”为非零 density。Sample 的保真目标只能测量 \(p\) 与
\(\widehat p_S\) 之间尚未表达的 density 变化，不能读取或最小化下游 Response 误差。
当前 implementation contract 以 total-variation residual 为唯一 density 投影
停止量：

\[
E_{\mathrm{TV}}(p,\widehat p_S)
=\frac12\int_{\mathcal D}
\left|p(u)-\widehat p_S(u)\right|\,d\Omega.
\]

Residual projector 每次只能根据这个尚未表达的 density 残差选择或细化 Physical
Sample；停止条件、候选与 deterministic tie-break 由 implementation contract 冻结。
无论具体求积如何，已确认的是它必须只由 density 的相对分布特征导出。density
平坦的区域可以由更大的 Physical Sample 表示，变化剧烈的区域需要更高分辨率。
整体缩放 \(\rho\) 不得改变 Sample geometry。

普通的 density 加权 CVT 可以提供 coverage、volume、邻接与几何优化的参考，但它最小化的是 density 加权角距离，而非 density 函数自身的变化误差，因此目前不是已确认的唯一采样律。

### 5.4 Density 与 Response 的当前边界

Physical Sample 的绝对内禀量已经在生成时固化为 \(\rho_j\)，其责任形状为
\(\chi_j\)：

\[
S_j^{\mathrm P}=(s_j,\phi_j,V_j^{\mathrm T},\chi_j,V_j^{\mathrm P},m_j,\rho_j).
\]

单源交互核只能接收 Sample 场、当前作用点、相位取向与相位预算：

\[
R^{\varepsilon}
=\mathcal I^{\varepsilon}(S,q,I_0).
\]

自然相把多个正质量 Physical Sample 作用点的单源结果按各自预算叠加；外界相只有
一个输入作用点。不得写成 \(\mathcal I(S,\rho,q)\)。density 的相对特征已经决定
Sample geometry，绝对 density 已经固化为 \((\chi_j,\rho_j)\)；原始 density 在
交互阶段退出作用域。

影响在 Sample 间进行运输，而不是采用带相位、干涉或振荡的波动模型。运输过程可以记录为日志，但不是持久场状态。

这里不再把“绝对 density 是否影响 Response”列为未决问题：寻找 Physical Sample 的
绝对内禀量，本来就是为了让相同环境下的不同 Sample 产生不同 Response。当前采用的
最小本构公理是：\(\rho_j\) 决定 Physical Sample 的局部吸收系数，\(\chi_j\) 决定
其在位置上的责任，Carrier 则只把传播载荷承运到可吸收的位置。

准确说，density 不是完整的“转换率”；它决定单位路径上的转换系数：

\[
\kappa_j=\sigma\rho_j.
\]

其中 \(\sigma\) 是对所有 Physical Sample 相同的场版本常量，而不是新的 Sample
属性或运行期调节量。当前接受的归一化是：在该版本的初始化参考均匀场中，
从作用点到对点的平均光学厚度为 1。implementation contract 必须给出参考
场、求积与数值值；正常事件步不得重估或调节 \(\sigma\)。

因此两种 density 信息承担不同职责：相对分布 \(p\) 的变化特征决定 Sample
geometry，固化在 Physical Sample 中的 \((\chi_j,\rho_j)\) 决定局部吸收强度。
\(\phi\) 只决定载荷的运输所有权，不是 physical density 的平均器。交互仍然只
读取 SampleField，不会回看原始 density。

---

## 6. 守恒运输与纯吸收 Response

本节把有限体积运输与辐射输运的纯吸收部分作为当前工作模型。它不是在宣称场就是电磁辐射，而是复用两条与本系统目的直接相关的结构：Sample 间守恒传递，以及传播载荷到局部 Response 的非负转换。

### 6.1 从作用点到对点的测地射线族

设一次作用发生在 \(q\in\mathcal D\)。从 \(q\) 出发的所有单位切向方向组成发射方向空间：

\[
\mathbb S_q
=\{\omega\in T_q\mathcal D:\|\omega\|=1\}
\cong S^{D-2}.
\]

令 \(d\nu_q(\omega)\) 为 \(\mathbb S_q\) 上归一化的旋转不变测度：

\[
\int_{\mathbb S_q}d\nu_q(\omega)=1.
\]

“作用点向全方向传播”定义为沿所有 \(\omega\) 等权发射，而不是按 Sample 数量或 Sample 邻接边数分配。每个发射方向唯一确定一条从 \(q\) 到对点 \(-q\) 的单位速率测地线：

\[
\gamma_{q,\omega}(r)
=\cos r\,q+\sin r\,\omega,
\qquad
0\le r\le\pi,
\]

其传播方向为：

\[
\dot\gamma_{q,\omega}(r)
=-\sin r\,q+\cos r\,\omega.
\]

因此传播的过程参数 \(r\) 就是真实角路径长度；实现不得用经过的 Sample 数量代替它。除源点与对点外，从 \(q\) 到任意 \(u\) 的传播方向也可直接写成：

\[
\mathbf v_q(u)
=\nabla_{\mathcal D}d(q,u)
=\frac{(q\cdot u)u-q}
{\sqrt{1-(q\cdot u)^2}}.
\]

交互时可访问的局部吸收系数只由 Physical Sample 的责任重建：

\[
\kappa_S(u)
=\sum_{j\in\mathcal P}\chi_j(u)\kappa_j
=\sigma\sum_{j\in\mathcal P}\chi_j(u)\rho_j.
\]

这里 \(\phi\) 与 \(\chi\) 的分离是必要的：前者使零-density 空区仍可由 Sample
graph 承运，后者才决定物理吸收和 Response。Carrier 可以有 \(\phi_j>0\)，但没有
\(\chi_j\)、\(\kappa_j\) 或 Response row；不能因为其 coverage 覆盖某处，就给该处
虚构 absorption。

令 \(I(r,\omega)\) 表示单位发射方向测度上的剩余传播载荷，初始总载荷为 \(I_0\)。每条射线满足：

\[
\frac{\partial I(r,\omega)}{\partial r}
=-\kappa_S(\gamma_{q,\omega}(r))I(r,\omega),
\qquad
I(0,\omega)=I_0.
\]

由于 \(d\nu_q\) 已归一化，\(\int I(0,\omega)d\nu_q=I_0\)。其解为：

\[
I(r,\omega)
=I_0\exp\left(
-\int_0^r
\kappa_S(\gamma_{q,\omega}(s))\,ds
\right).
\]

这里 \(I(r,\omega)\) 是单位归一化发射方向测度上的载荷量，不是球面单位面积 density。方向空间测度与球面面积测度满足：

\[
d\Omega
=|S^{D-2}|\sin^{D-2}r
\,dr\,d\nu_q(\omega).
\]

因此点源在 \(r=0\) 与对点 \(r=\pi\) 附近的单位面积强度可以发散，但沿发射方向保存的总载荷始终有限。本文使用单位球面和以弧度计的无量纲路径；\(\kappa_S\) 因而表示每弧度吸收系数。

Physical Sample \(j\) 在完整传播中吸收的标量载荷定义为：

\[
a_j
=\int_{\mathbb S_q}\int_0^\pi
\chi_j(\gamma_{q,\omega}(r))
\kappa_j
I(r,\omega)
\,dr\,d\nu_q(\omega).
\]

对应的一阶传播方向矩定义为：

\[
\mathbf M_j
=\int_{\mathbb S_q}\int_0^\pi
\operatorname{PT}_{\gamma_{q,\omega}(r)\rightarrow s_j}
\big(\dot\gamma_{q,\omega}(r)\big)
\chi_j(\gamma_{q,\omega}(r))
\kappa_j
I(r,\omega)
\,dr\,d\nu_q(\omega).
\]

这里的平行运输只用于把 Physical Sample 责任区内不同位置的切向量汇总到
\(T_{s_j}\mathcal D\)。当前采用唯一最短测地线上的 Levi-Civita 平行运输，并只在
\(d(\gamma,s_j)<\pi\) 时定义；cut locus 是连续积分中的零测集，数值积分不能把它
作为带非零权重的节点。\(\chi\) 具有局部支撑，不能跨对点任意选择非唯一测地线；
Carrier 没有方向矩积累。

传播到对点后仍未被吸收的总载荷为：

\[
I_{\mathrm{res}}
=\int_{\mathbb S_q}I(\pi,\omega)\,d\nu_q(\omega).
\]

由 \(\sum_{j\in\mathcal P}\chi_j\kappa_j=\kappa_S\) 可得严格账本：

\[
I_0
=I_{\mathrm{res}}+\sum_{j\in\mathcal P} a_j.
\]

该式是纯吸收运输的**原始账本**。当前 v2 不把 \(I_{\mathrm{res}}\) 留成跨相位状态，也不在对点制造一个独立的超级 Response；而是把原始吸收已经给出的空间比例视为本次载荷应落向何处的充分信息。令

\[
A_{\mathrm{raw}}(q)
=\sum_{j\in\mathcal P} a_j
=I_0-I_{\mathrm{res}}.
\]

在 \(A_{\mathrm{raw}}(q)>0\) 时，对完整单源作用做一次全局比例闭合：

\[
\widetilde a_j
=\frac{I_0}{A_{\mathrm{raw}}(q)}a_j,
\qquad
\widetilde{\mathbf M}_j
=\frac{I_0}{A_{\mathrm{raw}}(q)}\mathbf M_j.
\]

于是：

\[
\sum_{j\in\mathcal P}\widetilde a_j=I_0,
\qquad
\|\widetilde{\mathbf M}_j\|
\le\widetilde a_j.
\]

该闭合把残余按已经形成的 Sample Response 比例重新分配到所有吸收位置，保留相对空间分布与方向矩，不再留下对点状态。它是 v2 选择的终点公理，不是局部吸收方程推出的结论。下文无特别说明时，\(a_j,\mathbf M_j\) 均指闭合后的量。

若 \(A_{\mathrm{raw}}(q)=0\)，比例闭合没有定义；这只允许出现在真空运输测试或
尚未完成初始化的场中。正常动力学要求实际 source 具有非零 raw 吸收，不能用任意
方向或伪造 Response 掩盖零吸收。\(A_{\mathrm{raw}}>0\) 也不是充分的数值质量
证明：由于比例闭合会放大极小差异，闭合前还必须通过固定 SampleField 上的版本化
运输细化比较；比较 normalized raw Response 的标量分布和方向矩，而不是引入任意的
绝对 `MIN_RAW_ABSORPTION` 门槛。

上述定义是载荷在 \((r,\omega)\) 射线坐标中的连续参考模型：Physical Sample
提供吸收系数并接收沉积，Carrier 只承运；载荷尚未被表示成 Sample 间的成对通量。
它不能直接冒充“所有运输都发生在 Sample 间”的离散实现；后者必须把同一连续运输
投影为 Sample state 与反对称数值通量，并在 `physics_reference_s2` 中验证细化收敛到
本节定义。

### 6.2 传播载荷、Response 与有限体积守恒

为了保持“交互只发生在 Sample 层”的约束，实际离散必须把传播载荷投影到 Sample。令

\[
U_j=V_j^{\mathrm T}I_j,
\qquad
Q_l=\text{Physical Sample \(l\) 已累计的广延 Response}
\]

分别表示 Sample \(j\) 当前承接的广延传播载荷和 Physical Sample \(l\) 已经累计的
广延 Response。soft coverage 下，“当前由哪个 Sample 以 \(\phi\) 承接载荷”和
“由哪个 Physical Sample 以 \(\chi\rho\) 吸收载荷”不是同一个责任，因此一般守恒
形式需要非负的吸收转移量 \(\mathcal A_{j\rightarrow l}\)：

\[
\frac{dU_j}{d\tau}
+\sum_{k\in\mathcal N(j)}\mathcal F_{jk}
=-\sum_l\mathcal A_{j\rightarrow l},
\]

\[
\frac{dQ_l}{d\tau}
=\sum_j\mathcal A_{j\rightarrow l},
\]

\[
\mathcal F_{jk}=-\mathcal F_{kj},
\qquad
\mathcal A_{j\rightarrow l}\ge0.
\]

若连续传播载荷具有球面 density \(J(u,\tau)\)，则 soft coverage 对应的瞬时吸收 coupling 可写成：

\[
\mathcal A_{j\rightarrow l}
=\int_{\mathcal D}
\phi_j(u)\chi_l(u)\kappa_lJ(u,\tau)
\,d\Omega.
\]

其中 \(\phi_j\) 是载荷所有权，\(\chi_l\kappa_l\) 是 Physical Sample 的吸收责任。
对 \(l\) 求和得到 Sample \(j\) 应扣除的总载荷；对 \(j\) 求和得到吸收 Sample \(l\)
应累计的 Response。于是内部通量和吸收转移分别成对抵消：

\[
\frac{d}{d\tau}
\left(
\sum_{j\in\mathcal T}U_j+\sum_{l\in\mathcal P}Q_l
\right)=0.
\]

当 \(\phi\) 与 \(\chi\) 都是相同的硬 partition 时，吸收 coupling 才退化为对角形式
\(\mathcal A_{j\rightarrow j}\)；先前“Sample 自己扣除多少，就给自己的 Response
增加多少”的写法只在这种特例或经过证明的 mass-lumped 近似下成立，不能作为一般
soft coverage 定律。

soft partition 还可从弱形式导出一个成对通量候选。定义源点 \(q\) 下的有效有向界面系数：

\[
B_{jk}(q)
=\int_{\mathcal D}
\big(
\phi_j\nabla\phi_k
-\phi_k\nabla\phi_j
\big)
\cdot\mathbf v_q
\,d\Omega,
\qquad
B_{jk}=-B_{kj}.
\]

令 \(B^+=\max(B,0)\)、\(B^-=\min(B,0)\)，一阶 upwind 通量候选为：

\[
\mathcal F_{jk}
=B_{jk}^+I_j+B_{jk}^-I_k,
\qquad
\mathcal F_{kj}=-\mathcal F_{jk}.
\]

它使用 soft coverage 的梯度导出有效界面，正向系数读取上游 Sample，且同一界面只计算一次。该式是理论阶段记录的弱形式候选，不是 v2c 第一版的实现算法。
当前 accepted implementation contract 冻结的是另一条明确有限的
`endpoint_radial_finite_v1` graph rule；它只对 semantic384 声明 operational
语义，不能反向冒充这里的连续弱形式或 S2 收敛定理。实现者必须遵循 contract，
不得在两式间自行选择。若未来采用本弱形式，必须新立 ADR、变更 algorithm id
并重新验证一致性、稳定性、非负性和连续 reference 收敛。

离散 state 还必须为每个承运 Sample 保存切向方向矩 \(W_j\)，并保持
\(\|W_j\|\le U_j\)。某条 edge 上的正 scalar flux 从上游取走的 \(W\) 分量必须
按同一比例取走，到达下游时沿该 edge 的唯一短测地线平行运输；吸收时再按同一
\(\phi\)-to-\(\chi\) 责任分解把该方向矩交给 Physical Sample。换言之，方向矩由
**实际经过的 edge** 携带，不能每到一个 Sample 都以“作用点到该中心”的端点径向
向量重算。edge/coupling 的非零权重若触及 cut locus，必须显式失败或在进入运输前
由版本化图规则排除，不能任选方向。

点源的守恒初值投影为：

\[
U_j(0)=\phi_j(q)I_0,
\qquad
\sum_jU_j(0)=I_0.
\]

源点和对点处 \(\mathbf v_q\) 不唯一，连续积分可按零测集排除；数值实现必须把它们作为显式源边界与终点边界，不能在奇点上随意选取一个传播方向。

这里采用“纯吸收”公理：传播载荷的全部扣减都以同一数值计入某个 Sample Response，没有来源不明的损耗项。离散实现必须同时保留两份可审计账本：比例闭合前满足

\[
I_0=I_{\mathrm{res}}+\sum_{j\in\mathcal P}Q_j^{\mathrm{raw}},
\]

比例闭合后满足

\[
I_0=\sum_{j\in\mathcal P}Q_j.
\]

闭合只能按原始 Sample Response 的全局比例缩放，不能把残余直接写入 Event、原始 density 或一个持久对点对象。真空情形只验证原始账本，不进入正常事件步。

### 6.3 局部转换律

辐射输运给出的最小无饱和本构律是 Sample 的吸收系数：

\[
\kappa_j=\sigma\rho_j.
\]

在 soft overlap 的位置 \(u\)，Physical Sample \(j\) 对总吸收率的贡献不是独立执行
一次 \(\kappa_jI\)，而是：

\[
\mathcal A_j(u,I)
=\chi_j(u)\kappa_jI.
\]

只有 \(\phi\) 与 \(\chi\) 相同的硬 coverage 或经过证明的 mass-lumped 离散，才退化成
\(\mathcal A(\rho_j,I_j)=\kappa_jI_j\)。

在沿射线路径长度为 \(\Delta r\) 的一个离散小段上，必须先用同一个总吸收系数计算唯一一次载荷扣减。令

\[
\alpha_j(u)=\chi_j(u)\kappa_j,
\qquad
\kappa_S(u)=\sum_{j\in\mathcal P}\alpha_j(u),
\]

则常系数小段的精确更新为：

\[
I_{\mathrm{out}}
=I_{\mathrm{in}}e^{-\kappa_S\Delta r},
\qquad
\Delta A
=I_{\mathrm{in}}-I_{\mathrm{out}}.
\]

同一次扣减再按各 Sample 对吸收系数的贡献分配：

\[
\Delta a_j
=\begin{cases}
\dfrac{\alpha_j}{\kappa_S}\Delta A,
&\kappa_S>0,\\[6pt]
0,&\kappa_S=0.
\end{cases}
\]

于是 \(\sum_{j\in\mathcal P}\Delta a_j=\Delta A\)。衰减与 Sample Response 不能
分别进行两次数值积分，否则即使各自看似合理，也会产生守恒漂移。

其中“线性于当前载荷”“沿路径指数累计”和“线性于 density”需要分开说明，不能混成一个结论。

对固定 \(\rho\)，若要求局部转换只依赖当前载荷与当前 Sample，并且把同一均匀路径细分成多段不改变结果，那么透过率 \(T_\rho\) 必须满足：

\[
T_\rho(\lambda_1+\lambda_2)
=T_\rho(\lambda_1)T_\rho(\lambda_2),
\qquad
T_\rho(0)=1.
\]

在连续且非负的条件下，这只推出：

\[
T_\rho(\lambda)
=e^{-\kappa(\rho)\lambda}.
\]

也就是说，局部、Markov 和路径细分不变性推出指数形式，却不能单独推出 \(\kappa(\rho)=\sigma\rho\)。线性 density 律还需要一个更强的粗粒化闭合公理：把路径长度分别为 \(\lambda_1,\lambda_2\)、density 分别为 \(\rho_1,\rho_2\) 的相邻区域合并，并只保存加权平均 density

\[
\bar\rho
=\frac{\lambda_1\rho_1+\lambda_2\rho_2}
{\lambda_1+\lambda_2},
\]

合并前后的光学厚度仍完全相同：

\[
(\lambda_1+\lambda_2)\kappa(\bar\rho)
=\lambda_1\kappa(\rho_1)
+\lambda_2\kappa(\rho_2).
\]

再加连续性与真空无吸收 \(\kappa(0)=0\)，才得到：

\[
\kappa(\rho)=\sigma\rho,
\qquad
\sigma\ge 0.
\]

当前工作模型选择接受这条粗粒化闭合公理。它与“Physical Sample 只保存由
\(\chi\) 定义的区域平均 density”相匹配，但仍是 v2 选择的本构关系，不是 density
的几何定义自行证明出来的事实。

令只由 Physical Sample 的责任重建的绝对 density 为

\[
\widehat\rho_S(u)=\sum_{j\in\mathcal P}\rho_j\chi_j(u),
\]

则一条传播路径 \(\gamma\) 的累计厚度、残余载荷和总转换量为：

\[
x_\gamma
=\int_\gamma\sigma\widehat\rho_S(u)\,d\lambda,
\]

\[
I_{\mathrm{out}}
=I_{\mathrm{in}}e^{-x_\gamma},
\qquad
R_\gamma
=I_{\mathrm{in}}\left(1-e^{-x_\gamma}\right).
\]

因此应把先前的表述校正为：**Physical Sample 的绝对 density 与责任 \(\chi\)
共同决定单位路径转换系数；实际瞬时转换率是 \(\sigma\chi_j\rho_j I\)，累计
Response 则由整段路径的 physical-density 积分决定。**

这里的路径暴露 \(d\lambda\) 与 Sample 的 transport volume \(V_j^{\mathrm T}\) 不是
同一个量。\(V_j^{\mathrm T}\) 负责有限体积账本和界面通量；一束传播载荷在
coverage 内实际走过的路径或停留时间，以及其在 \(\chi\) 上的 physical responsibility，
才决定它承受多少吸收。不能把“访问一个 Sample”默认为一次固定吸收，也不能直接用
Sample 数量代替路径长度。

### 6.4 Sample 细分不变性的准确含义

有限体积结构保证的是“每个离散分辨率内部的账本严格守恒”，不是不同 Sample 数一定得到相同的总 Response 或局部 Response 分布。要使守恒与细分收敛成立，离散实现至少必须满足：

- transport coverage \(\phi\) 构成 partition of unity，并由它导出 transport volume
  与有效界面；physical responsibility \(\chi\) 单独导出 \(m_j\)、\(\rho_j\) 与吸收；
- 一次界面转移只计算一个数值通量，并以相反符号同时记入相邻两个 Sample；
- 吸收的载荷扣减与 Response 增量使用同一个离散量；
- 局部更新保持非负，不能从一个 Sample 扣除多于它实际持有的载荷；
- 细分均匀区域时，子区域的路径厚度之和等于原区域厚度。

最后一条与指数透过率的半群性质共同保证：把一个均匀 Sample 沿传播路径拆成两个，不改变总透过率。对非均匀单元，一般存在：

\[
V_j^{\mathrm T}\rho_jI_j
\ne
\int_{\mathcal D}
\chi_j(u)\rho(u)I(u)\,d\Omega,
\]

因为区域平均会丢失 density 与传播载荷在单元内的相关性。因此当前理论只能要求：
每个 \(K\) 下 \(\sum_{j\in\mathcal T}U_j+\sum_{l\in\mathcal P}Q_l\) 严格守恒；在
`physics_reference_s2` 中，随着最大 Sample 直径趋近于零，保守、一致且稳定的
离散应收敛到同一个连续解。不同有限 \(K\) 的总 Response 也可能在收敛前不同。
固定 \(K\) 的 `semantic384` 只承担有限 graph 的 operational invariant，不借此
宣称 \(S^{383}\) continuum convergence。只有父单元内 density 或载荷近似常量，或
Sample 额外保留混合矩时，才能宣称有限分辨率间的精确 Response 不变。

由于 density 的 Sample 平均使用球面面积测度 \(d\Omega\)，而传播光学厚度使用射线路径测度 \(dr\,d\nu_q\)，仅保证球面 \(L^1(d\Omega)\) density 误差收敛并不足以保证运输收敛。Sample 的 density-only 保真标准还必须控制所有测地路径积分，例如：

\[
\varepsilon_{\mathrm{geo}}(p,\widehat p_S)
=\sup_{q\in\mathcal D}
\sup_{\omega\in\mathbb S_q}
\left|
\int_0^\pi
\big(
p(\gamma_{q,\omega}(r))
-\widehat p_S(\gamma_{q,\omega}(r))
\big)
\,dr
\right|.
\]

该候选误差只读取相对 density，不读取某次 Response 或某个实际作用点，因此不违反
Sample 只能由场的内禀 density 特征决定的原则。更强但更简单的充分条件是控制
\(\|p-\widehat p_S\|_\infty\)，此时任意完整测地路径的积分误差至多为
\(\pi\|p-\widehat p_S\|_\infty\)。当前 implementation contract 用
\(E_{\mathrm{TV}}\) 作为 density-only projection gate，并另以
`physics_reference_s2` 的逐 ray/运输细化验证补足其不保证的路径误差；二者的
算法与结论边界以 accepted contract 为准。

### 6.5 Response 的最小内容

纯吸收模型已经决定 Response 的标量预算，却没有决定 Response 如何成为 EventCoordinate 的几何位移。EventCoordinate 位于球面上，其变化必须是当前切空间中的向量；一个非负吸收标量通常不足以唯一决定这个向量。

尤其在作用点的球面对点，多条路径可以从各方向对称汇聚：累计吸收量可以很大，一阶入射方向矩却恰好为零。因此“对点的大残余代表对场的平均影响”只有在区分标量总量与方向矩后才是准确的；不能把大标量自动解释成一个任意方向上的大位移。

因此一次单源运输只在 Physical Sample 上产生无相位 Response：

\[
\mathscr R_j=(a_j,\mathbf M_j),
\qquad
a_j\ge0,
\qquad
\mathbf M_j\in T_{s_j}\mathcal D,
\]

其中 \(a_j\) 是用于守恒账本的吸收量，\(\mathbf M_j\) 是沿源点向外传播方向累计的一阶切向方向矩。标量与方向矩都只是本次交互的充分统计量，不是持久场状态，也不是传播过程日志。完全对称的对点汇聚可以满足 \(a_j>0\) 而 \(\mathbf M_j=0\)，从而保留总量但不凭空制造方向。

相位只给方向矩定向，不给标量吸收量加符号：

\[
\mathscr R_j^{\varepsilon}
=(a_j,\varepsilon\mathbf M_j),
\qquad
\varepsilon=
\begin{cases}
-1,&\text{自然坍缩},\\
+1,&\text{外界反坍缩}.
\end{cases}
\]

因此自然相把接收单位拉向各场内源，外界相把接收单位沿作用点向外的传播方向推开。两相复用同一套运输、吸收与比例闭合；区别不是负吸收或另一套距离函数，而是已经在相位入口固定的方向取向。

### 6.6 Sample Response 的守恒回分配

Sample 代表的是当前 \(K\) 派生尺度下的几何结构，不是数据库中的 Event 行。
令 \(z_i\) 表示第 \(i\) 个可区分几何单位，\(\mu_i>0\) 表示该单位在当前
density 规范下的结构权重；在规范化的 \(\rho=M_{t,K}p\) 中每个单位
\(\mu_i=1\)。同一 Coordinate 上的多个 EventContent 属于同一个单位，在
当前派生尺度下不可区分的 EventCoordinate 也不能从同一次 Sample 交互获得
不同反馈。

为避免与 Sample 预算 \(K\) 混淆，核函数统一记为 \(\mathsf{k}_\ell\)。令已接受的紧支撑 kernel 满足

\[
\mathsf{k}_{\ell_t^K}(u,z_i)\ge0,
\qquad
\int_{\mathcal D}\mathsf{k}_{\ell_t^K}(u,z_i)\,d\Omega=1,
\]

并用同一个 kernel 定义 density 与 Physical Sample-to-geometry coupling：

\[
\rho_{t,K}(u)
=\sum_i\mu_i\mathsf{k}_{\ell_t^K}(u,z_i),
\]

\[
c_{ji}
=\mu_i\int_{\mathcal D}
\chi_j(u)\mathsf{k}_{\ell_t^K}(u,z_i)\,d\Omega,
\qquad j\in\mathcal P.
\]

\(c_{ji}\) 表示几何单位 \(i\) 有多少结构责任由 Sample \(j\) 承担。它是
前向 density-to-Sample 投影在本次交互中的非持久 coupling；由同一 kernel
与 partition of unity 自动得到：

\[
c_{ji}\ge0,
\qquad
\sum_{j\in\mathcal P}c_{ji}=\mu_i,
\qquad
\sum_i c_{ji}=m_j.
\]

并且

\[
\sum_i\mu_i
=\sum_{j\in\mathcal P}m_j
=\int_{\mathcal D}\rho(u)\,d\Omega.
\]

第一项保证 physical responsibility 非负；第二项保证每个可区分几何单位的完整
结构权重只被表示一次；第三项保证 Physical Sample 所代表的结构总量与其 density
mass 一致。Carrier 没有 \(c\) row，因而不进入这些边缘恒等式。density 若仅为验证
尺度不变性而整体缩放，\(\mu_i,c_{ji},m_j\) 一起缩放，不会破坏这组恒等式。由此，
Physical Sample 的带相位方向矩按同一 coupling 的伴随回分配：

\[
\Delta\mathbf P_{i\leftarrow j}
=\frac{c_{ji}}{m_j}
\operatorname{PT}_{s_j\rightarrow z_i}
(\varepsilon\mathbf M_j),
\qquad
m_j>0,
\]

标量吸收量按同一责任分配：

\[
\Delta a_{i\leftarrow j}
=\frac{c_{ji}}{m_j}a_j,
\qquad
a_i=\sum_j\Delta a_{i\leftarrow j}.
\]

其中 \(\operatorname{PT}\) 是球面最短测地线上的平行运输。把分配结果运回 Sample 的切空间后：

\[
\sum_i
\operatorname{PT}_{z_i\rightarrow s_j}
(\Delta\mathbf P_{i\leftarrow j})
=\varepsilon\mathbf M_j.
\]

Carrier 必须满足 \(c_{ji}=a_j=\mathbf M_j=0\)，不进入含 \(1/m_j\) 的回分配；
任何零质量 Physical candidate 也不得以任意除数掩盖真空行，而必须按 contract 的
投影规则删除或令该 SampleField 不可用。

因此守恒的是反馈载荷，而不是不同切空间中坐标位移的普通向量和。若 Sample 同等代表 \(n\) 个几何单位，上式才退化成每个单位获得 \(\mathbf M_j/n\)。一个几何单位中无论挂有多少 EventContent，都不能再次按内容数除小；这些 EventCoordinate 必须接受相同的几何变换。

这形成同一个表示算子的双向使用：

\[
EventCoordinate
\xrightarrow[\text{gather}]{\text{守恒聚合}}
Sample
\xrightarrow[\text{adjoint scatter}]{\text{守恒回分配}}
EventCoordinate.
\]

DensitySite 不在回程中产生第二次作用；它只标识前向投影已经使用的可区分几何单位。真正驱动回分配的仍然只有 Sample Response。

令一个几何单位在一个相位中收到的总带符号反馈载荷为

\[
\mathbf P_i
=\sum_j\Delta\mathbf P_{i\leftarrow j},
\]

由 \(\|\mathbf M_j\|\le a_j\)、平行运输保范数以及 \(\sum_i c_{ji}=m_j\)，每个相位满足：

\[
\|\mathbf P_i\|\le a_i,
\qquad
\sum_i a_i=I_0,
\qquad
\sum_i\|\mathbf P_i\|\le I_0.
\]

当前大场工作公理直接把无量纲切向反馈载荷解释为球面位移：

\[
z_i'
=\operatorname{Exp}_{z_i}(\mathbf P_i).
\]

这里没有 \(\eta\)、阻尼或运行期截断。当单位相位预算 \(I_0=1<\pi\) 时，\(d(z_i,z_i')=\|\mathbf P_i\|\)，所以一次相位在该相冻结的可区分几何单位上的总角位移不超过 1。自然相后会完整重建几何单位，两个相位的单位集合不必一一对应；因此完整事件步只能定义相位账本路径

\[
L_{\mathrm{event}}
=\sum_{i\in G_{\mathrm C}}d(z_i,z_i^{\mathrm C})
+\sum_{k\in G_{\mathrm A}}d(z_k^{\mathrm C},z_k^{\mathrm A})
\le2,
\]

其中 \(G_{\mathrm C}\) 与 \(G_{\mathrm A}\) 是第 7.2 节定义的两个相位各自冻结的几何单位集合。不能把该账本写成固定索引相对事件步初态的总距离，也不能把两个不同切空间中的载荷先相加后只做一次 Exp。\(\mathbf P_i=0\) 时不得任意选取方向；完全对称的汇聚因此不产生虚假位移。该等式是把单位事件载荷定义为单位球面上的无量纲角载荷的本构公理，不是守恒账本单独推导出的事实。

### 6.7 当前仍未由运输模型决定的内容

当前已接受“连续模型作 reference、正式离散采用局部 upwind finite-volume，
并通过反对称界面通量守恒”的方向，但运输模型还没有自动给出：

- soft coverage 上唯一的网格、求积和通量重建如何一致地逼近上述测地射线运输；
- 参考均匀场下 \(\sigma\) 的离散标定值；
- 全局比例闭合后的 Response 空间衰减是否足以产生所需的局部作用范围。

这些问题必须继续在 Sample 层内回答；不能为了补足方向信息而绕过 Sample 读取 Event 或原始 density。
它们的数值答案必须同时注明 backend：`physics_reference_s2` 的目标是连续参考
收敛；`semantic384` 的目标是有限 operational graph 的可审计不变量，不能把前者的
continuum 语言移植为后者未作出的承诺。

---

## 7. 一次事件步的两相动力学

一次事件到来只推进一个事件步，但该状态转移包含两个不可交换的有序相位：旧场先自然坍缩，改变后的场完整重建 Sample，再承接新作用点的外界反坍缩。两相各自使用单位总预算，最后才沉积新 Event。

### 7.1 等预算与自然相的 Sample 源份额

单位事件载荷定义为：

\[
I_0^{\mathrm C}=I_0^{\mathrm A}=1.
\]

相等的是两个相位注入并最终闭合的标量总预算，不是最终坐标位移；自然作用分散于整个场且方向可以互相抵消，外界作用集中于一个作用点，两者一般不会互为逆映射。

在自然相开始时冻结由旧场生成的 Sample 场 \(S_{t,K}^{\mathrm C}\)。令

\[
\mathcal M_{t,K}^{\mathrm C}
=\sum_{h\in\mathcal P}m_{h,K}^{\mathrm C}
=\int_{\mathcal D}\rho_{t,K}^{\mathrm C}(u)\,d\Omega.
\]

每个正质量 Sample \(h\) 作为一次场内作用的源积分元，其源份额为：

\[
w_{h,K}^{\mathrm{src},\mathrm C}
=\frac{m_{h,K}^{\mathrm C}}{\mathcal M_{t,K}^{\mathrm C}},
\qquad
I_{0,h,K}^{\mathrm C}=w_{h,K}^{\mathrm{src},\mathrm C},
\qquad
\sum_{h\in\mathcal P}w_{h,K}^{\mathrm{src},\mathrm C}=1.
\]

“每个正质量 Physical Sample 触发一次”指这组分辨率无关的求积贡献，不是发生
\(K\) 次状态更新，也不是每个 Sample 各发一份单位载荷。Carrier 不作自然源。每个源
\(h\) 必须针对自己的 \(I_{0,h}^{\mathrm C}\) 分别完成第 6.1 节的原始账本与比例闭合，
再汇总所有源的 Response；不能让一个低吸收源的残余由另一个源代为吸收。

自然相显式采用**无自力公理**：源 Sample \(h\) 分配给自身 Sample Response 的标量 \(a_{h\leftarrow h}\) 仍参与账本，但对应方向矩规定为零：

\[
\mathbf M_{h\leftarrow h}=0.
\]

这是因为单个 Sample 只有区域中心和区域平均量，不能从未解析的单元内部制造一个可信的自运动方向；一般 soft coverage 并不保证关于 \(s_h\) 径向对称，所以该零值是明确的自力排除规则，不是假称由 coverage 对称性自动推出。它不改变标量预算，也不允许在源点奇点任意选向。

自然相的总 Response 为：

\[
R_{t,K}^{\mathrm C}
=\sum_{h:m_{h,K}^{\mathrm C}>0}
\mathcal I^{-}
(S_{t,K}^{\mathrm C},s_h,w_{h,K}^{\mathrm{src},\mathrm C}),
\]

其标量吸收量保持非负，方向矩使用 \(\varepsilon=-1\)，所以接收单位被拉向场内作用源，形成聚拢。若 \(\mathcal M_{t,K}^{\mathrm C}=0\)，正常自然相不运行；该状态属于初始化而非小场稳定特例。

正常自然相还要求每个 \(m_{h,K}^{\mathrm C}>0\) 的源都满足 \(A_{\mathrm{raw}}(s_h)>0\)。任一正质量源无法形成 raw 吸收时，整个事件步都不得通过丢弃该源、把其预算转给其他源或重新归一化剩余权重继续运行；这表示初始化门槛尚未满足。外界相同样要求 \(A_{\mathrm{raw}}(q_t)>0\)。

### 7.2 自然相更新后必须完整重建交互面

令 \(G_{\mathrm C}\) 为自然相开始时的可区分几何单位集合。自然相先完成一次独立回分配与球面更新：

\[
E_t^{\mathrm C}
=\mathcal B(E_t,R_{t,K}^{\mathrm C}),
\qquad
z_i^{\mathrm C}
=\operatorname{Exp}_{z_i(t)}
(\mathbf P_i^{\mathrm C}),
\qquad i\in G_{\mathrm C}.
\]

由于 EventCoordinate 已经改变，旧 DensitySite、density 与 Sample 同时失效。外界相之前必须执行完整重建：

\[
X_t^{\mathrm C}
\xrightarrow{\ell_{t,K}^{\mathrm A}=\mathcal G_K(X_t^{\mathrm C})}
\ell_{t,K}^{\mathrm A}
\xrightarrow{\mathcal C_{\ell_{t,K}^{\mathrm A}}}
C_{t,K}^{\mathrm A}
\xrightarrow{\mathcal D_{\ell_{t,K}^{\mathrm A}}}
\rho_{t,K}^{\mathrm A}
\xrightarrow{\mathcal P_K}
S_{t,K}^{\mathrm A}.
\]

只重新移动 Sample 中心、沿用旧 \(\rho_{j,K}^{\mathrm C}\)，或让外界相继续读取 \(S_{t,K}^{\mathrm C}\)，都违反当前态充分性与 Sample 充分性。令完整重建后、外界相开始时的可区分几何单位集合为 \(G_{\mathrm A}\)；它不要求与 \(G_{\mathrm C}\) 一一对应。

### 7.3 外界反坍缩与新 Event 最后落位

新输入为不可变 Content \(c_t\) 与归一化作用方向 \(q_t\)。外界相以 \(q_t\) 为唯一源、以单位预算作用于新重建的 \(S_{t,K}^{\mathrm A}\)：

\[
R_{t,K}^{\mathrm A}
=\mathcal I^{+}(S_{t,K}^{\mathrm A},q_t,1),
\]

\[
E_t^{\mathrm A}
=\mathcal B(E_t^{\mathrm C},R_{t,K}^{\mathrm A}),
\qquad
z_k^{\mathrm A}
=\operatorname{Exp}_{z_k^{\mathrm C}}
(\mathbf P_k^{\mathrm A}),
\qquad k\in G_{\mathrm A}.
\]

外界相使用 \(\varepsilon=+1\)，所以已有几何单位沿从 \(q_t\) 向外的传播
方向移动。读出取自外界相闭合后的非负标量 Sample Response，经同一
coupling 回分配到可区分几何单位，再关联各单位上的 EventContent；不得重算
第二套 cosine 分数。读出发生在该相反馈和新 Event 沉积之前。

输入本身在两个相位开始前不会向 EventCoordinate 支撑集加入一个新几何单位；\(q_t\) 只作为外界边界作用点。若旧场已经有 Coordinate 位于 \(q_t\)，该既有几何单位仍照常参与两个相位并接受反馈，但新 Content 尚不存在，不能造成额外几何重数或当步自力。两个相位结束后才执行：

\[
E_{t+1}
=E_t^{\mathrm A}
\uplus\{(c_t,q_t)\}.
\]

新 Event 出生坐标严格等于 \(q_t\)，不接受自己的当步反馈，落位不再消耗第三份预算；它从下一事件步开始参与自然相。若 \(q_t\) 与已有 Coordinate 完全重合，Content 集合增加，但几何支撑集依照重复不变性不增加重数。

完整顺序因此是：

\[
E_t
\xrightarrow[\text{旧 Sample}]{\text{自然坍缩，预算 }1}
E_t^{\mathrm C}
\xrightarrow{\text{完整重建 Sample}}
S_{t,K}^{\mathrm A}
\xrightarrow[\text{作用点 }q_t]{\text{外界反坍缩，预算 }1}
E_t^{\mathrm A}
\xrightarrow{\text{在 }q_t\text{ 沉积 Content}}
E_{t+1}.
\]

### 7.4 大场适用域与温和位移

v2 的正常动力学是一套大场理论。场状态显式区分 `Building` 与 `Active`：
`Building` 可批量沉积初始 Event，但不得运行正常两相动力学；进入 `Active`
后，任何新增概念都必须逐个走完整事件步。只读 readiness probe 必须验证每个
正质量自然源和代表性外界源均有吸收、coupling 可行、Response 集中度及最大
预测位移满足冻结门槛。系统不靠运行期阻尼或额外步长把一个尚未建成的稀小
场伪装成稳定场。

对任一相位，令几何单位 \(i\) 分得的闭合标量份额为

\[
r_i^{\mathrm{resp}}
=\frac{a_i}{I_0},
\qquad
\sum_i r_i^{\mathrm{resp}}=1,
\]

并定义该相位的有效参与度：

\[
N_{\mathrm{eff}}
=\frac{1}
{\sum_i(r_i^{\mathrm{resp}})^2}.
\]

这里的 \(r_i^{\mathrm{resp}}\) 是接收端 Response 份额，不能与自然相的源权重 \(w_{h,K}^{\mathrm{src},\mathrm C}\)、DensitySite 数 \(M_{t,K}\) 或 Sample 数 \(K\) 混用。由 \(d(z_i,z_i')\le a_i=I_0r_i^{\mathrm{resp}}\) 可得响应份额加权的平均位移上界：

\[
\sum_i r_i^{\mathrm{resp}}d(z_i,z_i')
\le
\frac{I_0}{N_{\mathrm{eff}}}.
\]

在均衡大场中，\(N_{\mathrm{eff}}\) 个参与单位近似等权，典型单点位移因而呈 \(O(1/N_{\mathrm{eff}})\)。一般非均衡场只能保证每相总角位移不超过 1；不能仅凭很大的 \(M_{t,K}\) 或 \(K\) 宣称单点位移必然很小。正常运行的初始化门槛必须以代表性作用点上的实际 \(N_{\mathrm{eff}}\) 与 Response 集中度验证，具体充分门槛属于测试和初始化设计，不在正常动力学中增加调节参数。

### 7.5 遗忘、强化与动态平衡的当前含义

自然相的设计目标是让场内源的反向方向矩在局部形成净聚拢；多源叠加、非对称 coverage 与比例闭合并不自动证明任意一对相邻 EventCoordinate 的距离都会单调减小。该目标必须在局部聚类与长期事件流 fixture 上验证。只要聚拢使 EventCoordinate 在当前 \(K\) 派生尺度下变得不可区分，\(M_{t,K}\) 就会减少；EventContent 没有被删除，但内容之间的几何区分逐渐丢失，这就是当前定义的自然遗忘。

外界相在作用点附近施加反向作用，阻止或逆转当地聚拢；随后新 Event 在 \(q_t\) 沉积。反复受到作用的区域因而更可能保持可区分结构，这就是当前定义的强化。近区外界反坍缩是否稳定压过自然相、远区是否仍由自然坍缩占优，以及长期是否形成非平凡动态平衡，取决于第 6 节运输与全局比例闭合产生的实际空间分布，必须按验证计划测量，本文不把它伪装成已经证明的定理。

---

## 8. 一个 K 预算下的两段保真度

DensitySite 重建与 Sample 离散仍控制不同误差，但它们不再拥有两个互相独立
的运行预算。固定 \(K\) 先通过 \(\mathcal G_K\) 决定可表达的最细尺度，再用
同一个 \(K\) 生成 Sample：

\[
EventCoordinate
\xrightarrow[\text{K 派生尺度下的重建误差}]{N_{\mathrm{site}}(K,X_t)}
DensitySite
\rightarrow Density
\xrightarrow[\text{同一 K 下的离散误差}]{N_{\mathrm{sample}}\le K}
Sample.
\]

- \(N_{\mathrm{site}}(K,X_t)\) 是当前几何与预算共同导出的结果，不是第二个可调上限；
- \(N_{\mathrm{sample}}\le K\) 控制派生 density 的相对变化特征被有限 Sample 场表达到什么精度；Response 不参与尺度、数量或排布的定义。

implementation contract 必须证明 \(\mathcal G_K\) 选出的每个场版本同时满足
重建与离散保真标准。跨 \(K\) 曲线比较的是不同分辨率模型族及其极限，不能
把它标成同一场的纯数值细分。Sample 误差只依赖 density 拟合；Response 只能
验证选定版本的运输，不能反过来指导尺度或 Sample 生成。

---

## 9. 已确认约束

### 9.1 重复不变性

向同一 Coordinate 再挂一个 EventContent，不改变 \(X_t\)、\(\ell_t^K\)、
\(C_{t,K}\)、\(\rho_{t,K}\) 或由它生成的 Sample 分布。它只改变该位置可读出的 Content 集合。

### 9.2 全空间存在性

场的定义域始终是 \(\mathcal D\)。Event、DensitySite 或 Sample 的中心分布不能把无 Event 的方向变成不存在或不可达的区域。

### 9.3 旋转等变性

若所有 Coordinate 和扰动连同其稳定标签一起做同一个正交旋转，非退化情形下
DensitySite、density、Sample 分布和场响应也应只做同样旋转。若有限代表选择
落在 EPS tie 或连续对称退化中，则不存在唯一中心；此时依 accepted gauge 对齐
代表，并要求重建 density/coverage、coupling、Response 与 EventCoordinate 变化等
observable 旋转等变，不能把 ambient coordinate 排序伪装成物理选择。

### 9.4 分辨率单调性

在同一 \(X_t\) 上提高场版本预算 \(K\)，派生尺度只能变细或不变，重建与
Sample 逼近误差只能降低或保持。不同 \(K\) 是显式不同的分辨率模型族；同一
Active 场内不得改变 \(K\)。Sample 的排布不得读取 Response。

### 9.5 历史不可旁路

当前 EventContent–Coordinate 映射相同而 CoordinateLog 不同的两个状态，必须产生相同的场动力学。

### 9.6 Sample 中介性

所有会改变 EventCoordinate 的路径都必须经过真正的 Sample 交互。DensitySite 与 density 只能构造交互面，不能直接写 EventCoordinate。

### 9.7 Content 不变性

任何场更新只能更新 EventCoordinate。EventContent 的变化不属于场动力学。

### 9.8 Sample 充分性

Sample 生成完成后，它是本次交互唯一可访问的场拟合。原始 density、DensitySite 与 EventCoordinate 都不能作为运输计算的额外输入；EventCoordinate 只在最终反馈阶段作为更新目标重新出现。

### 9.9 软覆盖一致性

transport coverage \(\phi\) 默认是非负 partition of unity，硬 cell 只是特例；它定义
transport volume、有效界面和载荷所有权。physical responsibility \(\chi\) 定义
Physical Sample 的 density、质量、前向聚合与 Response 回分配。二者必须由同一个
SampleField 和已拟合 density 一致导出，但不能偷换成同一 membership：Carrier 的
\(\phi\) 可以为正而 \(\chi\) 严格不存在。

### 9.10 反馈重复不变性

Sample Response 按可区分几何单位而非 EventContent 行数守恒分配。同一 Coordinate 上增加 Content 不得稀释、放大或改变该位置得到的坐标反馈。

### 9.11 相位定向反馈

自然相在运输出口显式采用 \(-\mathbf M\)，外界相显式采用 \(+\mathbf M\)。EventCoordinate 的位移方向与该相已经定向的总方向矩一致，不得由 \(\mathcal B\) 再次翻转。方向矩为零时不产生人为选向。

### 9.12 两相顺序与重建

每个事件步必须严格执行“自然相更新旧场 → 完整重建 DensitySite、density 与 Sample → 外界相更新 → 在输入方向落入新 Event”。两相不得共用 Sample，也不得把位于不同切空间的反馈载荷先相加后只做一次 Exp。

### 9.13 等预算与 K 版本边界

自然相和外界相的总预算各为 1。自然相各正质量 Physical Sample 源只能获得
\(m_j/\sum_{k\in\mathcal P}m_k\) 的份额；Carrier 不参与这一求和，固定场版本内的
Sample 数量不得改变相位总预算。
修改 \(K\) 必须走显式 resolution migration，不能作为事件步内调参。

### 9.14 大场适用域

正常动力学只适用于初始化已经建立足够有效参与度的场。小场不通过 \(\eta\)、阻尼、截断或按 Sample 固定载荷获得特殊稳定；是否达到正常运行条件由初始化门槛与验证计划判定。

---

## 10. implementation contract 已闭合的实现选择

以下项目不授权实施者自行选择；唯一算法、常量、边界和失败方式必须由
[v2 实现契约](../specs/field-memory-v2-implementation-contract.md) 冻结。该契约
accepted contract 已逐项冻结下列选择，并构成独立 v2 实现许可。它们仍须按
Phase 0–4 用实验验证；它们是实现公理，不是已经由本文证明的自然定律。

1. \(\mathcal G_K\) 的唯一内禀误差准则、不可行边界、单调算法与 tie-break。
2. \(\mathcal C_{\ell_t^K}\) 的具体角聚类约束、代表 direction 和非唯一解处理。
3. 已接受的紧支撑 kernel family 的具体 profile、归一化与跨派生尺度一致性。
4. \(\mathcal P_K\) 如何依据相对 density 的变化误差生成有限 Sample，同时给出完整
   transport coverage \(\phi\)、transport/physical volume、physical responsibility
   \(\chi\) 与固化 density；Carrier 必须保持零物理质量。
5. density 变化误差应采用单元方差、最坏测地路径积分误差还是更强的统一范数，以及给定 \(K\) 的理论保真上界。
6. soft coverage 上唯一的 upwind finite-volume 网格、求积与通量重建，以及全局比例闭合后的 Response 是否保持足够空间衰减。
7. 参考均匀场、\(\sigma\) 的离散标定值，以及同一 kernel 导出的 coupling 在全部合法 geometry 上的稳定构造。
8. 近外界反坍缩与远自然坍缩的作用范围，以及长期非平凡动态平衡是否存在。
9. 初始化 readiness probe 的充分门槛，以及 Building 到 Active 的失败与恢复协议。

最终被接受的选择不得修改已经确立的因果边界：**场是完整方向空间中的当前态；Event 是持久拟合；DensitySite 只拟合 density；density 决定真正的 Sample；Sample 是唯一交互面；只有 Sample Response 可以反馈已有 EventCoordinate；自然相更新后必须重建交互面，外界相结束后新 Event 才在输入方向落位。**
