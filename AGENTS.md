# AGENTS.md — field-memory

## 场的核心原则

这些原则是 field-memory 的**宪法**——不可动摇，任何修改必须与它们自洽。违反这些原则的"修复"不是修复，是背叛。

### 1. 记忆是场的状态，不是任何主体的属性

场不区分"谁的"记忆。`impact()` 不接收 `owner`、不接收 `source`、不接收任何主体信息。Event 只有 direction 和 density，锚点只有 direction 和 density。**场是唯一的记忆载体。**

用户是事件的主要来源，模型是另一个来源，recall 是事件的循环来源。场不关心来源的身份，只关心方向和密度。问"记忆属于谁"就像问"磁场属于谁"——问题本身就预设了一个场不承认的前提。

### 2. 方向漂移是场的物理行为，不是需要绕过的缺陷

锚点的 `direction` 在松弛周期中漂移，这是场的**正常演化**。`origin_direction` 保存了出生方向，但它只用于**诊断**（测量漂移量、检测范式转移候选），不用于**计算**。拿 `origin_direction` 替代 `direction` 参与 impact/recall 计算，等于说"场的演化没有物理意义"，这废除场本身。

如果漂移导致坍缩，应该**修力学规则**（恢复力、阻尼），不是绕过演化结果。

### 3. 场是认知地形，不是用户偏好通讯录

`init()` 的职责是**铺出一片密集的认知地形**——从一个领域（一种语言、一个工作流、一套知识体系）中提取 50-200 个概念，每个概念通过真实 embedding 获得 direction，密度由基础性决定。这样形成的场才有拓扑结构、有稀疏性、有梯度。

5-10 个"我喜欢橙色"式的锚点不是场，是 hashmap 的糟糕实现。用户偏好是场建立之后自然产生的微扰，不是场的骨架。

### 4. 唯一演化入口不可有旁路

`RelaxationCycle::run()` 是场状态变化的唯一下降路径。任何修改锚点 direction 的代码必须在松弛周期内部，不能在外部开辟第二漂移通道。

### 5. 核心方程不可增加参数

```
impact(anchor, event) = cos_sim(anchor.direction, event.direction) × √density
```

这是**唯一的物理量**。范式转移、ECG、召回、持久化全部由此推导。不允许在 impact 中加入时间衰减、来源加权、情感因子等"改进"——每一个都是在引入场不承认的分类与判断。

### 6. 场的构建是唯一合法的 LLM 调用

`init` 时 LLM 从用户意图描述中提取 40-60 个概念维度，这是场里**唯一合法的 LLM 调用**。这不是"运行时判断"，是"场的构建"——场诞生前它不存在，LLM 帮它获得初始结构。场运行后，所有行为从 `impact()` 涌现，不再调用 LLM。

交互模式：用户说"我希望你是什么样的"，而不是"你是什么样的"。一句话 → LLM 展开地形 → 概念 embedding → 密集锚点场。

### 7. 场是逐层沉积的，不是一次性浇筑

init 可以多次调用。每次追加 40-60 个锚点到已有场上。新锚点与已有锚点之间自然产生 impact 关系。场是逐步增长的多元地形，没有人工上限——LLM 觉得这个领域有 80 个维度就 80 个。5-10 个"我喜欢橙色"式的锚点不是场，是 hashmap 的糟糕实现。

## 设计意图

field-memory 抛弃了"向量数据库 + RAG"的传统路线。核心立场是：

- 记忆不是静态存储，是系统被事件扰动后收敛到的**平衡态**
- 概念的意义由关联事件的**空间密度**动态赋予，不由硬编码标签决定
- 系统拒绝分类与判断。没有 Gap、Valence、Emotion、RecallStamp。所有行为从**一个物理量**涌现
- 场是认知地形，不是用户偏好通讯录。init 构建密集锚点场，用户偏好是场建立后的微扰

核心方程：

```
impact(anchor, event) = cos_sim(anchor.direction, event.direction) * sqrt(anchor.density)
```

范式转移、ECG、召回、持久化全部由此推导。

## 核心元素

| 结构体 | 角色 | 关键字段 |
|---|---|---|
| `Event` | 输入到场的唯一信号 | direction, text, timestamp, source |
| `AnchorKey` | 记忆场的节点 | direction, density, stiffness, damping, origin_direction |
| `ImpactTrace` | 松弛副产品，召回用 | event_id, anchor_id, impact, timestamp |
| `SeedConcept` | L4 静默种子 | orthogonal_direction, shadow_anchor, defeated_by |

锚点的 `stiffness` 和 `damping` 由 `density` 自动推导：

```
stiffness = sqrt(density)    # 高密度 → 强响应（被相关事件拉得更用力）
damping   = sqrt(density)    # 高密度 → 强恢复力（抵抗漂移，核心锚点更稳定）
```

两者都随密度增长，但效果相反：stiffness 驱动锚点朝扰动方向移动（push），damping 把它拉回之前的位置（pull）。高密度锚点既响应强烈，又拒绝永久漂移——这是场的自稳机制。

## 文件结构

```
field-memory/
├── Cargo.toml
├── crates/
│   ├── core/
│   │   └── field-mem-core/    # 核心引擎 — 纯记忆逻辑，零外部依赖
│   │       └── src/*.rs
│   ├── ext-cli/               # 外挂：命令行 demo
│   └── ext-server/            # 外挂：HTTP 聊天服务器
│       └── src/static/        # 前端静态文件
│           ├── index.html     # 控制台 HTML 骨架
│           ├── style.css      # 控制台样式
│           ├── app.js         # 控制台逻辑
│           ├── field.html     # 3D 场可视化 HTML 骨架
│           ├── field.css      # 场可视化样式
│           └── field.js       # 场可视化逻辑（ES module）
├── docs/
│   ├── design.md
│   └── superpowers/plans/
└── AGENTS.md
```

核心引擎 `field-mem-core` 保持纯粹，不引入任何 HTTP、前端、鉴权、业务逻辑。外挂模块通过 Cargo workspace 引用核心，各自独立演进。

## 设计亮点

| 亮点 | 说明 |
|---|---|
| 无 LLM 运行时判定 | Event 输入只有 `text -> embedding -> direction`，运行时不调用 LLM |
| init 是唯一 LLM 调用 | 场构建时 LLM 提取概念维度，之后纯物理涌现 |
| 唯一演化入口 | `RelaxationCycle.run()` 是场状态变化的唯一路径，无旁路 |
| 自稳力学 | stiffness = damping = √d：高密度锚点响应强但恢复力也强，场自然抗拒坍缩 |
| 召回 = 写入同广播 | 查询和事件走同一个 `impact()` 函数 |
| 范式转移 = 唯一结构变换 | shadow_anchor 保留完整快照，恢复无损 |
| 5 个参数 | 向量维度、时间窗口、阻尼基数、刚度基数、收敛阈值 |

## 工程规约

- **5 个配置参数** — `DseCoreParams` 只增不减需要计划更新。
- **一文件一职责** — 不跨模块泄漏职责。
- **`engine.rs` 只做编排** — 算法改动在 `physics.rs` / `cycle.rs` / `recall.rs`。
- **工具两件套** — `init_field`（构建场）、`recall_memory`（召回），无其他。`seed_memory` 和 `associate_memory` 已废除。

### 前端规约

- **React + Next.js (static export)** — 使用 React 组件 + Next.js App Router，禁止手写 DOM 操作。
- **CSS Modules + CSS 变量** — 每个组件使用 `.module.css`，共享样式在 `globals.css` 中定义 CSS 变量。
- **`'use client'` 显式标记** — 所有包含浏览器端逻辑（事件、状态、ref、hooks）的组件文件第一行必须是 `'use client'`。
- **`dynamic()` + `ssr: false`** — 对包含 `localStorage` / `sessionStorage` 访问的组件使用 dynamic import + SSR 禁用。
- **SSR 安全** — 不要在 `useState` 初始化器或模块顶层访问 `window`、`localStorage`、`sessionStorage`；放在 `useEffect` 或惰性初始化器中。
- **类型定义汇总** — `src/types/index.ts` 所有类型定义汇总，不分散在组件中。
- **API 封装** — 所有 HTTP 请求通过 `lib/api.ts` 的 `apiCall` 函数。
- **视觉风格遵守** — 使用已有 CSS 变量（`--bg`, `--surface`, `--text`, `--accent`, `--border`, `--tag-bg`, `--panel-bg`, `--user-bubble`, `--text-muted`），不引入新颜色变量。
- **不使用 emoji，倾向于 icon 而非文字标签**。
- **面板折叠** — 侧栏面板通过 CSS transition 实现折叠，`collapsed` clsas 设置 `width: 0; padding: 0; border: none; margin: 0; opacity: 0; pointer-events: none`。

## 测试规约

- **内联 `#[cfg(test)] mod tests`** — 单元测试和实现写在一起。
- **`tests/integration.rs`** — 跨模块端到端测试。
- **依赖 `DummyEmbedProvider` 的测试天然不稳定** — 标记 `#[ignore]` 并写注释。

## 已记录教训

1. **`lib.rs` 的 re-export 不可引用不存在的模块**。先做骨架或推迟 re-export。
2. **`bincode 1.x` 中 `bincode::Error` 是 `Box<bincode::ErrorKind>`**。改用 `Serialization(String)` 手动 map。
3. **`sled::Error::Unsupported` 不接受字符串参数**。改用 `MissingData(&'static str)`。
4. **`Vec.remove()` 后不能复用之前绑定的 `n`**。移除后 `return` 不是 `break`。
5. **`DummyEmbedProvider` 基于哈希，不是语义**。
6. **单体 HTML 是维护灾难** — 1000+ 行 HTML 让修改定位极其困难，必须拆为 HTML + CSS + JS。
7. **折叠面板的 toggle 按钮不能放在面板内部** — `overflow:hidden` + `width:0` 会把按钮也隐藏掉，用户无法展开回来。按钮应放在容器中 absolute 定位，或用 JS 动态计算位置。
8. **JS state 与 HTML toggle class 必须一致** — `state.showLabels = false` 但 HTML 写 `class="toggle on"` 是已有 bug，拆分时必须对齐。

9. **回忆加强回忆是自洽行为，不需要阻止** — 召回在场中广播方向向量产生 ImpactTrace，密度增加是场的自然响应。真正需要防的是：系统管道内容（system prompt 注入的 recall 文本、tool call JSON 结果）被当作事件录入，以及 LLM 通过 init_field 将已存在的锚点当作新概念重复注入。防护层级：核心引擎用 `EventSource` 标记来源（信息性），服务端 init_field 做锚点去重，system prompt 加强防回写提示。
10. **bincode 不支持 `#[serde(default)]` 的缺字段反序列化** — bincode 是非自描述格式，旧数据缺少新字段会直接报错。persist 的 load 函数必须做版本容错：先尝试新格式，失败后 fallback 到 LegacyEvent 手动 backfill。

11. **会话真理源必须在服务端，不在浏览器** — 之前的设计是前端 `var sessions = []` 作为真理源，每次 mutation fire-and-forget POST 整个数组。这导致 5 个相互叠加的脆弱点：(a) tab 切换/关闭/冻结都丢 (b) 流式最后一笔 fire-and-forget 容易丢 (c) 多设备"最后写入赢"互相覆盖 (d) JSON 文件半截写损坏 (e) visibility 变化不强制 refresh。修复方式是服务端 hold `Arc<Mutex<SessionsData>>` + atomic write（tmp + fsync + rename），所有 mutation 走细粒度 endpoint（POST/PATCH/DELETE），流式输出原子化为 4 步：user msg 立即 POST → assistant 占位 await POST 拿 idx → 流过程不发请求 → 流结束 PATCH 占位。**只要连上服务端，会话永不丢失。**

12. **atomic rename 防半截写** — `std::fs::write` 直接覆盖目标文件，写入中途崩溃会留半截 JSON，下次 load 退回空。修复：先写 `path.tmp` → `sync_all()` 落盘 → `rename(tmp, path)`。POSIX 上 rename 是原子的，旧文件在 rename 完成前始终完整。配合 `Arc<Mutex<>>` 让所有 mutation 都走同一个 mutate helper，强制每次都 atomic 落盘，不会漏。

13. **系统回显与模型回复必须用不同 role 区分** — 命令响应（`/help`、`/status`、错误提示）如果用 `assistant` role 写进 messages 流，下次 send 会把它们当成 history 发给模型，污染 LLM context。修复：引入独立 role 字符串 `system_note`，send() 构造历史时已有过滤 `role in {user, assistant}` 自动排除；渲染时给差异化样式（居中、浅灰底、info 图标、低饱和度）。这条本质是教训 11 的延伸 — 服务端是真理源意味着 role 语义必须从源头正确划分，否则下游 send 构造历史时无法区分。


14. **damping 公式曾经写反，导致全场坍缩** — 原公式 `damping = 1/√d`，密度越高的锚点恢复力越弱，配合 `stiffness = √d`（高密度被推得更猛），形成双重加速坍缩：核心锚点最容易被漂走。修复为 `damping = √d`，与 stiffness 对称。两者同增但效果相反：stiffness 驱动位移（push），damping 抵抗位移（pull）。高密度锚点既响应强烈又拒绝漂移，这是场的自稳机制。**根因分析先于代码修改——发现坍缩不能绕过 direction 去用 origin_direction，那是在废除场的物理性。**

15. **`consolidate_from_recall` 不得修改 direction** — 旧实现中 recall 后会向 query 方向轻微拉动 direction（`moving_avg_direction`），这开辟了松弛周期之外的第二条漂移通道，违反"唯一演化入口"原则。修复后 `consolidate_from_recall` 只增 density + update_mechanics，不改 direction。密度反馈是允许的（recall 加强记忆是自洽行为），方向漂移必须只在 RelaxationCycle 内部发生。
