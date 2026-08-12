# field-memory v2 生产维度保真实验报告

Status: active
Owner: field-memory
Last updated: 2026-08-12
Scope: 冻结 BGE-M3 语义方向空间的降维保真度；不验证 v2 动力学
Related code: `experiments/v2_dimension_fidelity/`
Related docs: [实验协议](../specs/2026-08-12-v2-dimension-fidelity-experiment.md)、[v2 权威、分辨率与维度 ADR](../decisions/2026-08-12-v2-authority-resolution-and-dimension.md)、[v2 理论基石](../design/field-memory-v2-foundations.md)

## 结论

在冻结的 BGE-M3、6,611 条 memory、2,161 个 query、uncentered spherical
PCA 和全部预注册门禁下，**384 是候选网格中第一个通过的语义环境维度**。384、512、
768 三档通过；3、8、16、32、64、128、256 未通过。离线 validator 实际校验
dataset、cache、projection bundle、split、pair hash 和完整 metric schema，并从
保存的 metric 独立重算全部 hard gate 与最小通过维度后确认：

```text
selection                 dimension_fidelity_passed
minimum passing dimension 384
passing dimensions        384, 512, 768
```

这支持把 `semantic_candidate_d384` 与本次 projection bundle 作为后续
implementation contract 的**候选生产表示**。它不批准 v2 动力学实现，也不证明
transport、守恒、局部性、坍缩、遗忘或长期平衡正确；`physics_reference_s2` 仍是
独立的严格动力学 reference backend。

候选网格从 256 直接跳到 384，因此本实验没有排除 257–383 中存在更低通过
维度。若架构只允许预注册的版本化档位，384 是当前合法选择；若目标改为寻找
逐整数的最低维度，必须另开 refinement protocol，不能把本次结论改写成“所有
整数维度中的绝对最低值”。

## 1. 冻结输入与隔离边界

| 项目 | 正式值 |
|---|---|
| dataset | 6,611 memories / 2,161 queries |
| dataset SHA-256 | `ed5a9836f277ea8242fce0bd477363bb21e81d606226dc607b92554d7b7d9901` |
| embedding | Ollama `bge-m3:latest`, 1,024D |
| model digest | `7907646426070047a77226ac3e684fbbe8410524f7b4a74d02837e43f2146bab` |
| Ollama | `0.21.2` |
| PCA Build | 5,321 memories / 73 groups |
| heldout | 1,290 memories / 22 groups |
| heldout neighbor probes | 512 |
| global geometry sample | heldout-only 100,000 unique unordered pairs |
| candidate dimensions | 3, 8, 16, 32, 64, 128, 256, 384, 512, 768 |
| random diagnostics | dimensions 3/32/128/512 × 3 frozen seeds |

PCA 只读取 Build memories，不读取 heldout memories 或 query embedding。全局
cosine/angle hard gate 的 100,000 个 pair 只从 heldout memories 中无放回抽取，
因此不会用拟合集上的几何保真冒充样本外保真。query 和 heldout-memory 邻域都
以 `native_embedding_reference_1024` 的 exact cosine top-k 为基准；相同 cosine
按冻结 memory index 升序打破平局。

## 2. 正式结果

### 2.1 几何与邻域门禁

| d | 二阶矩保留 | cosine error p95 | Spearman | local angle p95 | query R@10 | query R@50 | heldout R@10 | pass |
|---:|---:|---:|---:|---:|---:|---:|---:|:---:|
| 3 | 0.5607 | 0.5143 | 0.7566 | 0.8745 | 0.0122 | 0.0428 | 0.0178 | no |
| 8 | 0.6313 | 0.4553 | 0.8068 | 0.7118 | 0.1265 | 0.2242 | 0.1291 | no |
| 16 | 0.6949 | 0.3892 | 0.8404 | 0.5896 | 0.2800 | 0.3829 | 0.2744 | no |
| 32 | 0.7643 | 0.3015 | 0.8967 | 0.4700 | 0.4433 | 0.5446 | 0.4357 | no |
| 64 | 0.8347 | 0.2150 | 0.9370 | 0.3367 | 0.6110 | 0.6860 | 0.5863 | no |
| 128 | 0.9049 | 0.1316 | 0.9705 | 0.2051 | 0.7619 | 0.8100 | 0.7297 | no |
| 256 | 0.9635 | 0.05891 | 0.9925 | 0.08986 | 0.8975 | 0.9188 | 0.8785 | no |
| **384** | **0.9860** | **0.02153** | **0.9990** | **0.03700** | **0.9599** | **0.9697** | **0.9523** | **yes** |
| 512 | 0.9942 | 0.00851 | 0.9999 | 0.01530 | 0.9836 | 0.9873 | 0.9824 | yes |
| 768 | 0.9988 | 0.00205 | 1.0000 | 0.00356 | 0.9962 | 0.9965 | 0.9957 | yes |

单位：angle 为 rad；三个 recall 是相对 native exact-neighbor 集合的平均重叠率。

256 是最接近通过但仍不合法的候选。它通过 Spearman、local angle 与三组静态
readout ratio，却同时失败于四个 hard gate：

- cosine error p95 `0.05891 > 0.05`；
- query neighbor recall@10 `0.89750 < 0.90`；
- query neighbor recall@50 `0.91876 < 0.95`；
- heldout-memory neighbor recall@10 `0.87852 < 0.90`。

因此 384 是冻结候选网格与选择规则得出的第一个通过点，不是看完结果后挑选的
折中值；它也不声称排除了 257–383 的所有整数维度。

### 2.2 384 维的门禁余量

| hard gate | 384 结果 | 阈值 | 绝对余量 |
|---|---:|---:|---:|
| invalid projected rows | 0 | `<= 0` | 0 |
| cosine error p95 | 0.02153 | `<= 0.05` | 0.02847 |
| cosine Spearman | 0.99903 | `>= 0.98` | 0.01903 |
| local angle error p95 | 0.03700 | `<= 0.10` | 0.06300 |
| query neighbor recall@10 | 0.95993 | `>= 0.90` | 0.05993 |
| query neighbor recall@50 | 0.96971 | `>= 0.95` | 0.01971 |
| heldout-memory recall@10 | 0.95234 | `>= 0.90` | 0.05234 |
| ground-truth Recall@5 ratio, overall | 0.99947 | `>= 0.95` | 0.04947 |
| ground-truth Recall@5 ratio, LoCoMo | 0.99935 | `>= 0.95` | 0.04935 |
| ground-truth Recall@5 ratio, project | 1.00000 | `>= 0.95` | 0.05000 |

最紧的两个非离散余量是 cosine Spearman `+0.01903` 与 query recall@50
`+0.01971`。384 不是勉强擦过单一指标，但也不应被描述为对任意 corpus 或模型
变化都稳健；embedding digest、corpus 或 projection bundle 改变后必须重跑。

### 2.3 静态 ground-truth 读出

native 1,024D 的原始 Recall@5 为：overall `0.43789`、LoCoMo `0.38709`、
project `0.98370`。384D/native 的比值分别为 `0.99947`、`0.99935`、`1.0`。

这里必须区分“保留了 native 行为”和“native 行为本身足够好”。比例门禁证明
384D 几乎没有进一步损坏这套 frozen dense readout；它不能把 native LoCoMo
Recall@5 `0.38709` 变成已经合格的生产记忆质量，也不能替代后续 v2 Response
readout 测试。冻结数据中有 1 个重复 `relevant_id` 条目，runner 与 validator
都按集合语义折叠并在 artifact 中显式记录。

## 3. 低维究竟造成什么

低维不会自动破坏正确实现中的非负性、总预算守恒或两相时序；这些是算法结构
不变量。但它会改变场本身所依赖的角几何：原本可区分的方向被合并，邻域次序
改变，随后 kernel density、coupling、Sample 分布和 Response 都会成为另一个场。

本实验把这种影响量化得很直接：

- `semantic_candidate_d3` 的 query neighbor recall@10 只有 `0.0122`，heldout
  memory recall@10 只有 `0.0178`。它不能因为几何上也位于 \(S^2\) 就冒充
  `physics_reference_s2` 的生产语义空间。
- 64D 已保留 83.47% 二阶矩，但 query recall@10 仍只有 `0.6110`；只看 explained
  energy 会严重高估语义场保真。
- 128D 的静态 readout ratio 已接近 native，但样本外 cosine 与邻域仍明显失真；
  只看任务标签同样会低估几何损失。
- 256D 已很接近，却仍同时漏掉全局角误差和三个邻域门禁。对 v2 来说，这些不是
  装饰性指标：density 与 Sample 都要由相对方向结构导出。

所以低维的危险不是“代码跑不起来”，而是代码严格执行了一个被压扁后的场。

## 4. 随机投影对照

Gaussian random projection 只作诊断、不参与选择。三个 seed 在 512D 的
cosine error p95 为 `0.06246`–`0.06596`，Spearman 为 `0.96437`–`0.97357`；
相比之下，spherical PCA 在 384D 已达到 `0.02153` 与 `0.99903`。

这说明当前通过结论依赖 corpus-adaptive spherical PCA，而不是“任意降到 384
维都可以”。正式生产表示必须携带本次 projection bundle 或由同一冻结协议重建；
不能只保存整数 `384` 后换一张随机矩阵。

## 5. 执行与资源控制

先运行 128 memories / 32 queries / 2,000 pairs 的 preflight。Python 实验在
48.49 秒完成，峰值 RSS 178,536,448 bytes；首个 shell wrapper 随后因 zsh 的
只读变量名 `status` 返回 1，但 Python 已输出 `complete`，artifact 完整，离线
validator 重算通过。没有为掩盖 wrapper 错误而重跑模型；formal wrapper 改用
`run_exit`。

formal 只运行一个 Python 实验进程，BLAS 四个线程变量均固定为 `1`：

| 资源 | 结果 | 门禁 |
|---|---:|---:|
| runner wall time | 574.88 s | 3,600 s |
| `/usr/bin/time` real | 576.13 s | 3,600 s |
| peak RSS | 1,798,717,440 bytes | 4,294,967,296 bytes |
| cache + bundle + result | 36,858,815 bytes | 1,073,741,824 bytes |
| swap | 0 | 必须无资源失败 |

embedding cache 每 10 个 batch 原子落盘；运行中没有自动重试、降规模、换模型、
改阈值或并发启动第二个实验进程。

## 6. Artifact 与可追溯证据

| artifact | SHA-256 |
|---|---|
| formal JSON | `7a0cb1d33f820f0d1f35c37ca79528505d28a069f99fddb49b29e02a8c995ea5` |
| strict embedding cache archive | `cb635c89ad7c0d48f5f9b8b6f30ecc4466cf43efc6044780a6388fe8e08dd3d3` |
| strict embedding cache content | `1bc7c787ee3eaa476fe0c2de1e1fc47bb10bcaee5a5fee4ed8da6327c9db6d95` |
| PCA bundle archive | `2ba52a192937c9d4f8b6ab34980cf733539c9417af1b4db7c5c9ae636d80dd68` |
| PCA bundle content | `17f2932d84ce59107e1f080ddd9279e30390fc6822f41a89f75b72a5cd8cbc11` |
| runner | `ca1932965152cfe4f6574614248629555088e2228fd6359860f5421d8acc1232` |
| protocol | `70d60acb808569c250dbb0975ce3eb4f8cd6141359a712ebfe78742ff0db1ad5` |
| offline validator | `091e13efdafc798dbbed459ad832defaafec64c86608f3ff12c99274771f3b7f` |

生成物位于 `experiments/v2_dimension_fidelity/artifacts/`，按仓库规则忽略，不
把约 37 MB 的可重建二进制 cache/bundle 提交进 Git。正式 JSON 保存了 dataset、
model、Ollama、git、runner、protocol、cache、projection、split、probe、pair、
指标、阈值和资源记录；validator 实际读取并校验这些文件、重建 split/probe/pair
hash，并从保存的完整 metric 重算 gate 与 selection，而不只检查 64 位 hex
字符串的格式。它不重新执行 embedding、PCA 或 10 轮全量 metric 计算；完整
数值复算仍等价于按冻结 runner 重新运行 formal 实验。

交付前的独立只读复核另从 cache、dataset 和 bundle 重新计算了 256D/384D 的
全部 hard-gate metric，结果与 formal JSON 逐值一致；并验证 384 个 PCA 分量对
Build-only \(X^TX\) 的相对特征残差为 `2.67e-8`、正交误差为 `8.41e-9`。这项
针对临界两档的独立复算弥补了通用 validator 默认不重跑全部 PCA/metric 的边界，
但仍不等同于第二次完整 10 档 formal run。

运行时 HEAD 为 `eae8a895…`，worktree 含本轮文档/runner 与用户已有的其他未提交
修改，因此 commit 本身不是实验代码的充分标识；上表的 runner/protocol/content
hash 才是本次计算的精确事实来源。实验没有读取无关工作树文件，也没有从它们
重建冻结 dataset。

## 7. 后续决策

建议后续 accepted implementation contract 明确区分：

1. `physics_reference_s2`：只用于解析几何、守恒和动力学 reference 测试；
2. `semantic_candidate_d384`：当前冻结 BGE-M3/corpus 下第一个通过的候选；
3. `native_embedding_reference_1024`：实验基准和模型/corpus 变化时的 provisional
   no-compression fallback。

在用户明确接受 384 作为当前生产候选前，本报告不把它升级成不可变架构常量。
即使接受，下一步仍是先冻结 v2 implementation contract，再按 Phase 0–4 验证
Sample、同核 coupling、守恒运输、Response、两相事件步与大场长期行为；不能
用本次 representation pass 跳过理论和动力学门禁。
