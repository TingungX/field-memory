# field-memory v2 实现契约

Status: accepted
Owner: field-memory
Last updated: 2026-08-13
Scope: v2 独立核心、Density/Sample 有限表示、两相动力学、持久化、CLI 与受控验证
Related code: crates/core/field-mem-v2/、crates/ext-v2-cli/、experiments/v2_validation/
Related docs: [v2 理论基石](../design/field-memory-v2-foundations.md)、[v2 权威与分辨率 ADR](../decisions/2026-08-12-v2-authority-resolution-and-dimension.md)、[384D ADR](../decisions/2026-08-12-v2-semantic-dimension-384.md)、[Sample 投影与 backend 边界 ADR](../decisions/2026-08-13-v2-sample-projection-and-backend-boundary.md)、[v2 验证计划](2026-08-10-field-memory-v2-validation-plan.md)

## 0. 契约地位

本文冻结 v2 第一版唯一可实施算法：

    algorithm_id = "fm-v2c-wendland-residual-edge-fvm-v2"

本文已经 accepted，实施者可以创建独立 v2 动力学代码。实现必须逐式镜像本文；
accepted 不表示本构公理已经被自然定律证明。Phase 0 至 Phase 4 的职责是证伪、
量化并验证。任何改变公式、候选集合、tie-break、常量或验收门的修改都必须先以
新 ADR 修改本文并更换 algorithm_id，不能藏在代码或运行参数中。

v2 与 v1 完全隔离。禁止依赖或搬用 Anchor、impact、stiffness、damping、
RelaxationCycle、v1 persistence、v1 CLI/server schema。允许复用的只有第三方
基础库和通用的原子文件替换写法。

本版本有两个不可互换 backend：

1. physics_reference_s2 是唯一的连续几何、cubature 与 ray reference；
2. semantic_production_384 是只定义在 DensitySite 控制点上的有限 operational
   model；它不宣称在 S^383 上构造连续网格或连续求积。

二者共享 Event -> DensitySite -> Density -> Sample -> Response -> EventCoordinate
因果链、双责任 Sample、账本和两相顺序；它们不能互相替代或互相冒充 artifact。

## 1. 交付边界与模块

### 1.1 workspace

新增两个 package：

    crates/core/field-mem-v2
    crates/ext-v2-cli

根 workspace 只新增这两个 member。v1 package、CLI、server 与前端不改。v2
默认 library 路径为：

    libraries-v2/<library>/snapshot.json

该路径不能被 v1 server 扫描。根 Cargo.lock 必须纳入版本控制；依赖解析以
Cargo.lock 为复现真理源。

### 1.2 核心文件职责

    field-mem-v2/src/
      lib.rs          仅导出窄公共 API
      error.rs        结构化错误
      numeric.rs      f64、Kahan、log-domain 与容差
      geometry.rs     球面距离、Exp、Log、PT、frame
      version.rs      FieldVersion 与 backend identity
      event.rs        EventContent、EventCoordinate、CoordinateLog
      resolution.rs   G_K、GeometryUnit、DensitySite
      kernel.rs       Wendland、logZ 与 S2 reference cubature/ray
      sample.rs       residual projector、Sample、phi/chi、coupling
      transport.rs    graph FVM、方向矩与 raw/closed ledger
      response.rs     adjoint scatter、readout
      step.rs         Building/Active 与两相事件步
      persist.rs      v2 snapshot 原子保存/加载

ext-v2-cli 只做参数解析、embedding adapter、调用核心和自动保存；不得持有第二
份动力学。

### 1.3 唯一调用图

    EventCoordinate
      -> GeometryUnit
      -> DensitySite
      -> normalized physical Density
      -> Physical/Carrier SampleField
      -> graph Transport
      -> Sample Response
      -> coupling adjoint scatter
      -> EventCoordinate

transport 只能读取冻结 SampleField、source direction 与 source budget。它不能
读取 Event、DensitySite、原始 density 或 Content。response scatter 只能读取
TransportResult 与 SampleField 已固化的 coupling。Event 只在最终坐标更新和
Content readout 时重新进入。

## 2. 数值域、版本与常量

### 2.1 数值域

- 环境向量维度 D >= 2，方向空间为单位球面 S^(D-1)。
- 核心标量、坐标、矩、求积和 snapshot 使用 f64。
- 核心测度是 normalized sphere measure dω；整球 volume 为 1。
- theta(x,y) = acos(clamp(dot(x,y), -1, 1))，单位为 rad。
- 外部坐标必须 finite、长度大于零；按 index 升序 Kahan 求平方和后归一化。
- 所有 NaN、Infinity、负质量、非法维度或非法预算都返回结构化 Result error。

S2 artifact 额外报告 surface_area = 4*pi 与 physical_volume = 4*pi*V；核心
计算不把 4*pi 带入 density 或 transport。384D artifact 只记录球面积符号公式。

### 2.2 冻结常量

    STATE_SCHEMA_VERSION        = 2
    ARTIFACT_SCHEMA_VERSION     = 1
    DEFAULT_SAMPLE_BUDGET       = 64
    MIN_SAMPLE_BUDGET           = 4
    ACTIVE_MIN_GEOMETRY_UNITS   = 32
    ACTIVE_MIN_N_EFF            = 32
    ACTIVE_MAX_SITE_SHARE       = 0.125
    ACTIVE_MAX_ANGLE_RAD        = 0.125

    EPS_ABS                     = 1e-10
    EPS_REL                     = 1e-8
    EPS_ANGLE                   = 1e-9
    EPS_LEDGER                  = 1e-9
    EPS_KERNEL                  = 1e-11
    EPS_QUADRATURE              = 5e-5
    EPS_REPRESENTATION          = 0.03125
    EPS_TRANSPORT_DISTRIBUTION  = 1e-5
    CUT_LOCUS_MARGIN_RAD        = 1e-6

    SCALE_LEVEL_MAX             = 20
    PHYSICAL_COVERAGE_FACTOR    = 2.0
    CARRIER_OFFSET_RAD          = 0.10
    CARRIER_RADIUS_RAD          = pi/2 + 0.10
    SIGMA                       = 1/pi

    FVM_CFL_COARSE              = 0.45
    FVM_CFL_FINE                = 0.225
    FVM_HMAX_COARSE             = pi/32
    FVM_HMAX_FINE               = pi/64
    FVM_MAX_STEPS               = 65536

    KERNEL_GL_ORDERS            = [256, 512, 1024]

这些是 algorithm version 的常量，不是运行参数。唯一公开的版本参数是 backend
identity 与 Sample budget K；同一 Active 场内 K 不变。

不存在 MIN_RAW_ABSORPTION、eta、阻尼、恢复率、事件来源权重、runtime scale、
runtime sigma、response-guided sampling 或高维 cubature seed。

### 2.3 账本容差

对预算 I0 的账本，统一比较 normalized error；绝对容差为
I0 * EPS_LEDGER。向量比较使用欧氏范数。小于零但大于
-I0 * EPS_LEDGER 的 roundoff 必须作为 ledger_correction 显式入账后归零；
更小的负值失败。

### 2.4 球面几何原语

所有 backend 共用以下唯一 f64 几何定义。单位 `x,y`、`c=clamp(dot(x,y),-1,1)`：

    theta(x,y) = acos(c)
    tangent_project_p(v) = v - dot(v,p)*p
    Exp_x(v) = cos(norm(v))*x + sin(norm(v))*v/norm(v)
    Log_x(y) = theta(x,y) * normalize(y-c*x)

Exp 的 `norm(v)<=EPS_ABS` 分支返回 x；Log 的 `theta<=EPS_ANGLE` 分支返回零；
`theta>=pi-CUT_LOCUS_MARGIN_RAD` 时 Log 失败。输出只允许为 roundoff 做一次
normalize/tangent projection，并验角度/范数 residual。

对 `v in T_x` 且 `theta(x,y)<pi-CUT_LOCUS_MARGIN_RAD`，唯一最短测地 PT 为：

    PT_(x->y)(v) = v - (dot(v,y)/(1+dot(x,y))) * (x+y)

`theta<=EPS_ANGLE` 时返回 `tangent_project_p(v)` with `p=y`；cut-locus margin 内非零权重调用
失败。PT 后投影到 `T_y`，但不得改变方向或人为 renormalize；验证 tangency 与
`abs(norm(out)-norm(v))<=EPS_ABS+EPS_REL*norm(v)`。源/对点的显式零矩分支优先，
零权重路径不得先调用 PT。

`v_q(s)` 在非奇点固定为：

    ((dot(q,s))*s - q) / sqrt(1-dot(q,s)^2)

并验证它位于 `T_s`、范数1；`theta(q,s)<=EPS_ANGLE` 或
`>=pi-EPS_ANGLE` 时使用第7.3节零分支。禁止由 ambient axis 补传播方向。

## 3. FieldVersion、持久状态与公共 API

### 3.1 FieldVersion

FieldVersion 至少包含：

    state_schema_version: 2
    algorithm_id: String
    scalar: "f64"
    dimension: u32
    sample_budget: u32
    backend_kind: "physics_reference" | "semantic_production"
    space_kind: "physics_reference_s2" | "semantic_production_384"
    measure: "sphere_normalized"
    kernel_id: "wendland_c2_log_normalized_v2"
    projection_arithmetic: "f64_index_kahan_v1"
    sample_projection_id: "residual_greedy_controls_v2"
    graph_id: "endpoint_radial_finite_v1"
    gauge_id: "density_label_frame_v1"
    sigma: f64                 # 必须逐 bit 等于 1/pi 的版本常量
    dataset_sha256: null | String
    embedding_identity: null | EmbeddingIdentity

合法组合只有：

1. physics_reference / physics_reference_s2 / D=3 / dataset_sha256=null /
   embedding_identity=null；
2. semantic_production / semantic_production_384 / D=384 / 完整 frozen identity。

semantic identity 必须逐字段相等：

    model = "bge-m3:latest"
    model_digest = "7907646426070047a77226ac3e684fbbe8410524f7b4a74d02837e43f2146bab"
    dataset_sha256 = "ed5a9836f277ea8242fce0bd477363bb21e81d606226dc607b92554d7b7d9901"
    source_dimension = 1024
    target_dimension = 384
    projection_family = "uncentered_spherical_pca_eigh_v1"
    projection_archive_sha256 = "2ba52a192937c9d4f8b6ab34980cf733539c9417af1b4db7c5c9ae636d80dd68"
    projection_content_sha256 = "17f2932d84ce59107e1f080ddd9279e30390fc6822f41a89f75b72a5cd8cbc11"
    ollama_version = "0.21.2"

FieldVersion.dataset_sha256 与 EmbeddingIdentity.dataset_sha256 必须相等。
FieldVersion.validate() 必须逐字/逐 bit 校验本节全部 frozen id/常量字段，并拒绝
`dimension < 2`、`sample_budget < MIN_SAMPLE_BUDGET`、
`sigma.to_bits() != (1.0_f64/pi).to_bits()` 以及任何 identity 混搭；只保存
dimension=384 不足以加载生产场。

### 3.2 持久状态

    FieldState {
      version,
      lifecycle: Building | Active,
      next_event_id: u64,
      next_step_id: u64,
      events: Vec<Event>,
      coordinate_log: Vec<CoordinateLogEntry>
    }

    Event {
      id: u64,
      content: String,
      coordinate: Vec<f64>
    }

    CoordinateLogEntry {
      step_id: u64,
      event_id: u64,
      phase: Natural | External,
      before_coordinate: Vec<f64>,
      after_coordinate: Vec<f64>
    }

next_event_id 与 next_step_id 均从 1 开始。成功的 `deposit_building` 只消费一个
EventId；成功的 `apply_event` 只消费一个 EventId 和一个 StepId；`activate`、
readiness/query/save/load 不消费任何 ID。mutation 失败时两个 ID 均不消费。
Content 字节永不由动力学修改。

DensitySite、density、SampleField、Response、readout 和 transport trace 全部是
即时派生状态，不持久化。CoordinateLog 仅审计；从数值真理源删除它不得改变
下一步动力学。

### 3.3 公共 API

    new_building(version) -> FieldState
    deposit_building(state, content, coordinate) -> EventId
    derive_resolution(state) -> DerivedResolution
    build_sample_field(state) -> SampleField
    readiness(state) -> ReadinessReport
    activate(state, report_sha256) -> ()
    apply_event(state, content, coordinate) -> EventStepResult
    query_read_only(state, coordinate) -> ReadoutResult
    save_v2(path, state) -> ()
    load_v2(path, expected_version) -> FieldState

query_read_only 允许执行 ephemeral scalar scatter 以产生 readout；禁止 moment
scatter、Exp、append、日志和持久状态变化。

## 4. EventCoordinate、GeometryUnit 与 G_K

### 4.1 几何去重

按 EventId 升序扫描当前 Event，并维护已建立 GeometryUnit。一个坐标若与多个
既有代表的角距均 `<=EPS_ANGLE`，归入 `geometry_label` 最小的代表，否则创建新
GeometryUnit；`geometry_label` 是该 unit 第一个、也即最小的 EventId，只充当
版本化数值 gauge。这里是 representative-greedy 去重，不做传递闭包：A 接近 B、
B 接近 C 不推出 A 与 C 合并。精确重复只能以更大的 EventId 后加入，因而不改变
代表。geometry_label 只在 EPS 不可分辨或内禀严格对称的 tie 中使用；普通几何
选择不得按 ambient coordinate bytes 排序。

去重只确定当前不可区分的几何支撑。增加同坐标 Content 只增加成员 EventId；
不得改变 GeometryUnit 数、density、SampleField 或 physics_sha256。Content/member
mapping 使用独立 mapping_sha256。

### 4.2 尺度 ladder 与 DensitySite

唯一候选尺度：

    ell_n = pi * 2^(-n),  n = 0..20

先为每个 GeometryUnit g 计算旋转不变内禀签名：

    intrinsic_signature(g) = sort_ascending([theta(g,h) for every h])

两个签名按位置字典序比较；第一个绝对差大于 EPS_ANGLE 的分量决定次序，所有
分量差均不超过 EPS_ANGLE 才视为 gauge tie，并取 geometry_label 较小者。对每个
ell_n，在 GeometryUnit 代表上构造确定性 farthest-point prefix：

1. 第一个中心是 intrinsic_signature 最低的 GeometryUnit；
2. 每次计算每个未选单位到已选中心的最小角距；
3. 取最大值；距最大值不超过 EPS_ANGLE 的候选中先取 intrinsic_signature
   最低者，仍 tie 才取 geometry_label 最低者；
4. 最大值 <= ell_n 时停止。

每个 GeometryUnit 归到最近中心；距相差不超过 EPS_ANGLE 时使用同一
intrinsic_signature/geometry_label 次序。每个 cluster 是当前尺度下一个
DensitySite；site direction 是被选中心的当前 coordinate，SiteId 按 FPS 选择
顺序从0递增，site mass mu_i = 1。

    M = number_of_sites = sum_i mu_i

cluster 中所有 GeometryUnit 在反馈时接受同一个 site 载荷。空场没有
DensitySite、density 或 SampleField，保持 Building。

### 4.3 G_K

对每个 ell_n，执行第 6 节的完全确定性 residual-greedy 投影。N_req 不是一个
组合优化或存在性最小值；它的定义是：

    N_req^greedy(p_ell, epsilon)
      = 第一个通过全部结构门且 E_TV <= epsilon 的 greedy prefix 的实际节点数

候选耗尽或结构门失败时 N_req^greedy = infinity。唯一分辨率算子选择最细可行
尺度：

    G_K(X) = 数值最小的 ell_n，
             满足 N_req^greedy(p_ell_n, 1/32) <= K

无解返回 ResolutionInfeasible。状态仍可在 Building 沉积更多 Event，但不能
Active。greedy 路径不依赖 K；因此 K 增大只会允许更长的既有 prefix，G_K 只能
变细或不变。API 不接受独立 ell。DerivedResolution 记录每个被尝试尺度的
site count、N_req^greedy、E_TV、结构失败原因与最终 witness。

## 5. Wendland kernel 与 backend 的 density 定义

### 5.1 共同 profile

定义：

    psi(t) = (1 - t)^4 * (1 + 4t),  0 <= t < 1
    psi(t) = 0,                      t >= 1

对 t >= 1，log_psi(t) 为 negative infinity；对 0 <= t < 1：

    log_psi(t) = 4 * log1p(-t) + log1p(4t)

所有 pair evaluation 都从 log_psi 开始，并通过 log-sum-exp 归一化。禁止
Gaussian/cosine softmax 或 runtime kernel choice。

### 5.2 physics_reference_s2：连续 Wendland 与唯一连续 reference

physics_reference_s2 只在 D=3 上使用连续归一化 kernel：

    log_k_3_ell(u, z) = log_psi(theta(u,z)/ell) - logZ_3(ell)

    logZ_D(ell) = log[
      Gamma(D/2) / (sqrt(pi) * Gamma((D-1)/2))
      * integral_0^ell psi(r/ell) * sin(r)^(D-2) dr
    ]

归一化使用正权重的一维 Gauss-Legendre log-sum-exp。依次计算 256、512、1024
阶；相邻两阶 `L_prev,L_next` 的相对差唯一为
`abs(expm1(L_next-L_prev))`，其值 `<=EPS_KERNEL` 时接受后一阶；1024 仍不通过则
KernelUnresolved。实现和持久化只保存 logZ，绝不保存或以普通域重新计算 Z。
不得用有限球面节点之和重新定义 Z。

连续 density 为：

    rho(u) = sum_i mu_i * exp(log_k_3_ell(u, z_i))
    p(u) = rho(u) / M

因此 integral rho dω = M，integral p dω = 1。所有连续 density、phi、chi、
coupling、volume、E_TV 与 A 积分都使用同一套 support-aware adaptive S2 cubature，
不得以固定节点漏掉紧支撑。

该 cubature 的唯一版本为 `s2_cube_face_adaptive_gl_v1`，完全由公式生成，不读取
外部 quadrature asset。六个初始 face cell 均为参数方形 `[-1,1]^2`，顺序与
右手 frame 固定为：

    face 0: n=+x, e1=+y, e2=+z
    face 1: n=-x, e1=+y, e2=-z
    face 2: n=+y, e1=-x, e2=+z
    face 3: n=-y, e1=+x, e2=+z
    face 4: n=+z, e1=+x, e2=+y
    face 5: n=-z, e1=-x, e2=+y

每行满足 `cross(e1,e2)=n`。chart、Jacobian 与 normalized measure 为：

    X_f(a,b) = normalize(n + a*e1 + b*e2)
    J_f(a,b) = (1 + a^2 + b^2)^(-3/2)
    dω = J_f(a,b) da db / (4*pi)

cell `C=[a0,a1]x[b0,b1]` 的角点依次 `v00,v10,v11,v01`。定义：

    Omega(x,y,z) = 2*atan2(abs(det(x,y,z)),
                    1 + dot(x,y) + dot(y,z) + dot(z,x))
    area(C) = (Omega(v00,v10,v11) + Omega(v00,v11,v01))/(4*pi)

coarse 固定 `GL2xGL2`：node `+-1/sqrt(3)`、weight 1。fine 固定 `GL4xGL4`；令：

    x1=sqrt((3-2*sqrt(6/5))/7), w1=(18+sqrt(30))/36
    x2=sqrt((3+2*sqrt(6/5))/7), w2=(18-sqrt(30))/36
    ordered nodes=[(-x2,w2),(-x1,w1),(x1,w1),(x2,w2)]

一维 node 仿射映射到 cell；二维 raw weight 是两维仿射 Jacobian、GL weight、
`J_f/(4*pi)` 的乘积，再将该规则的全部 raw weight 同比缩放到精确
`sum weight=area(C)`。cell、node、输出分量均按 face/path/i/j 升序 Kahan 累加。

cell center 是参数中点映射，保守外接半径为：

    R_C = min(pi, hypot((a1-a0)/2,(b1-b0)/2) + EPS_ANGLE)

chart pullback metric 最大特征值不超过1，因此该半径不得缩小。对 support cap
`(z,r)`，只有 `r<pi-EPS_ANGLE` 且
`theta(cell.center,z)>R_C+r+EPS_ANGLE` 时才可证明 cell 与该 cap 相离；否则必须
计算或细分。density `(z_i,ell)`、physical responsibility `(s_j,2*ell)` 与所有
transport profile `(s_j,r_j)` 都注册到 support union。不能因 GL node 恰好全落在
support 外而返回解析零。

每个被积输出分量必须提供非负 activity 上界 `B_l(C)>=sup_C abs(f_l)`，唯一
递推如下。primitive profile 用
`d_min=max(0,theta(cell.center,z)-R_C)` 与单调 Wendland 上界；`phi,chi` 上界固定
为1；sum 用各项上界之和，product 用各非负上界之积，absolute difference 用
两边上界之和，标量常数取绝对值相乘。由此 V/O 上界为1，`c_ji` 上界为
`mu_i*exp(-logZ_i)*psi(d_min/ell)`，A 上界为 `SIGMA*rho_l`，p/p_hat/E_TV
按上述 sum/product 规则从 kernel 与已冻结 rho 唯一递推。禁止以采样最大值替代
这个上界。若
coarse/fine activity 都为零但 cell 仍可能与 support 相交，必须给误差加
`area(C)*B_l(C)`，不得把 underflow 当解析零。cell error 是所有分量
`e_l=abs(Q4_l-Q2_l)+missing_bound_l`。对每个分量，全局 tolerance 固定为
`tol_l=EPS_QUADRATURE+EPS_REL*max(1,abs(sum_leaf Q4_l))`；仅当
`sum_leaf e_l<=tol_l` 对全部分量成立时停止并固化 fine sum。cell normalized error
固定为 `max_l(e_l/tol_l)`。否则每轮只四分 normalized
error 最大 cell；并列按 `(face_id,base4_path)`，子 cell 顺序 `00,01,10,11`
（先低 a 再低 b）。最大深度24、最多2,000,000个 leaf；非 finite、非法上界或
到限仍不收敛均为 `CubatureUnresolved`，artifact 记录 cell/path/component witness。

S2 ray reference 是独立的逐 ray 连续诊断，不用有限 ray 集冒充对整个
`omega` 空间的连续积分。fixture 对每条 ray 逐 f64 物化单位 source q、单位切向
omega 与可选 normalized diagnostic weight；要求 `abs(dot(q,omega))<=EPS_ANGLE`。
权重只形成 fixture 明示的离散 aggregate，不作为连续 `dnu_q` quadrature。

`origin=analytic_reference` 的 `uniform_ray_oracle` 不构造 Sample、chi 或 graph；
它唯一允许的 opacity 分支是
`kappa_S=fixture.sigma*fixture.density.value=1/pi`，只计算 total opacity、I_res 与
零/对称 moment oracle。它不得产生 per-Sample Response，也不得进入 G_K、readiness
或 persistence。普通 state/direct SampleField ray 必须使用下述 chi/rho 定义，
不得借 analytic 分支绕过 Sample。

固定：

    gamma(r)=cos(r)q+sin(r)omega, r in [0,pi]
    gamma_dot(r)=-sin(r)q+cos(r)omega
    kappa_j(u)=SIGMA*chi_j(u)*rho_j
    kappa_S(u)=sum_j kappa_j(u)

初始端点为0与pi。每个 density/chi cap 与 ray 的交点都解析加入端点：令
`A=dot(q,z), B=dot(omega,z), H=hypot(A,B), delta=atan2(B,A)`；对 cap radius r，
若 `H>0` 且 `abs(cos(r)/H)<=1+EPS_ANGLE`，加入 `(0,pi)` 内所有
`delta+-acos(clamp(cos(r)/H,-1,1))+2*k*pi, k in {-1,0,1}`。端点按值升序，距
不超过 EPS_ANGLE 时保留较小值。

每段以 GL8/GL16 exponential-product rule 比较。GL node/weight 不用 asset：
Legendre recurrence 从 `cos(pi*(k-1/4)/(n+1/2))` 起固定做16次 Newton，权重
`2/((1-x^2)*P'_n(x)^2)`，按对称性补根后升序；非 finite、非正或非严格有序失败。
第5.1节 logZ 的 GL256/512/1024 使用完全相同的 root/weight 生成器，再仿射映射
到 `[0,ell]`；不得调用平台黑盒 quadrature table。
按 r 升序处理 node；映射权重 w 下：

    d_tau = w*kappa_S(gamma(r))
    I_new = I_old*exp(-d_tau)
    d_abs = I_old-I_new
    d_a_j = d_abs*kappa_j/kappa_S
    d_M_j = PT_(gamma(r)->s_j)(gamma_dot(r))*d_a_j

`kappa_S=0` 时无变化；按 SampleId 分配，最后一个正 kappa 接收 f64 remainder，
使每个阶数自身严格满足 `sum_j d_a_j + I_res = I0`。比较 GL8/GL16 的 opacity、
I_res、每个 a 与 M 坐标；每个标量/坐标的唯一容差为
`EPS_QUADRATURE+EPS_REL*max(1,abs(value_GL16))`。任一分量超限则该段二分且左段
先行，最大深度32、每 ray最多
1,000,000个逻辑 node，越界为 `RayReferenceUnresolved`。PT 只在
`theta(gamma,s_j)<pi-CUT_LOCUS_MARGIN_RAD` 时计算；GL node 不含端点，source/
self/antipode endpoint moment 按第7.3节零分支。finite graph 只与已注册逐 ray
observable 比较趋势；本版不声称用四条或任意有限 ray 重建完整角向积分。

### 5.3 semantic_production_384：有限 column-stochastic operational kernel

semantic_production_384 完全定义在当前 M 个 DensitySite 控制点上：

    controls = { (z_i, omega_i) | i = 1..M }
    omega_i = 1 / M

它没有 S^383 cubature、rank quotient、Gaussian node、Monte Carlo node 或连续
积分陈述。每个尺度只按需计算 pair log-profile，不能 materialize M x M kernel
matrix。只要 log-domain pair evaluation 可计算，该尺度即可进入 G_K；不以大
矩阵分配作为尺度可行性门。

定义有限 column-stochastic operational kernel：

    logK_ai = log_psi(theta(z_a, z_i)/ell)
              - logsumexp_b(log_psi(theta(z_b, z_i)/ell))

    K_ai = exp(logK_ai)

每个列 i 的 self pair 严格非零，因此 logsumexp 有定义且：

    K_ai >= 0
    sum_a K_ai = 1

连续归一化常数 Z 在列归一化中精确抵消；semantic backend 不计算、保存或使用
Z。其唯一 density truth 是：

    r_a = sum_i mu_i * K_ai
    p_a = r_a / (M * omega_a)

因此：

    sum_a omega_a * p_a = 1

同一 K_ai 也必须用于第 6 节 coupling；禁止另算一套 pair-kernel density、
c 或 mass。它是版本化 finite operational density，不是连续球面 density 的
数值求积替身。

## 6. Sample 投影、双责任与 coupling

### 6.1 Sample role

每个 Sample 都是同一 SampleField 的一个节点；它有 transport capability 与
可选 physical capability：

    Carrier:
      broad_carrier_profile = true
      physical = false

    Physical:
      broad_carrier_profile = false
      physical = true

    CarrierPhysical:
      broad_carrier_profile = true
      physical = true

CarrierPhysical 不是额外节点，而是与 carrier 同方向的 physical candidate 对
已有 carrier 节点的升级。它保留 carrier 的 broad transport profile，同时获得
physical responsibility。这样不产生零长度 edge，也不牺牲全域 coverage。

`broad_carrier_profile` 只选择 broad carrier radius，不决定节点是否参与运输。
所有三种 role 都是 transport node，并参与 phi、V^T、U/W 与 graph。
Carrier-only 的 mass、density、c row、Response row 严格为零；它可保存传播中的
U/W，并可拥有 A 的 owner row。任何 physical Sample（包括 CarrierPhysical）
有 chi、mass、density、Response 与 c row。

### 6.2 O(M) 候选、gauge 与去重

physical candidate pool 只含：

1. 每个 z_i；
2. 每个 -z_i；
3. 两个确定性 carrier direction。

不得加入 pair sum、pair difference、随机方向、Response 方向或 Content 方向。
因此候选生成与存储均为 O(M)。

b 是最低 SiteId 的 direction。对按 SiteId 升序的每个 site direction z，先算

    t_raw = z - dot(z,b) * b

取第一个 `norm(t_raw) > EPS_ANGLE` 的 site，并令 `t=normalize(t_raw)`。若不存在，
在 canonical ambient axes `e_0,e_1,...,e_(D-1)` 中取 `abs(dot(e_k,b))`
最小者；值相差不超过 EPS_ABS 时取最小 axis index。随后同样投影并归一化得到 t。
carrier direction 为：

    c0 = b
    c1_raw = -cos(CARRIER_OFFSET_RAD) * b + sin(CARRIER_OFFSET_RAD) * t
    c1 = normalize(c1_raw)

必须验证 `abs(dot(t,b))<=EPS_ABS` 与 `abs(norm(c1)-1)<=EPS_REL`。它们先作为
Carrier 节点加入。candidate 按 candidate_key 升序扫描并维护已建立代表 Vec；
一个方向若同时落入多个 `EPS_ANGLE` 邻域，归入 key 最小的代表；这里同样不做
传递闭包。代表取 candidate_key 最小者。candidate_key 的 wire tuple 唯一为：

    (kind_rank: u8, origin_id: u64, sign_rank: u8)

固定编码：`carrier c0=(0,0,0)`、`carrier c1=(0,1,0)`、`+z_i=(1,SiteId,0)`、
`-z_i=(1,SiteId,1)`；tuple 按无符号整数字典序比较。不得使用浮点字节、枚举声明
顺序、遍历地址或容器迭代顺序 tie-break。去重后 node 仍保留其最小 key；promotion
另记录提供 physical capability 的 candidate key。

若 z_i 或 -z_i 与 c0/c1 重合，它是一次 CarrierPhysical promotion，不创建
第二个同方向节点。若两个 physical candidate 相互重合，也只保留一个 candidate。
最终任意两个 Sample center 的夹角都必须大于 EPS_ANGLE；否则
SampleDegenerate。graph 永不通过 theta=0 除法制造 edge。

这是一项 versioned gauge。GeometryUnit label 随整体旋转保持，所有距离和内禀
签名旋转不变，故无 tie 的旋转副本严格等变；EPS tie、rank-1 或连续对称 fixture
先按同一 label gauge 对齐，或比较重建 observable，不比较任意 gauge 中心。

### 6.3 transport responsibility phi

初始 SampleField 包含两个 Carrier；总节点预算 K 包含它们。carrier transport
radius 为 CARRIER_RADIUS_RAD。普通 Physical 的 transport radius 为
min(pi, PHYSICAL_COVERAGE_FACTOR * ell)。CarrierPhysical 使用二者的较大值。

对每个 backend 的 domain point x，令 r_j 为该 Sample 的 transport radius：

    g_j(x) = psi(theta(x, s_j) / r_j)
    phi_j(x) = g_j(x) / sum_h g_h(x)

分母必须严格正。两个 carrier 的 separation 为 pi - CARRIER_OFFSET_RAD，且
各自 radius 大于 pi/2，因此完整方向空间有至少一个正 carrier profile。
所有归一化以 max-scaled 或 log-sum-exp 实现；所有 profile 真为零或非 finite
时失败。

phi 是运输所有权，不是 physical density。Carrier 的 broad phi 不得产生质量。

physics_reference_s2：

    V_j^T = integral phi_j(u) dω

semantic_production_384：

    phi_ja = phi_j(z_a)
    V_j^T = sum_a omega_a * phi_ja

两种 backend 都严格验证 sum_j V_j^T = 1。

### 6.4 physical responsibility chi、mass 与重建

只对 physical Sample 集合 P 定义；责任核先限制到 physical density 支撑：

    h_j_plus(x) = indicator[rho(x) > 0]
                  * psi(theta(x, s_j) / (2 * ell))
    d_P(x) = sum_l_in_P h_l_plus(x)
    chi_j(x) = h_j_plus(x) / d_P(x),  if d_P(x) > 0
    chi_j(x) = 0,                    if d_P(x) = 0

在 physical density 为零的 S2 方向，chi 全部为零。为使 residual-greedy 的
partial prefix 有唯一评分语义，`rho(x)>0` 但 `d_P(x)=0` 时也使用上面的零责任：
c、m、V^P 与 p_hat 均按已覆盖域积分，p_hat 在 hole 为零，禁止按已覆盖质量
重归一化。此约定只供 partial-prefix scoring；最终 SampleField 必须 U_P=0。
S2 的 density support 内若分母为零，报告 uncovered physical mass，并使最终
structural_pass 失败。S2 不能仅靠
cubature 节点“没有看见 hole”来宣称覆盖；每个 DensitySite i 必须有一个已选
Physical center j 满足：

    theta(z_i, s_j) <= ell - EPS_ANGLE

每个 density kernel 的支撑半径是 ell，而 h_plus 的支撑半径是 2*ell；该证书
保证 density cap 的正值区域内至少有一个正 responsibility。支撑边界 rho=0，
不要求正。不满足证书时 prefix 结构失败。semantic 的每个控制点均有正 p_a；
semantic 明确定义 `h_ja=psi(theta(z_a,s_j)/(2*ell))`；因每个控制点 `p_a>0`，
不再乘连续 support indicator。最终 prefix 的每个 a 必须满足
`sum_l_in_P h_la>0`；partial scoring 对失败控制点使用同样的 chi=0/p_hat=0 约定。
禁止以 floor、fallback nearest node、后处理 renormalization、Sinkhorn、IPFP
或 NNLS 修补。

统一审计量为相对未覆盖质量：

    U_P = (1/M) * integral rho(x)
          * indicator[sum_l_in_P h_l_plus(x) = 0] dω

semantic 对应：

    U_P_op = (1/M) * sum_a r_a
             * indicator[sum_l_in_P h_la = 0]

只有上述几何证书（S2）或所有控制点分母（semantic）通过时，U_P 才结构性
等于零。未通过时仍按零责任约定计算 partial E_TV 并进入 greedy 比较；它不能
成为 first-passing/final witness。下述 coupling marginals、总质量与 p_hat 积分
恒等式只对 U_P=0 的最终 prefix 作 hard gate；partial prefix 可缺失的列/总质量
必须被 U_P 完整计量。

physics_reference_s2：

    c_ji = mu_i * integral chi_j(u) * exp(log_k(u, z_i)) dω
    m_j = sum_i c_ji
    V_j^P = integral_(rho(u)>0) chi_j(u) dω

semantic_production_384：

    chi_ja = chi_j(z_a)
    V_j^P = sum_a omega_a * chi_ja
    c_ji = mu_i * sum_a chi_ja * K_ai
    m_j = sum_i c_ji = sum_a chi_ja * r_a

两种 backend 都要求：

    c_ji >= 0
    sum_j_in_P c_ji = mu_i          for every i
    sum_i c_ji = m_j                for every physical j
    sum_j_in_P m_j = M
    V_j^P > 0 and m_j > 0           for every physical j

这些是解析/有限模型的目标恒等式。semantic finite sums 与 S2 adaptive cubature
都用 SiteId/SampleId 升序 Kahan 累加，并逐项验收 residual：

    abs(column_sum_i - mu_i)
      <= EPS_ABS + EPS_REL * max(1, abs(mu_i))
    abs(row_sum_j - m_j)
      <= EPS_ABS + EPS_REL * max(1, abs(m_j))
    abs(sum_j m_j - M)
      <= EPS_ABS + EPS_REL * max(1, abs(M))

S2 coarse/fine 两次都须独立通过且在 artifact 写出最大 residual；semantic 不做
post-hoc correction。任何 backend 都不得为了通过边缘而重归一化 c 或 m。

Carrier-only 的 c row、m、rho 严格为零。physical Sample density 为：

    rho_j = m_j / V_j^P

重建 density：

physics_reference_s2：

    p_hat(x) = (1/M) * sum_j_in_P rho_j * chi_j(x)
    E_TV = 0.5 * integral abs(p(x) - p_hat(x)) dω

semantic_production_384：

    p_hat_a = (1/M) * sum_j_in_P rho_j * chi_ja
    E_TV_op = 0.5 * sum_a omega_a * abs(p_a - p_hat_a)

semantic 的 strict marginals 保证 sum_a omega_a * p_hat_a = 1。对于 S2，
adaptive cubature 还必须证明 coupling marginals、p_hat integral 与 E_TV 的
coarse/fine 收敛。最终 SampleField 固化细化值。

### 6.5 deterministic residual-greedy

初始状态只有两个 Carrier，Physical 集为空。每个 ell 的 `N_req^greedy` 探索
不读取当前 FieldVersion.K：它在完整、固定候选池上运行到 first pass 或候选耗尽；
新增节点最多到候选池节点总数，CarrierPhysical promotion 不增加节点数。
得到 N_req 后，G_K 才以 `N_req<=K` 判断该 FieldVersion 是否可承载 witness；最终
返回的 SampleField 绝不超过 K。这样同一个 ell 的 N_req 不随查询预算改变。
FieldVersion 已统一拒绝 `K<MIN_SAMPLE_BUDGET`。

greedy 必须能从不完整 prefix 生长；因此区分 `candidate_score_valid` 与最终
`structural_pass`。临时 operation 先计算 coverage/physical projection，只要数值
finite、方向不退化、预算未超、并且至少有一个 positive physical volume/mass，
就可计算 `(U_P,E_TV,key)` 并参与选择。N_req 探索不把当前 K 当作预算门。
`U_P>0`、graph 尚未连通或部分 coupling/PT
尚触及 cut locus 是这个 partial prefix 的可改进状态，不把该 operation 丢弃；
partial prefix 的 E_TV 仍按第6.4节在完整 domain 计算：未覆盖区域固定
`p_hat=0`，不能按已覆盖质量重新归一化或只在子域评分；未覆盖质量同时由 tuple
第一项优先惩罚。只有 true denominator 非 finite、已选方向退化、正质量责任
本身无法定义才是 score-invalid 候选，等价
`(infinity,infinity,key)`。

每次选入 score-valid operation 后，prefix 必须按以下固定顺序计算最终
`structural_pass`：

1. direction 去重后任意两点距 `>EPS_ANGLE`；节点数只用于记录 N_req，最终
   FieldVersion witness 另要求 `N_req<=K`；
2. 全 domain 的 transport phi coverage 证书成立，所有 `V^T_j>0`；
3. 第 6.4 节 S2 support certificate 或 semantic control denominator 全部通过，
   `U_P=0`；
4. physical `V^P_j,m_j` 均正，c/m/rho finite、nonnegative，coupling marginals
   在本节后述容差内；
5. 对任意 `c_ji>0`，`theta(s_j,z_i)<pi-CUT_LOCUS_MARGIN_RAD`；
6. 第 7.1 节 graph 的全部 nonzero-overlap pair 通过 cut-locus gate，且正 edge
   graph connected；
7. A finite、nonnegative；对任意 `A_ol>0`，owner 到 absorber 的 PT 也通过
   cut-locus gate；
8. backend refinement 与 hash payload 均可构造，最后才计算 E_TV。

任一步未通过时记录确定性 failure code，但只表示“尚未 first-pass”；greedy 继续
从这个已扩展 prefix 选择下一项，直到通过、候选耗尽或没有 score-valid operation。
某个候选自身 physical volume/mass 为零时该候选 score-invalid，不永久加入。
其余候选按以下 tuple 字典序最小：

    (uncovered_physical_mass, E_TV_or_E_TV_op, candidate_key)

选定后永久加入或升级。若有 CarrierPhysical promotion，它增加 physical
capability 但不增加节点数。仅当 `structural_pass=true` 且同时达到：

    uncovered_physical_mass = 0
    E_TV_or_E_TV_op <= EPS_REPRESENTATION

时立即停止；不能为了用满 K 继续加点。若所有 candidate operation 用尽仍不通过，
N_req^greedy = infinity。若通过，N_req^greedy 是这个 first passing prefix
的实际总 Sample 节点数（包含两个 carrier，capability 不另计数）。该过程不读取
source、Response、transport、EventContent 或成员数。

## 7. Sample graph、吸收与运输

### 7.1 finite graph

对任意 Sample j,k 定义 transport overlap：

physics_reference_s2：

    O_jk = integral phi_j(u) * phi_k(u) dω

semantic_production_384：

    O_jk = sum_a omega_a * phi_ja * phi_ka

数值上 `O_jk <= EPS_ABS` 规范为 zero-overlap，不建 edge；`O_jk > EPS_ABS`
视为 nonzero overlap，且仅当
`EPS_ANGLE < theta(s_j,s_k) < pi-CUT_LOCUS_MARGIN_RAD` 时建立无向 edge：

    H_jk = O_jk / theta(s_j, s_k)

具有 nonzero overlap 的 cut-locus pair 使当前分辨率失败。S2 coarse/fine 对每个
pair 的 zero/nonzero 分类必须一致，否则 `GraphUnresolved`；semantic 只按同一
finite Kahan sum 分类。由于 Sample direction
已去重，零 edge 永不创建。正 overlap graph 必须连通。

两个 backend 的 finite Sample graph 与 FVM 都使用同一个 versioned
`endpoint_radial_finite_v1` rule：

    r_j = theta(q, s_j)
    B_jk(q) = H_jk * (r_k - r_j) / theta(s_j, s_k)
    B_kj(q) = -B_jk(q)

在 semantic384 中它只定义 finite operational transport，不被解释成 S^383 的
连续 PDE；在 physics_reference_s2 中它仍是有限 D=3 graph，必须用第5.2节逐 ray
continuous reference 检查 coarse/fine 与跨 K 趋势。S2 reference 不把该 B 公式
提升成连续真理，semantic 也不能借 S2 趋势声称高维 continuum convergence。

### 7.2 absorption matrix

transport owner o 到 physical absorber l 的 rate coefficient：

physics_reference_s2：

    A_ol = SIGMA * rho_l * integral phi_o(u) * chi_l(u) dω

semantic_production_384：

    A_ol = SIGMA * rho_l * sum_a omega_a * phi_oa * chi_la

A 只用于传播载荷扣减与 Sample Response 生成。Carrier 可作为 owner，所以它有
A row；Carrier-only 不能作为 absorber，所以没有 A column。c 是 physical
response 回到 DensitySite 的 coupling；A 与 c 不得混名、复用或互相代替。

uniform reference rho=1 的完整角路径 optical thickness 是 SIGMA*pi=1。
SIGMA 属于 FieldVersion，不随当前 M 或 K 重估。

### 7.3 transport state

每个 Sample 保存一次单源临时状态：

    U_j >= 0
    W_j in T_(s_j) S^(D-1)
    norm(W_j) <= U_j

初值：

    U_j(0) = I0 * phi_j(q)

q 与 s_j 非源点/非对点时：

    W_j(0) = U_j(0) * v_q(s_j)

源点或对点方向未定义时 W_j=0，不得任选方向。I0 必须 finite 且 0 < I0 <= 1；
vacuum diagnostic 允许只运行 raw。

### 7.4 一个 FVM step

令 I_j = U_j / V_j^T。对 unordered edge 只算一次：

    F_jk = max(B_jk, 0) * I_j + min(B_jk, 0) * I_k
    F_kj = -F_jk

令：

    d_j = sum_k max(B_jk, 0) + sum_l A_jl

步长规则：

1. d_j = 0 的 row 不进入 CFL minimum；
2. 若至少一行 d_j > 0：

       h = min(hmax, tau_remain, CFL * min_(d_j>0)(V_j^T / d_j))

3. 若全部 d_j = 0：

       h = min(hmax, tau_remain)

先用旧 U/W 同步做 edge step：

    U_j_star = U_j - h * sum_k F_jk

从 j 流向 k 的正标量 amount = h * max(F_jk, 0)。若 amount = 0，转移方向矩
精确为零，不做 amount/U_j。若 amount > 0 而 U_j <= 0，报告
InconsistentFlux。所有 edge amount 必须先从同一个旧 `(U^n,W^n)` 计算，再同步
更新；不得原地逐 edge 消耗。对每条有向正流 `j->k` 定义：

    R_jk = (amount_jk / U_j^n) * W_j^n
    T_jk = PT_(s_j -> s_k)(R_jk)

其中 amount=0 时 R=T=0；非零 amount 才允许除以 U。同步 provisional moment：

    W_j_star = W_j^n - sum_k R_jk + sum_h T_hj

同一 row 的 `sum_k amount_jk <= U_j^n` 由 CFL 验证，因此多 outgoing edge 只按
同一旧 W 的比例移除一次总份额。PT 采用第 2 节 cut-locus gate；不能为 edge
任选切向量。

随后做 local exact absorption。令：

    Arow_j = sum_l A_jl
    lambda_j = Arow_j / V_j^T

若 Arow_j = 0，则 `b_j=0, U_j_new=U_j_star, W_j_new=W_j_star`，不进入任何
absorber 分配。否则：

    b_j = U_j_star * (-expm1(-h * lambda_j))
    U_j_new = U_j_star - b_j
    W_j_new = (1 - b_j / U_j_star) * W_j_star

若 U_j_star = 0，则 `b_j=0, U_j_new=0, W_j_new=0`；若 b_j > 0 且
U_j_star <= 0，报告 InconsistentAbsorption。该零分支不做除法。只有 b_j > 0
时才计算
b_j/U_j_star 和 A_jl/Arow_j。对每个 physical l：

    delta_a_l = b_j * A_jl / Arow_j
    delta_M_l = (A_jl / Arow_j)
              * PT_(s_j -> s_l)((b_j / U_j_star) * W_j_star)

所有 absorber 分配都读取同一个 absorption 前 `W_j_star`；同一份 b 同时从 U
扣减、由上式按同比分从 W 扣减并进入 Response。任何非零 A 对应的
PT 必须距 cut locus 大于 margin。每步验证 U>=0、norm(W)<=U 与：

    sum_j U_j + sum_l a_raw_l = I0

迭代至 tau=pi；超过 FVM_MAX_STEPS 为 TransportUnresolved。

### 7.5 raw ledger、闭合与细化

最终：

    I_res = sum_j U_j
    I0 = I_res + sum_l a_raw_l
    norm(M_raw_l) <= a_raw_l
    A_raw = sum_l a_raw_l

A_raw=0 时只有 raw result；不得 close、scatter、Exp 或 append。A_raw>0 时，
必须先通过本节细化，再在该 source 内比例闭合：

    a_l = I0 * a_raw_l / A_raw
    M_l = I0 * M_raw_l / A_raw

残余不持久、不跨 source、不进入下一相。

在固定同一 SampleField 上分别运行：

    coarse: CFL=0.45,  hmax=pi/32
    fine:   CFL=0.225, hmax=pi/64

两者均须 raw ledger 通过且 A_raw>0。比较：

    D_a = sum_l abs(a_raw_l_coarse/A_coarse - a_raw_l_fine/A_fine)
    D_M = sum_l norm(M_raw_l_coarse/A_coarse - M_raw_l_fine/A_fine)

要求 D_a + D_M <= EPS_TRANSPORT_DISTRIBUTION。否则 ResponseUnresolved，不闭合。
最终结果使用 fine run。这一门以 A_raw>0 加相对分布/方向矩收敛定义可闭合性，
不引入任意 raw absorption 下限。

## 8. Response、scatter 与两相事件步

### 8.1 natural self moment

自然 source h 的单源结果中，只把 absorber h 的方向矩分量 M_(h<-h) 置零；
其 scalar 保留，其他 source 到 h 的 moment 也保留。该操作在多 source 聚合前
完成。

### 8.2 Physical Sample 到 DensitySite

对 phase sign epsilon=-1（natural）或 +1（external）：

    delta_a_(i<-j) = (c_ji / m_j) * a_j
    delta_P_(i<-j) = (c_ji / m_j) * PT_(s_j -> z_i)(epsilon * M_j)

只遍历 m_j>0 的 physical Sample。Carrier-only 不进入。按 SampleId/SiteId 顺序
Kahan 累加；非零 responsibility 的 PT 必须不在 cut locus。每个 site 得到
a_i、P_i，且 norm(P_i)<=a_i。

同一 site cluster 内所有 GeometryUnit、同一 GeometryUnit 内所有 Event 获得
同一个 P_i；不按 Content 数再次除法。

### 8.3 Exp 与 readout

    z_i_new = Exp_(z_i)(P_i)
            = cos(norm(P_i))*z_i + sin(norm(P_i))*P_i/norm(P_i)

P_i=0 时 identity。不存在 eta、damping 或 clip。输入 phase budget<=1，故位移
处于 injectivity radius 内；更新后重新归一化只允许修正 roundoff。

外界相 fine closed scalar a_j 先经同一 c/m 做 ephemeral scalar scatter 得 a_i。
按 a_i 降序、SiteId 升序，返回该 GeometryUnit 的所有 Content（EventId 升序）。
readout 发生在外界 moment scatter/Exp 与新 Event append 之前；它没有第二套
cosine 或原始 density 查询。

### 8.4 Active 两相事件步

    1. clone current state as an atomic working copy
    2. derive G_K -> DensitySite -> density -> SampleField^C
    3. for every Physical Sample h with m_h>0:
         I0_h = m_h / sum_h m_h
         raw -> refinement -> close independently
         zero only M_(h<-h)
    4. aggregate natural Response -> scatter(-M) -> Exp old coordinates
    5. append Natural CoordinateLog entries for every old EventId
    6. fully rebuild G_K -> DensitySite -> density -> SampleField^A
    7. external q, I0=1: raw -> refinement -> close
    8. readout from closed external scalar
    9. scatter(+M) -> Exp intermediate coordinates
   10. append External CoordinateLog entries for every old EventId
   11. append new Event(content,q) at the original normalized input q
   12. atomically commit state and increment ids

两相不能共用 SampleField，也不能把不同切空间中的 P 先相加后一次 Exp。新 Event
不参加本步任何 source、Response、自力或日志；下一事件步才进入旧场。

每相总预算各为 1。自然相每个 source 的 raw/closed ledger 独立保存；任何 source
A_raw=0 或 refinement 失败使整个事件步原子失败，不能把其预算重分给其他 source。

CoordinateLog 顺序固定为 phase 先 Natural 后 External，phase 内 EventId 升序；
即使 before_coordinate=after_coordinate 也写一条。load 验证每个成功 step 对每个
旧 Event 恰有两条且前后坐标相接。

## 9. Building、Active、hash 与 persistence

### 9.1 Building 与 readiness

Building 只允许 deposit_building：在输入坐标原位追加 Content/Event，不运行自然
或外界相，不写 CoordinateLog。它用于建立足够大的初始场，不是小场特殊动力学。

readiness 是只读、确定性重建。为使“代表性外界 source”不是实现者自行挑选的
集合，对每个已完整构造的 SampleField 固定构造 `ReadinessProbe` Vec。每个 probe
有 wire key `(kind:u8,a:u64,b:u64)`；按该 tuple 的无符号字典序运行和报告，绝不按
浮点坐标、hash map 迭代或去重后的 q 排序。固定 kind 为：

    physical_center=0, carrier_center=1, carrier_antipode=2,
    physical_graph_midpoint=3

其中 Physical 指 `role != carrier && m_j>0`，包括 CarrierPhysical。probe 集合恰为：

1. 对每个 Physical Sample j（SampleId 升序），
   `(physical_center,j,0)`，`q=s_j`；
2. 对两个 retained primary candidate key 分别为 carrier c0 `(0,0,0)` 与 carrier c1
   `(0,1,0)` 的 Sample，按 carrier rank `r=0,1`，
   `(carrier_center,r,j)`，`q=s_j`；即使该节点被提升为 CarrierPhysical，也仍保留
   该 probe；
3. 对同一两个 carrier 节点，`(carrier_antipode,r,j)`，`q=-s_j`；
4. 对每条 `H_jk>0`、两个端点均为 Physical 的无向 graph edge，令 `j<k`，
   `(physical_graph_midpoint,j,k)`，
   `q=normalize(s_j+s_k)`，归一化使用第 2.1 节的 index-ascending Kahan 规则。

第 7.1 节的 cut-locus gate 保证第 4 项的和非零且唯一短弧存在。不同 probe 即使
产生逐 bit 相同的 q 也不得合并；例如 CarrierPhysical 的 center 同时属于第 1、2
项是有意的。空的 physical-edge 子集合合法，但两个 carrier probe 和其 antipode
必须各恰有两个，否则当前 SampleField 结构失败。

对每个 probe，固定以 `I0=1` 在冻结 SampleField 上执行第 7.3--7.5 节的 coarse/
fine raw transport、细化比较和 closed Response；不做 Exp、append、日志或任何
持久状态修改。必须记录 coarse/fine raw ledger、`D_a`、`D_M`、fine `A_raw`，再用
第 8.2 节同一 coupling 做 scalar/moment scatter（不执行 Exp）。令
`r_i=a_i/sum_i a_i`，分母按 SiteId 升序 Kahan 累加；记录每个 SiteId 的
`a_i,r_i,norm(P_i)`、`N_eff`、`max_i r_i` 和 `max_i norm(P_i)`。每一 probe 必须：

1. 两个 raw ledger 与 `D_a+D_M<=EPS_TRANSPORT_DISTRIBUTION` 均通过；
2. fine `A_raw>0`；
3. scalar/moment scatter 有定义，且 `N_eff>=ACTIVE_MIN_N_EFF`、
   `max_i r_i<=ACTIVE_MAX_SITE_SHARE`、
   `max_i norm(P_i)<=ACTIVE_MAX_ANGLE_RAD`。

已能构造 SampleField 时，任一 probe 失败不得抑制其后 stable key 的 probe；报告
必须包含整个确定 probe 集合的成功/失败 metrics。尚不能构造 SampleField 时，报告
明确记录该结构失败，probe Vec 为空，不能以空 probe 宣称通过。

单个 readiness stage 还必须同时满足：

1. GeometryUnit >= `ACTIVE_MIN_GEOMETRY_UNITS`；
2. G_K 与最终 `E_TV` 或 `E_TV_op` 通过；
3. coverage、transport/physical 两类 volume、coupling marginals、graph、A、S2
   cubature 或 semantic finite arithmetic，以及全部 hash payload 都有效；
4. 上述完整 probe 集合全部通过。

在原始 stage 通过后，readiness 必须作一次不落盘的完整 natural preflight。对原
SampleField 中每个 Physical Sample h（SampleId 升序）独立以
`I0_h=m_h/sum_l m_l` 运行第 8.4 节第 3--5 步所需的 raw/refinement/close；所有
source 都在同一个冻结场上运行并记录，不能因一个 source 失败而跳过其后的 source。
每个 source 必须 `A_raw>0` 且通过细化。仅在所有 source 成功后，按第 8.1 节先将
每个 `M_(h<-h)` 置零，再聚合、scatter `-M`、Exp 到一个内存 working copy；不得
写 CoordinateLog、消费 ID 或 append Event。由该 working copy 完整重建出的第二个
stage 必须再次满足上述 1--4 和完整 probe 集合。natural preflight 失败时不得生成
第二 stage。

这些阈值是版本化 initialization policy，不进入动力学公式。readiness report
包含两个 stage 的 state/physics/sample hashes、完整 probe metrics、natural preflight
source metrics、全部 gates 与 report_sha256；其精确 payload 在第 9.2 节定义。

activate(state, report_hash) 必须同步重跑 readiness；只有 passed=true、state hash
未变且重算 report hash 等于参数时才切 Active。report 不持久充当权威缓存。

Active 接受任意 finite q，但有限 readiness probe 不冒充全域证明。实际 q 若
A_raw=0、graph/transport unresolved 或中间重建不再满足结构门，本次 mutation
原子失败，原状态不变。它不自动降 K、改 scale、加 density floor 或切回 Building。

### 9.2 hash 边界

所有权威 SHA-256 都使用同一个 `fm_v2_canonical_le_v1` codec，绝不 hash JSON、
serde/bincode 输出或平台内存布局。摘要输入首先写入以下互不相同的 ASCII domain
tag（含末尾 NUL）：

    state_sha256          "fm-v2/state/v2\0"
    physics_sha256        "fm-v2/physics/v2\0"
    mapping_sha256        "fm-v2/mapping/v2\0"
    sample_field_sha256   "fm-v2/sample-field/v2\0"
    report_sha256         "fm-v2/readiness/v1\0"
    coordinate_sha256     "fm-v2/fixture-coordinates/v1\0"

primitive 编码固定如下：u8 原字节；u32/u64 为 little-endian；bool 为 u8
0/1；String 为 u64 byte length 后接 UTF-8；Option 为 u8 0，或 u8 1 后接 payload；
Vec 为 u64 element count 后逐项编码。f64 必须 finite，`-0.0` 先规范成 `+0.0`，
再写 IEEE-754 `to_bits()` 的 little-endian u64；NaN/Infinity 不可 hash而是结构失败。
struct 严格按本文字段出现顺序；map/set 禁止直接编码，必须先转成本文规定 key
升序 Vec。摘要以 lowercase 64-char hex 暴露。

固定 enum tag：BackendKind `physics_reference=0, semantic_production=1`；SpaceKind
`physics_reference_s2=0, semantic_production_384=1`；Lifecycle `building=0,
active=1`；CoordinatePhase `natural=0, external=1`；SampleRole `carrier=0,
physical=1, carrier_physical=2`。新增 enum 必须先 bump codec/algorithm id。

以下是 canonical payload 的唯一字段表；`Vec<Vec<f64>>` 外层按指定行序，内层按
coordinate index。所有 SHA 字段自身以 lowercase hex String 编码，不能 decode
成 raw bytes。未列字段不进入对应 hash：

    EmbeddingIdentityWire =
      model:String, model_digest:String, dataset_sha256:String,
      source_dimension:u32, target_dimension:u32, projection_family:String,
      projection_archive_sha256:String, projection_content_sha256:String,
      ollama_version:String
    FieldVersionWire =
      state_schema_version:u32, algorithm_id:String, scalar:String, dimension:u32,
      sample_budget:u32, backend_kind:u8, space_kind:u8, measure:String,
      kernel_id:String, projection_arithmetic:String, sample_projection_id:String,
      graph_id:String, gauge_id:String, sigma:f64, dataset_sha256:Option<String>,
      embedding_identity:Option<EmbeddingIdentityWire>
    GeometryUnitWire = geometry_gauge_rank:u64, representative_coordinate:Vec<f64>
    DensitySiteWire =
      site_id:u64, geometry_gauge_rank:u64, direction:Vec<f64>, mu:f64
    ResolutionAttemptWire =
      level:u32, ell:f64, site_count:u64, n_req:Option<u64>,
      representation_error:Option<f64>, failure_code:Option<String>,
      witness_candidate_keys:Vec<(u8,u64,u8)>
    DerivedResolutionWire =
      chosen_level:Option<u32>, chosen_ell:Option<f64>,
      attempts:Vec<ResolutionAttemptWire>
    SampleWire =
      sample_id:u64, candidate_key:(u8,u64,u8), role:u8, direction:Vec<f64>,
      transport_radius:f64, transport_volume:f64,
      physical_volume:Option<f64>, mass:Option<f64>, density:Option<f64>
    EdgeWire =
      low_sample_id:u64, high_sample_id:u64, overlap:f64, conductance:f64

`geometry_gauge_rank` 是 GeometryUnit 按 `(intrinsic_signature,geometry_label)` 排序后
从0递增的 rank；它编码被允许的对称 gauge，但不泄漏原始 EventId。GeometryUnitWire
按 gauge rank；
DensitySiteWire 按 SiteId；SampleWire 按 SampleId；EdgeWire 端点先取 min/max 后按
`(low,high)`。dense matrix 的唯一 wire 为
`rows:u64,cols:u64,values_row_major:Vec<f64>`。semantic 的 phi/chi/K/c/A 与 S2
固化 fine phi/chi/c/A 均使用它；不适用的 backend matrix 用 Option=None，不得用
空矩阵冒充。S2 adaptive cubature 另编码按 `(face_id,base4_path)` 排序的：

    CubatureLeafWire =
      face_id:u8, base4_path:Vec<u8>, bounds:Vec<f64>[恰4项], area:f64,
      fine_values:Vec<f64>, error_bounds:Vec<f64>

graph/A、coupling 与 cubature component 顺序是正文公式的 owner-major、
absorber/site-minor 顺序；source-dependent B/Response 不属于 SampleField hash。

`sample_field_sha256` payload 唯一为：

    FieldVersionWire, DerivedResolutionWire, Vec<DensitySiteWire>,
    Vec<SampleWire>, phi:Option<MatrixF64Wire>, chi:Option<MatrixF64Wire>,
    operational_K:Option<MatrixF64Wire>, coupling:MatrixF64Wire,
    graph_edges:Vec<EdgeWire>, absorption:MatrixF64Wire,
    cubature_leaves:Option<Vec<CubatureLeafWire>>

`physics_sha256` payload 唯一为 `FieldVersionWire,Vec<GeometryUnitWire>,
sample_field_sha256:String`。`mapping_sha256` payload 唯一为：

    mapping_version:u32=2,
    entries:Vec<(event_id:u64,geometry_gauge_rank:u64,site_id:Option<u64>)>

entries 按 EventId；重复/missing EventId 均失败。`state_sha256` payload 唯一为
`FieldVersionWire,lifecycle:u8,next_event_id:u64,next_step_id:u64,events,logs`；
Event wire 为 `id:u64,content:String,coordinate:Vec<f64>`，log wire 为
`step_id:u64,event_id:u64,phase:u8,before:Vec<f64>,after:Vec<f64>`。

Readiness probe key 即第 9.1 节 `(kind:u8,a:u64,b:u64)`。固定 metric wire 为：

    ProbeMetricWire =
      key:(u8,u64,u64), q:Vec<f64>, passed:bool,
      coarse_raw_error:f64, fine_raw_error:f64,
      scalar_distribution_distance:f64, moment_distribution_distance:f64,
      fine_absorbed:f64, n_eff:f64, max_site_share:f64,
      max_predicted_angle:f64, failure_code:Option<String>
    GateWire =
      id:String, passed:bool, observed:Option<f64>, threshold:Option<f64>,
      failure_code:Option<String>
    ReadinessReportWire =
      state_sha256:String, stage0_physics_sha256:Option<String>,
      stage0_sample_field_sha256:Option<String>,
      stage1_physics_sha256:Option<String>,
      stage1_sample_field_sha256:Option<String>,
      probes:Vec<ProbeMetricWire>, gates:Vec<GateWire>, passed:bool

probes 按 key，gates 按 UTF-8 id bytes。natural preflight source metrics 也编码为
ProbeMetricWire，kind 固定4、`a=SampleId,b=0`，并合并进 probes。
`report_sha256` 只 hash ReadinessReportWire，不递归包含自身。

snapshot state_sha256 使用上述 State payload；Event 按 EventId、log 按
`(step_id,phase_tag,event_id)` 升序。

physics_sha256 绝不含 Content、EventId、GeometryUnit member、member count、
CoordinateLog 或任何 mapping；重复 Content 或同坐标成员变动不能改变它。

mapping_sha256 使用上述 Mapping payload，独立覆盖 EventId -> GeometryUnit/
Optional<SiteId> 的成员映射。
Building 状态总能由当前坐标计算 GeometryUnit；若空场或 G_K/Resolution 尚不可行，
SiteId 为 None。只有已成功派生最终分辨率时为 Some(SiteId)。因此每次 Building
deposit 都能原子保存/加载，不要求提前 Active。sample_field_sha256 使用上述
SampleField payload；它同样不含 Content/member。

report_sha256 使用上述 ReadinessReportWire。fixture coordinate_sha256 payload 为：

    schema_version:u32, fixture_id:String, backend_kind enum, space_kind enum,
    dimension:u32, K:Option<u32>, origin_tag:u8, coordinate_count:u64,
    coordinate vectors in manifest order

origin tag 固定 `state=0,direct_sample_field=1,analytic_reference=2`。state origin 的
coordinate 是 events 顺序；direct 是 SampleId 顺序；analytic 是 source q 后接
manifest omega 顺序。Content、expected 值与 graph 不进入 coordinate hash。manifest
不得以 null 冒充已验证 hash；Phase 0 生成后必须把 lowercase digest 写回 committed
fixture，后续 run 只验证、不改写。

### 9.3 snapshot 与 adapter

snapshot.json 是唯一权威文件，顶层 state_schema_version 必为 2，包含 version、
lifecycle、ids、events、完整 CoordinateLog、state_sha256 与 mapping_sha256。保存：

1. 在同目录创建唯一 tmp；
2. 写完整 UTF-8 JSON，flush、sync_all；
3. rename 到 snapshot.json；
4. sync parent directory。

load 先解析 state schema，再 validate FieldVersion、所有坐标、ID/log cardinality、
mapping 与 state hash。绝不读取/迁移 v1 sled。未知 schema 或 identity mismatch
失败。

正式 projection asset 路径：

    assets/v2/bge-m3-spherical-pca-384.npz

安装/加载时验证 archive SHA
`2ba52a192937c9d4f8b6ab34980cf733539c9417af1b4db7c5c9ae636d80dd68`、上述
content hash、components float32 shape[1024,768]、
eigenvalues float64 shape[1024] 与 metadata；只读前384列。输入先以 f64
index-ascending Kahan norm 归一化；float32 component cast f64 后按 output index
和 input index 升序 Kahan dot；输出再同法 L2 normalize。禁止 FMA-dependent
批量路径改变 operation order。

Ollama 请求错误、shape 错误、零向量或 identity 错误必须返回 Result error；
不得沿用 v1 adapter 的 zero-vector fallback。

## 10. CLI

package 名 field-mem-v2-cli，binary 名 field-mem-v2。固定命令：

    field-mem-v2 init <library> --reference-s2 --sample-budget K
    field-mem-v2 init <library> --semantic-384 --sample-budget K
    field-mem-v2 deposit <library> --content TEXT (--coordinate CSV | --embed-text TEXT --ollama-base-url URL)
    field-mem-v2 readiness <library>
    field-mem-v2 activate <library> --report-sha256 HASH
    field-mem-v2 add <library> --content TEXT (--coordinate CSV | --embed-text TEXT --ollama-base-url URL)
    field-mem-v2 query <library> (--coordinate CSV | --embed-text TEXT --ollama-base-url URL)
    field-mem-v2 inspect <library>

semantic384 的 deposit/add/query 只允许 embed-text；coordinate CSV 仅
physics_reference_s2。所有 mutation 成功后自动原子保存；失败不写盘。CLI 从
snapshot 取得 embedded FieldVersion，并用当前 binary 的 frozen version tuple
构造 expected_version 后调用 load。

第一版不提供 K migration。要改变 K，用户显式创建新 Building library 并重新
deposit 当前 Content/Coordinate；不能宣称恢复旧分辨率已丢失的信息。

## 11. fixtures、artifact 与串行验证

### 11.1 committed fixture manifest

固定文件 experiments/v2_validation/fixtures-v1.json，顶层 `schema_version` 必为
1；fixture、run manifest、status 与 result 的 JSON 顶层都统一使用该键；
`ARTIFACT_SCHEMA_VERSION` 是代码常量名，不产生第二个 JSON 键。每个 fixture
至少包含：

    fixture_id, backend_kind, space_kind, embedding_identity,
    dimension, K, origin, lifecycle_allowed,
    materialized coordinates/content/source,
    coordinate_sha256 或 phase0_generated_after_build 状态

reference fixture 必须显式为 `physics_reference/physics_reference_s2/null`；semantic
fixture 必须携带完整冻结 embedding identity。runner 不得从 dimension 或 origin
猜 backend。

需要进入 Phase 1 及以后数值验收的 direct SampleField fixture，必须在对应 phase
启动前物化 role、direction、phi/chi 值、V、rho/m/c/A/graph；runner 不得自行补
默认。Phase 0 的 `vacuum_transport` 只须显式物化 Carrier role/direction、transport
volume 与所有 physical row 为空，其 phi/graph 可由 Phase 0 builder 生成并写入
artifact，不能把未生成值当黄金结果。direct fixture 永不 readiness 或持久化。

committed manifest 在 contract 接受时至少物化 Phase 0 family：empty、
duplicate_coordinate、orthogonal_basis、symmetric_pair、antipodal、rotated_copy、
vacuum_transport。single_site、uniform_shell、local_cluster、large_balanced、
large_skewed 与 dynamic_balance_v1 必须在其首次对应 phase 启动前追加并物化；
未物化的 family 不能运行或宣称通过。生成后坐标全部物化进 JSON；测试不重新用
另一语言计算黄金比或旋转常量。

### 11.2 artifact 与 runner

runner 只串行启动，并固定：

    FM_V2_SEED=20260810
    RUST_TEST_THREADS=1
    RAYON_NUM_THREADS=1

每 run artifact 的顶层 `schema_version` 必为 1，并包含：

    run_manifest.json
    status.json
    result.json
    summary.md
    command.txt
    environment.json
    stdout.log
    stderr.log
    sidecars/

`run_manifest.json`、`status.json` 与 `result.json` 的 required key、JSON type、
enum、nullability 和 `additionalProperties=false` 边界唯一以 committed
`experiments/v2_validation/artifact-schema-v1.json` 为准；契约接受时该文件的
SHA-256 写入验证 README，runner 与 validator 必须先验证同一 schema bytes。
禁止近义 key：一律使用 `embedding_identity`、`n_density_sites`、
`sample_budget_k`、`transport_sample_count`、`physical_sample_count` 与
`carrier_sample_count`。status 启动时为 running，结束原子替换为
pass|fail|timeout|resource_limit|invalid_artifact|skipped。

自然逐 source ledger 唯一路径为
`sidecars/natural/source-<source_id>.ledger.jsonl.zst`；descriptor path 相对
`sidecars/`，所以 JSON 中写 `natural/source-<source_id>.ledger.jsonl.zst`。
JSONL 第一行是 exact-key header：

    record_type="header", source_id:String, initial:f64, absorbed:f64,
    residual:f64, closure_factor:Option<f64>, rows:u64

后续恰 rows 行是 exact-key ResponseRow：

    record_type="response", sample_id:u64, a_raw:f64, M_raw:Vec<f64>,
    a_closed:Option<f64>, M_closed:Option<Vec<f64>>

JSONL 只作可读容器：UTF-8、每行一个 object、拒绝重复/未知 key、数值必须 finite；
空白、key 顺序与 zstd frame 不影响 logical identity。descriptor 的
`logical_rows_sha256` 使用 domain `"fm-v2/response-row-stream/v1\0"` 和第9.2节
binary codec，依次编码 header 的 source_id/initial/absorbed/residual/
closure_factor/rows，再按 SampleId 严格升序编码每行 sample_id/a_raw/M_raw/
a_closed/M_closed；`file_sha256` 另 hash 实际压缩 bytes。validator 必须解压、严格
解析、重编码并同时验证两个 SHA 与 row count。P0--P2 mode=`full`，P3--P4
mode=`streamed_full`；二者 logical schema 相同，禁止只保存 digest 的模式。

--latest 选择请求 phase/tier 最新 started_at_utc/run_id；最新 run 不完整或失败
就直接失败，不回退旧 pass。Phase 3 无 tier 时只读取最新完整 batch，不能拼接
不同 batch。

### 11.3 受控串行命令

每条命令单独完成后才可启动下一条；不得使用 workspace-wide 并行测试。

    env RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 cargo test -p field-mem-core -- --test-threads=1
    env RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 cargo test -p dse-cli --no-run
    env RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 cargo test -p dse-server --no-run

    env RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 cargo test -p field-mem-v2 -- --test-threads=1
    env RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 cargo test -p field-mem-v2-cli -- --test-threads=1

    env RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 python3 experiments/v2_validation/run.py --phase 0
    python3 experiments/v2_validation/validate_artifact.py --latest --phase 0
    env RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 python3 experiments/v2_validation/run.py --phase 1
    python3 experiments/v2_validation/validate_artifact.py --latest --phase 1
    env RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 python3 experiments/v2_validation/run.py --phase 2
    python3 experiments/v2_validation/validate_artifact.py --latest --phase 2

    env RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 python3 experiments/v2_validation/run.py --phase 3 --tier S64
    python3 experiments/v2_validation/validate_artifact.py --latest --phase 3 --tier S64
    env RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 python3 experiments/v2_validation/run.py --phase 3 --tier S128 --continue-batch BATCH
    python3 experiments/v2_validation/validate_artifact.py --latest --phase 3 --tier S128
    env RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 python3 experiments/v2_validation/run.py --phase 3 --tier S256 --continue-batch BATCH
    python3 experiments/v2_validation/validate_artifact.py --latest --phase 3 --tier S256
    env RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 python3 experiments/v2_validation/run.py --phase 3 --tier S512 --continue-batch BATCH
    python3 experiments/v2_validation/validate_artifact.py --latest --phase 3 --tier S512

Phase 4 命令由 validator 在 unlock 成功时打印；人工拼写不能绕过门。Phase 3
每个 tier run 后立即离线 validate，失败即停。Phase 4 S1024 只在同 git SHA、
contract SHA、fixture SHA 的完整 Phase 3 batch 通过且 S512 wall/RSS 均低于
上限70%时启动。

## 12. Phase 验收

### Phase 0：结构

- normalize/distance/Exp/PT/cut-locus；
- duplicate Content 不改变 physics hash；
- G_K 无独立 ell、N_req 是 greedy first pass、K 单调；
- S2 logZ 归一化 256/512/1024；
- semantic column-stochastic K、p/r/c/m 单一质量真理；
- full transport coverage、Carrier-only exact zero physical mass；
- chi support、V/m/c strict marginals、E_TV；
- semantic finite control arithmetic 不含 S^383 cubature；
- S2 support-aware cell/ray reference 不漏 compact support；
- rotation/gauge observable；
- snapshot round-trip 与 v1 isolation。

### Phase 1：单机制

- vacuum raw residual=I0、不 close；
- uniform rho=1 的 S2 continuous ray residual=e^-1；
- graph pairwise flux antisymmetry、nonnegative U、edge-carried W bound；
- d_j=0、amount=0/U=0、b=0/U_star=0 分支；
- raw/closed ledger、A_raw=0 与 tiny-positive-A convergence；
- natural self moment only；
- coupling scatter、readout、large balanced/skewed N_eff；
- S2 coarse/fine graph 对 continuous ray reference 收敛。

### Phase 2：事件步

- natural -> log -> rebuild -> external readout/scatter -> log -> append；
- 两相各预算1、逐自然 source 子账本；
- phase-local Exp，不跨重建合并 P；
- newborn 不自作用；
- read-only query 不改变 snapshot；
- 中间失败完全回滚。

### Phase 3/4：大场

按验证计划的 K/tier 顺序检查每 tier 内部 ledger、E_TV、participation、位移界、
旋转副本与 duplicate invariance；跨 tier 只报告已被理论支持的 scale/
representation monotonicity。dynamic_balance_v1 串行64步，记录 Nsite、N_eff、
p50/p95/max displacement 与 near/far contribution；阈值以 committed fixture
manifest 为真理源，不查看结果后修改。

semantic384 另验证 frozen embedding/PCA/dataset identity、finite control
invariants 和持久化；不得把它报告成 S^383 continuum convergence。

## 13. 明确边界

1. Physical density 与 coupling 共享 Wendland；transport phi 与 physical chi
   是同一 SampleField 内不同责任，不是两套场。
2. Carrier 保证承运覆盖但不造质量。全覆盖不保证一定吸收；A_raw=0 只表示本次
   source 在当前有限场不可闭合。
3. K=64 的 384D graph 是有限 operational model，不是高维局部连续网格。
4. 连续对称 density 没有唯一有限中心；版本化 gauge 后比较 observable。
5. E_TV 控制 density 表示，不证明 transport；transport 另过 coarse/fine 与 S2
   continuous reference 门。
6. 每个 K 是不同分辨率模型族；内部严格守恒不等于有限 K 之间 Response 相同。
7. 本版优先完成完整、可测、可持久化的核心框架，不引入鉴权、多租户、远程服务、
   自动 migration 或 v1 compatibility layer。

## 14. accepted 证据

本文改为 Status: accepted 时已经同时满足：

- committed `experiments/v2_validation/dual_responsibility_probe.py` 数值原型通过，
  证明 Carrier 的 c/m/rho/Response 精确为零而 transport volume 为正；
- 独立数学 reviewer 对 G_K、semantic K/r/c/m 单一质量真理、A/c 分离、FVM 分支、
  closed ledger、backend 边界无 HIGH；
- fixtures-v1.json 的 artifact schema 与至少一个 Phase 0 小 fixture 已物化；
- docs/README、foundations、validation plan 引用本 ADR/contract 且没有 bipole、
  MIN_RAW 或 S383 continuum 的旧权威冲突；
- git diff --check 通过，控制字符扫描为空，v1 baseline 单进程通过；
- contract accepted 变更单独提交，之后才创建 v2 crate。
