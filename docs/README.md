# 文档索引

- [v2 权威、分辨率与维度决策](decisions/2026-08-12-v2-authority-resolution-and-dimension.md)（accepted；v2 为权威、v1 路径冻结且无继承关系、K 定义场版本分辨率、S² 仅作 reference）
- [v2 当前生产语义维度 384 决策](decisions/2026-08-12-v2-semantic-dimension-384.md)（accepted；冻结 BGE-M3、spherical PCA 与 projection identity，不等于任意 384D）
- [v2 Sample 投影、覆盖责任与 backend 边界](decisions/2026-08-13-v2-sample-projection-and-backend-boundary.md)（accepted；Sample 是 density 残差投影；transport \(\phi\) 与 physical \(\chi\) 分责；Carrier 零物理质量；S² 连续 reference 与 semantic384 operational model 不互冒）
- [v2 理论基石：Event、Sample 与尺度密度](design/field-memory-v2-foundations.md)（当前 v2 理论权威草案；包含两相事件步、等预算与大场适用域）
- [v2 生产维度保真实验协议](specs/2026-08-12-v2-dimension-fidelity-experiment.md)（active；真实 BGE-M3 embedding 上的预注册实验，不验证 v2 动力学）
- [v2 生产维度保真实验报告](reports/2026-08-12-v2-dimension-fidelity.md)（formal artifact 已验证；384 已被接受为当前生产语义维度）
- [v2 分阶段验证计划](specs/2026-08-10-field-memory-v2-validation-plan.md)（active；从解析性质到 64–512 大场梯度；1,024 受门禁控制）
- [v2 实现契约](specs/field-memory-v2-implementation-contract.md)（accepted；v2 唯一实现算法、wire schema、命令与阶段门禁，独立 v2 框架实现已解锁）
- [v2 可行性规模验证报告](reports/2026-08-09-v2-feasibility.md)（旧观察骨架的规模证据，不作为当前理论实现或验收基线）
- [v2 设计草案](superpowers/specs/2026-07-04-field-memory-v2-design.md)（superseded 思想演进稿；不可作为实现规范）
- [v1 冻结实现设计](design.md)（superseded；Anchor/impact 路径，仅用于维护现有实现）

## 权威顺序

1. accepted ADR 冻结已经由用户确认的架构决策；若与 draft 文档冲突，以 ADR 为准；
2. v2 理论基石在不违背 accepted ADR 的范围内定义当前本体、因果边界和已确认动力学；
3. accepted implementation contract 冻结唯一实现算法和命令；v2 实现必须逐式镜像它，改变选择需新 ADR 与 algorithm id；
4. 验证计划只规定如何证明实现，没有权力自行选择算法；
5. v1 设计、旧 v2 草案和可行性观察报告都不是 v2 实现权威。

ADR 的 accepted 结论记录“为什么这样决定”，原则上不回写成可变 draft；若后续
方案改变已接受边界，应新增 ADR，并把被替代文档标为 `superseded` 或移入归档。
