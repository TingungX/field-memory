# field-memory v2 可行性规模验证报告

Status: active
Owner: field-memory
Last updated: 2026-08-10
Scope: v2 设计理念、观察骨架、长期记忆读取/写入对比
Related code: `experiments/v2_feasibility/`
Related docs: `docs/superpowers/specs/2026-07-04-field-memory-v2-design.md`, `AGENTS.md`

## 技术结论

在 6,611 条 memory、2,161 个 query、64 个写入案例的规模上，当前 v2 观察骨架还不能作为传统 RAG/记忆系统的替代品：

- 传统 hybrid RAG 的 recall@5 为 `0.5624`，当前 v2 为 `0.0172`；v2 的 precision@5、MRR、NDCG@5 也都只有传统 baseline 的约 `1/27`–`1/33`。
- 主要不是 30 条样本太少，而是 v2 的 sample projection：默认 `sample_budget=96` 每个事件只覆盖 `1.45%` 的 6,611 个 anchor。把 budget 提到 4,096（覆盖 `61.96%`）后 recall@5 升到 `0.2593`，仍低于 baseline，说明链式传播、readout 和反馈顺序也需要重新定义。
- 写入语义已经被真实观测：64/64 个 v2 写入进入 event log，说明 `write -> perturb` 管道确实执行；但 0/64 返回原始事件文本，32 个 replay 写入在默认投影下也没有读回预期 anchor。当前设计把“写入发生”与“可读取”拆开了，却还没有补上二者之间的可验证契约。
- v2 的长时间 warm sequential 运行使未触及 anchor 大量坍缩到 density 下限：最终 mean density `0.0653`、min `0.05`。这不是通过绕过方向演化修饰出来的结果，而是当前 collapse/coverage 近似在大场上的直接行为。
- `test-model` 真实接入已验证到 transport 层，但当前代理不稳定：传统 context 的 Responses SSE 调用得到 HTTP 200、12.7 秒后开始返回模型输出；受 160-token 上限和 reasoning 占用影响，最终可见答案被截断。v2 context 的单题 SSE 调用在获得 HTTP response 后 90 秒未产生一个 payload，被按进程控制阈值主动停止。非流式路径仍会在 usage DB 锁处报 500。LLM 结果从未参与 retrieval 评分。

因此当前判断是：v2 的“场作为唯一 ontology、perturb 同时读写、样本作为交互投影、事件时间推进”值得继续做独立实验；但 v2 还没有通过替换生产记忆层所需的读取质量、写入可回读性和规模稳定性门槛。

## 1. v2 理念的完整复述

以下是设计草案中的设计意图，不等同于本报告中为了可运行而选定的公式。草案明确写着“design idea, not implementation spec”。

### 1.1 ontology 与投影

1. **场是唯一 ontology。** 记忆不属于用户、模型或某个来源；事件只携带方向和密度，来源身份不进入物理方程。
2. **anchor 是存储投影。** anchor 是持久化的场状态快照，不是传统意义上的标签条目或用户偏好通讯录。
3. **sample point 是交互投影。** sample 是 perturb 时从场中生成的暂态交互点，承担局部响应和链式传播，不应被当成第二套持久化记忆。
4. 只有 anchors 持久化；samples 重新生成。events、traces、seeds 的持久化策略在草案中仍未完全决定。

### 1.2 读写等价与时间

- `perturb(query)` 同时完成读取和写入：查询先在场中产生响应，再把反馈写回场状态。
- 时间从 wall-clock relaxation window 改成 **per-event step**：每个事件推进一次场，而不是等待真实时间窗。
- readout 必须发生在 feedback 之前，否则已经变化过的场会污染当前事件的读取结果。

### 1.3 动力学

- direction 是慢变量；density 是局部激活/沉积强度。
- 事件附近 density 增加，远处发生 collapse/decay；anti-collapse 规则应该保证场不会被少数事件吸干。
- 影响不是对所有 anchor 的独立广播，而是通过 sample point 的串行链传播；高密度 relay 会吸收、衰减或改变剩余影响。
- 核心形状仍保留 `cos_sim × sqrt(density)`，但 center/sample 的定义被重新解释。
- paradigm shift、shadow anchor、SeedConcept 不再是显式结构；若需要结构变化，应由传播和场状态涌现。

### 1.4 草案留下的六个实现开放项

1. sample 如何生成；
2. 邻域趋势如何计算；
3. relay 的阈值和 attenuation 如何确定；
4. readout 如何从响应映射回 anchors；
5. event 如何保留、回放和恢复；
6. bincode 等持久化格式如何迁移。

这六项不是本实验偷偷替 v2 做出的“正式设计”。本实验把每个近似值记录在结果 JSON 中，便于被替换和质疑。

## 2. 现成系统参考：Mem0

本实验选择 [Mem0 repository](https://github.com/mem0ai/mem0) 作为传统但可用的开源 memory/RAG 参照，并在 commit `4debc58` 做了源代码审计。Mem0 的公开路径是：

```text
Memory.add()
  ->（可选）LLM 提取/更新/删除记忆
  -> embedder
  -> vector store（Qdrant 为当前默认本地路径）
Memory.search()
  -> 向量检索，可叠加 keyword/BM25 信号
```

本实验没有安装完整 Mem0 服务栈：当前提供的本地 proxy 没有 `/v1/embeddings` 路由。为了不把缺少 embedding provider 混入 v2 结论，baseline 保留了 Mem0 形状（文本 memory + dense vector + lexical/BM25-like signal + recency），embedding 由本机 Ollama `bge-m3` 真实生成。该边界被写入 `dataset_scale.json` 的 provenance 和 `results_scale.json` 的 metadata。

## 3. 数据与规模

### 3.1 数据来源

| 数据集 | 规模 | ground truth | 用途 |
|---|---:|---|---|
| 控制 fixture `dataset.json` | 30 memories / 11 queries / 3 writes | 手工 relevant IDs | 快速回归，不作为 benchmark |
| LoCoMo turns | 5,882 memories | 1,977 个可由 evidence 映射的 QA | 长期、多 session 记忆读取 |
| 项目压力集 | 729 chunks / 85 files | 184 个确定性 chunk queries | 工程文档、Rust、TS/TSX、CSS、TOML 的规模干扰 |
| scale 合计 | 6,611 memories / 2,161 queries | 2,161 个 query 行 | 最终对比 |
| scale writes | 64 | 32 replay + 32 novel | 读取/写入可观察性 |

LoCoMo 采用 [官方 snap-research/locomo 数据发布](https://github.com/snap-research/locomo) 的 pinned source commit `3eb6f2c585f5e1699204e3c3bdf7adc5c28cb376`，下载文件 SHA-256 为 `79fa87e90f04081343b8c8debecb80a9a6842b76a7aa537dc9fdf651ea698ff4`。其数据许可为 CC BY-NC 4.0；原始图片未下载，turn 中的 caption/query 仅作为文字字段保留。

LoCoMo 原始 1,986 个 QA 中，9 个问题的 evidence 在发布文件中无法映射到 turn，脚本将这 9 个问题从可评分集合排除，并把 unresolved evidence 数写进 diagnostics，而不是静默当作 miss。

### 3.2 数据构造规则

- LoCoMo 每个 dialog turn 是一个 memory，保留 speaker、session、date 和 `D<section>:<turn>` evidence ID；初始 `seed_hits=1`，避免用 QA 标签泄漏 density。
- 项目压力集从当前仓库的 Markdown、Rust、TypeScript/TSX、CSS、TOML 分块；排除 `.git`、`target`、`node_modules`、`.next` 和 experiment 目录，query 与 chunk ID 是确定性生成的，不由 LLM 批量生成。
- replay write 是已有 LoCoMo turn 的“Follow-up memory”变体，预期关联原 anchor；novel write 是不在 memory 集合中的 experiment event marker。
- 生成脚本为 `experiments/v2_feasibility/prepare_scale.py`，所以 scale 数据可以从 pinned source 重建，而不是依赖一次不可审计的手工复制。

## 4. 对比方法

### 4.1 传统 baseline

每个 query 对所有已存 memory 计算：

```text
score = 0.75 * dense_cosine
      + 0.20 * lexical_overlap
      + 0.05 * recency
```

其中 dense 和 query 使用同一个 Ollama `bge-m3`（1,024 维），lexical 使用可读的 token/IDF overlap，recency 使用固定 `as_of=2026-08-09`。写入是非突变查询前先追加一条可检索文本，再立即查询。

### 4.2 v2 观察骨架

当前可运行近似是：

1. 一个 memory 对应一个 anchor，初始 density 为 `seed_hits`；
2. sample 是 anchor direction 的 replica，使用按 density 加权的 systematic sampling；
3. sample 总数由 `sample_budget` 限制，默认 96；
4. sample 按 query cosine 降序形成串行链，剩余 influence 按 density-dependent absorption 和 `propagation_threshold=0.06` 衰减；
5. 先保存 positive-response readout，再做 density feedback、bounded direction step 和 untouched-anchor collapse；
6. `write` 和 `read` 都调用同一个 `perturb`；novel write 进入 event log，但当前骨架不创建新 anchor。

`sample_budget`、threshold、吸收上限、collapse 速率等都不是 v2 草案已接受的参数，全部是实验 operationalization。所有值在 `results_scale.json.metadata.config` 中可复核。

### 4.3 评分

只用 deterministic ground truth：precision@5、precision of returned、recall@5、MRR、NDCG@5、returned count、chain length、sample coverage、density delta 和 direction drift。LLM 不参与评分；LLM 只被允许做最多两笔“把 retrieval context 交给模型”的集成 smoke test。

### 4.4 数据与结果工件审计

`validate_artifacts.py` 在当前保存的 scale dataset/result 上通过。它验证了：

- 6,611 memory、2,161 query、64 write 的主键唯一；所有 query evidence 和 replay write anchor 均指向存在的 memory；日期均符合 ISO 日期格式；
- dataset diagnostics 与来源分段实际计数一致（1,977 个 LoCoMo 可评分 query、184 个项目压力 query）；
- traditional 与 v2 两套结果行都一一覆盖 2,161 个 query，返回 ID 没有越出 memory 集，五个 aggregate 指标均由逐行值重新计算验证；
- write summary 覆盖 64 个写入行且汇总 rate 可重算；7 个 sample-budget point 的 coverage 严格随 budget 增长并等于 `budget / 6,611`；
- `results_scale.json.metadata.llm_used_for_scoring` 为 `false`。

这是一项工件一致性检查，不会重新调用 Ollama、4000 端口或 v2 计算，因此不能替代重新运行实验。

## 5. 规模结果

### 5.1 总体读取质量

| pipeline | precision@5 | precision/returned | recall@5 | MRR | NDCG@5 |
|---|---:|---:|---:|---:|---:|
| traditional hybrid RAG | 0.1289 | 0.1289 | 0.5624 | 0.4595 | 0.4712 |
| v2 warm sequential | 0.0047 | 0.0047 | 0.0172 | 0.0170 | 0.0152 |

传统 baseline 每个 query 都返回 5 条；v2 也在 2,161 个 query 中有 2,159 个返回 5 条，因此这次的差异不是只靠“返回更少”造成的，而是 returned IDs 本身没有命中 evidence。v2 的平均 chain length 为 `25.73`（范围 5–78）。

LoCoMo 的 source category 分组也没有显示 v2 只在某一类失效：

| source category | queries | baseline recall@5 | v2 recall@5 |
|---|---:|---:|---:|
| 1 | 281 | 0.2487 | 0.0102 |
| 2 | 320 | 0.7023 | 0.0203 |
| 3 | 89 | 0.2806 | 0.0131 |
| 4 | 841 | 0.6163 | 0.0245 |
| 5 | 446 | 0.4339 | 0.0135 |
| project-context | 184 | 1.0000 | 0.0000 |

这里的 category 数值保持 LoCoMo 原始标签，不在本报告中擅自重新命名；project-context 是本仓库自建压力集。

### 5.2 sample capacity audit

| sample budget | coverage of 6,611 anchors | precision@5 | recall@5 | MRR | NDCG@5 |
|---:|---:|---:|---:|---:|---:|
| 32 | 0.48% | 0.0016 | 0.0054 | 0.0076 | 0.0058 |
| 96 (default) | 1.45% | 0.0047 | 0.0172 | 0.0170 | 0.0152 |
| 192 | 2.90% | 0.0074 | 0.0276 | 0.0259 | 0.0232 |
| 512 | 7.74% | 0.0135 | 0.0567 | 0.0402 | 0.0422 |
| 1,024 | 15.49% | 0.0280 | 0.1186 | 0.0761 | 0.0826 |
| 2,048 | 30.98% | 0.0427 | 0.1838 | 0.1006 | 0.1170 |
| 4,096 | 61.96% | 0.0583 | 0.2593 | 0.1415 | 0.1661 |

趋势是单调改善，但到 61.96% coverage 仍未追上 baseline recall `0.5624`。因此“把 96 调大”是必要的容量问题，却不是完整的架构修复。没有运行 6,611 全覆盖档，避免把 O(query × anchor × sort) 成本继续放大；4,096 已经足够显示曲线方向。

### 5.3 场状态和运行成本

默认 96-sample、2,161 个 warm sequential events 的最终状态：

```text
anchor_count       6,611
event_count        2,161
sample coverage    1.44%（每个 event）
mean density       0.0653
min density        0.05（collapse floor）
max density        7.7178
mean chain length  25.73
```

由于每次 event 都对未触及 anchors 应用 collapse，场的总 density 从初始约 6,611 下降到约 431.49。该结果不是“方向漂移坏了所以改用 origin_direction”，而是当前 collapse 与有限 sample coverage 组合的可观测后果。

本次 full run 的本地耗时（毫秒）如下：

| phase | elapsed |
|---|---:|
| embedding（命中已有 cache） | 8.97 ms |
| 2,161 query comparison | 41,456 ms |
| 64 write comparison | 5,694 ms |
| 7 组 sensitivity | 127,995 ms |

第一次 embedding 使用了 Ollama `bge-m3`，共产生约 32 MB 的去重向量 cache；后续运行全部命中 cache，没有向 4000 端口发 embedding 请求。

### 5.4 写入与读取

| write observation | traditional | v2 |
|---|---:|---:|
| total cases | 64 | 64 |
| exact self-hit after write | 64/64 | n/a（不创建新 anchor） |
| raw event text returned | 不是该 baseline 指标 | 0/64 |
| event log contains write | n/a | 64/64 |
| replay reads expected anchor | n/a | 0/32 |

baseline 的 64/64 self-hit 是“追加原文后立即用原文查找”的管道 sanity check，不应被解读为 64 个独立 memory quality 分数。v2 的 64/64 event-log 命中证明写入入口工作；0/64 raw-text 回读和 0/32 replay-anchor 命中则暴露了当前 v2 持久化/readout 契约的缺口。

## 6. LLM 集成测试：已经接入，但不把它当评测器

LLM 在本实验中的唯一职责是消费已经检索出的 context；所有 retrieval 和 write/read 指标仍来自 deterministic evidence。模型没有被要求比较 pipeline、判断答案正确与否，或给 v2 打分。

| step | pipeline / endpoint | observed result | interpretation |
|---|---|---|---|
| 1 | traditional / Chat Completions | HTTP 500 | 上游调用结束后，proxy 的 `usage_records INSERT` 触发 `sqlite3.OperationalError: database is locked`；不是 model ID 或 prompt 格式错误。 |
| 2 | traditional / Responses SSE | HTTP 200，12.7 s，收到模型输出 | 证明 `test-model` 可真实消费 retrieval context。160 token 输出预算主要被 reasoning 占用，可见答案在第一条 citation 中截断，不能解读为质量结果。 |
| 3 | v2 / Responses SSE | 获得 HTTP response，但 90 s 内 0 SSE payload；受控中断 | 单一 TTY 测试进程被主动 Ctrl-C 停止，没有并发/自动重试，也没有可评分模型输出。 |

第一笔和第二笔的详细无 secret 记录分别是 `llm_smoke_failure.json` 与 `llm_smoke_traditional_responses_stream.json`。第三笔的控制记录是 `llm_smoke_v2_interrupted.json`：它明确标记为无 payload 的中断，而不是伪装成 model failure 或 v2 指标。

`/v1/models`、`/health` 可访问，`test-model` 的 routing 指向 `deepseek-v4-flash`。同时 host Uvicorn 与 Docker container 都是测试前已有的服务；本任务没有重启、修改或停止它们。当前结论是：真实接入已经覆盖到 traditional context 消费和 v2 context 请求的 proxy 流式边界，但代理的 SQLite writer/stream 首字节稳定性不足，不能用它做回答质量评判。这一外部失败不改变第 5 节和第 5.4 节的确定性结论。

## 7. 证据边界与局限

1. **不是完整 Mem0 对比。** 传统 baseline 复现的是公开架构形状；没有运行 Mem0 的 LLM extraction、Qdrant server 或其完整 prompt/evaluator。
2. **v2 仍是观察骨架。** sample、relay、readout、collapse 都是可替换 operationalization，不能把这组数字当成 v2 理论的最终上限。
3. **LoCoMo evidence 是 turn-level ground truth。** turn retrieval 命中不等于最终 QA 答案正确；本报告刻意只评 deterministic retrieval，避免把本地 `test-model` 当裁判。
4. **project-context query 是压力集。** 它验证工程文档规模和 distractor，不是独立公开 benchmark。
5. **warm sequential 有顺序效应。** 所有 query 按数据文件顺序逐个 perturb；这是检验“读也是写”的刻意设置，同时也意味着 state delta 不能与 cold-start 独立 query 分数混为一谈。
6. **事件与 samples 尚未持久化。** 当前 v2 skeleton 的 event log 只在内存实验对象中记录；真正的 crash/restart、replay、bincode migration 仍未验证。
7. **未运行 6,611 全覆盖 budget。** 4,096 coverage 已足以显示 retrieval 仍落后，但完整上界仍是一个待测问题；它不应被报告成已知结果。
8. **外部代理/stream 不稳定。** 非流式路径会在 usage DB 锁处把已完成上游调用转成 500；v2 SSE 请求也在 90 秒内没有首字节。LLM 只作 transport smoke test，报告结论完全基于 deterministic data path。

## 8. 建议的下一阶段

当前不建议直接改写生产 v1。建议把 v2 继续作为隔离实验，按以下顺序收敛：

1. **先冻结可观测契约。** 明确每次 `perturb` 必须输出：sample coverage、chain、readout anchor IDs、event ID、state delta；这些指标先于物理参数调优。
2. **补齐 write/read 契约。** novel event 要么拥有可回读的 event projection，要么明确“v2 只写 field state、不承诺原文 recall”；replay 期望 anchor 的映射必须有可验证规则，不能只留下 event log。
3. **单独确定 sample generation 与容量模型。** 用容量曲线作为验收输入，决定 sample budget 是固定、随 anchor 数增长，还是按局部邻域生成；不要把 `96` 当默认真理。
4. **把 collapse/anti-collapse 做成单独实验。** 先验证总 density、floor 命中率、方向 drift 和事件顺序稳定性，再谈 paradigm/结构涌现；不要绕过 direction 演化。
5. **增加 cold/warm/replay 三种 protocol。** cold read 测纯读取，warm sequential 测读写等价，replay 测持久化恢复；三者分别报告，不混成一个平均数。
6. **达到门槛后再考虑生产试验。** 至少需要在固定公开集上接近传统 baseline 的 recall/precision、novel write 可定义地可回读、重启后 state 可恢复，并且容量/延迟曲线可解释。

## 9. 可复现命令

```bash
# 1. 重建 scale dataset（默认下载 pinned LoCoMo source）
python3 experiments/v2_feasibility/prepare_scale.py \
  --output experiments/v2_feasibility/dataset_scale.json

# 2. 先做短探针
python3 experiments/v2_feasibility/run_experiment.py \
  --dataset experiments/v2_feasibility/dataset_scale.json \
  --limit-memories 128 --limit-queries 8 --limit-writes 4 \
  --skip-sensitivity --cache /tmp/field-memory-v2-scale-probe.npz \
  --output /tmp/field-memory-v2-scale-probe.json

# 3. full deterministic run（不使用 LLM）
python3 experiments/v2_feasibility/run_experiment.py \
  --dataset experiments/v2_feasibility/dataset_scale.json \
  --output experiments/v2_feasibility/results_scale.json \
  --cache /tmp/field-memory-v2-scale-embeddings.npz \
  --embedding-batch-size 32

# 4. 验证保存的数据和结果工件（无网络、无模型调用）
python3 experiments/v2_feasibility/validate_artifacts.py

# 5. 可选：受控的一笔真实 context transport smoke test（不参与评分）
LLM_API_KEY='<endpoint-key>' LLM_MODEL='test-model' \
python3 experiments/v2_feasibility/run_llm_smoke.py \
  --pipeline traditional \
  --output /tmp/traditional-context-smoke.json \
  --max-questions 1 --max-output-tokens 160 --stream
```

## 10. 未决问题

- v2 的 sample 是否应该覆盖全场、局部邻域，还是可重放的 event-specific projection？
- relay absorption 应该依据 density、局部梯度还是邻域趋势？
- 未触及 anchor 的 collapse 是每 event 发生，还是只有在可观测的局部时间尺度中发生？
- event text 是否是 v2 的合法 readout 对象；如果不是，怎样提供不依赖分类标签的原文回读？
- 当场状态达到数万 anchor 时，哪些指标必须保持近似线性，哪些可以接受 O(Q·N)？
- proxy 的 usage DB 锁和 SSE 首字节问题修复后，是否需要在相同的单题/输出预算下重新进行两笔 context smoke test；在那之前不把 LLM 结果写入质量结论。
