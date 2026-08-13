# field-memory v2 Phase 0 checkpoint handoff

Status: active
Owner: field-memory
Last updated: 2026-08-13
Scope: v2 独立 crate 的 Phase 0 检查点、已实现边界、验证证据与后续接续顺序
Related code: `crates/core/field-mem-v2`、`crates/ext-v2-cli`；S² Sample cubature 检查点在 `sample.rs`
Related docs: [v2 实现契约](field-memory-v2-implementation-contract.md)、[v2 分阶段验证计划](2026-08-10-field-memory-v2-validation-plan.md)、[v2 理论基石](../design/field-memory-v2-foundations.md)、[Sample 投影与 backend 边界 ADR](../decisions/2026-08-13-v2-sample-projection-and-backend-boundary.md)

## 1. 当前结论

这是一个可编译、测试和继续扩展的 **Phase 0 检查点**，不是 Phase 0 完成声明，
更不是可用的完整 field-memory v2。v2 已从冻结的 v1 路径中独立出来；当前代码没有
引用 `field-mem-core` 的 Anchor、impact、RelaxationCycle 或 v1 persistence。

实现继续以 accepted contract 为唯一算法权威：

- algorithm id：`fm-v2c-wendland-residual-edge-fvm-v2`
- contract SHA-256：`e9bc3e6960139139a1cb10158b997ffb2dec23069400ca631dd971ebf84fd2b8`
- fixture manifest SHA-256：`f68631c148ba0ee8144f999fd842269f5da068ec8868273d60e2daad1569dfdc`
- artifact schema SHA-256：`4481c5ad3234b594df3c578c03af5931beed3e158367b7ccb63376fef76c457a`

上述任一权威字节发生变化，都必须先重新核对 contract/ADR 和 algorithm identity，
不能由后续实现者自行“合理化”或沿用旧 artifact。

## 2. 已实现

| 模块 | 当前事实 |
|---|---|
| `numeric` | `f64`、Kahan 累加、稳定指数/对数辅助；NaN 与正无穷不会被静默当成零质量 |
| `geometry` | `Direction`、角距、切向投影、Exp/Log、平行运输、cut-locus 与 source/antipode 显式零矩分支 |
| `version` | 只接受 S²/D=3 reference 与冻结 identity 的 semantic/D=384 两组 FieldVersion |
| `event` | Event/Step/Geometry ID、Building/Active schema、Building deposit 与 CoordinateLog 基础不变量 |
| `persist` | canonical state/mapping hash、独立 golden digest、Building mapping、原子 JSON snapshot、版本与 hash 校验 |
| `kernel` | Wendland C2 profile、S² normalizer refinement、semantic column weight；非法数值结构化失败 |
| `kernel/S² cubature` | contract 固定的 cube-face GL2/GL4 自适应求积、support cap 剪枝、activity bound、确定性 leaf 与上限失败 |
| `resolution` | geometry dedup、内禀 signature、O(M²) FPS DensitySite、21 层尺度审计；semantic backend 可记录真实 first-passing witness |
| `sample` | 共享 residual-greedy；semantic384 的 lazy column kernel 与 discrete \(\phi/\chi/c/A\)；S² 用同一 greedy，连续 \(\phi/\chi/c/A/E_{TV}\) 走 adaptive cubature 并固化 leaf；Carrier/Physical/promotion、finite graph 两边共用 |
| `ext-v2-cli` | 独立 package 边界可构建；命令尚未启用 |

持久化目前完整覆盖 Building 状态。Active 状态必须由后续 resolution/step 集成提供
完整 `EventId -> GeometryUnit -> optional SiteId` mapping；当前 API 会显式拒绝缺失映射，
不会伪造或静默保存。

## 3. 尚未实现，因此不可宣称通过 Phase 0

- S² residual-greedy 已接通 cubature，但单点/稀疏 site 上连续 \(E_{TV}\) 仍可大于
  `EPS_REPRESENTATION`，greedy 返回真实 `projection_candidate_exhausted`，不是
  `sample_projection_unavailable_s2`。尚无 committed S² first-passing fixture。
- semantic384 已能得到真实 residual-greedy witness，但尚未完成权威
  `sample_field_sha256/physics_sha256` 编码与 committed Phase 0 fixture replay。
- `transport.rs`、`response.rs`、`step.rs` 仍是空边界；尚无 edge-carried moment、
  raw-to-closed Response 或两相动力学。SampleField 已冻结 graph edge 与 A，FVM
  不得回读 Event/DensitySite。
- 尚无自然相 → 重建 → 外界相 → readout → append Event 的原子事务。
- CLI 仅证明 crate/package 边界，尚无 init/deposit/readiness/activate/apply/query/save/load。
- validation runner、artifact replay 与 Phase 0 integration fixture 尚未接入 Rust 实现。

因此，单元测试全绿只证明当前 primitive 与 Sample 投影边界自洽，不证明 contract 的 Phase 0 gate。

## 4. 当前验证证据

基础检查点 `0aa9b45` 的 33 个 primitive tests 已通过。本次继续实现后必须以当前
工作树的串行门禁为准；交付前记录最终计数与日志路径，不能沿用旧计数冒充新证据：

```text
RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 \
  cargo test -p field-mem-v2 --lib -- --test-threads=1
  result: 46 passed; 0 failed
  full log: .codex/logs/phase0-s2-sample-checkpoint/cargo-test.log

cargo clippy -p field-mem-v2 --all-targets -- -D warnings
  result: passed
  full log: .codex/logs/phase0-s2-sample-checkpoint/clippy.log

cargo check -p field-mem-v2-cli
  result: passed
  full log: .codex/logs/phase0-s2-sample-checkpoint/cli-check.log
```

不得用全 workspace 的绿色结果替代上述 v2 定向门禁，也不得并行运行多个阶段。

## 5. 推荐续接顺序

1. 在冻结 `SampleField` 上实现 graph FVM、scalar `U`、edge-carried moment `W`、
   raw ledger、细化 gate 与逐源 closure；transport API 不得读取 Event/DensitySite。
   graph overlap 与 A 已由 Sample 投影固化，不要重新选边公式。
2. 实现 Response scatter/Exp 和两相 `apply_event` 原子事务；失败必须回滚 working copy。
3. 接通 Active mapping 持久化、readiness report/hash、CLI 和 artifact runner/replay。
4. 完成 Phase 0 integration 后，才按验证计划逐阶段解锁 Phase 1–4。

## 6. 已知后续审计项

- S² activity bound 必须用 Wendland `d_min` 证明零区；对 \(\phi/\chi/O\) 一律取 1
  会使 `missing_bound=area` 把零区细到百万 leaf。后续改 bound 只能更紧，不能再放宽到
  常值 1。
- 冻结候选池只有 \(\{z_i,-z_i,c_0,c_1\}\)。单点 site 在 \(\ell=\pi\) 上 structural
  prefix 可通过（\(U_P=0\)、coupling 列和 \(\approx\mu\)），但连续
  \(E_{TV}\approx 0.41>0.03125\)；再加入对点会在粗尺度碰到 coupling cut-locus。
  不要为了让单点 fixture 变绿而扩大候选池或改 \(E_{TV}\) 门。需要 first-passing
  S² witness 时另选更密的 site 集，或先提 ADR。
- semantic projector 当前会在一次 prefix 评估中重复生成 kernel column；保持 lazy
  column 真理，后续只做同结果缓存，不得物化 M×M 作为运行时状态。
- CoordinateLog 的首个 Natural `before` 必须由 `apply_event` working copy 原子生成；只看
  最终 snapshot 无法独立重建第一次事件前坐标，后续 integration test 应保存并核对该
  provenance，而不是拿最终 Event coordinate 反推。
- 当前工作区还有与本检查点无关的 v1/frontend/`CLAUDE.md` 修改；本次提交没有纳入、
  stash 或改写它们。后续提交仍须显式限定 v2 文件范围。

## 7. 续接验收语句

下一位实现者开始时应将状态描述为：

```text
v2 contract 已 accepted；Phase 0 primitive、semantic Sample projection 与 S² cubature
Sample projection checkpoint 可用；S² 尚无 first-passing E_TV witness。
transport FVM、Response、两相 step、CLI 与 artifact runner 未完成。
继续实现，不重新选择算法，也不把当前单元测试当作 Phase 0 pass。
```
