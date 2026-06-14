# Hermes 记忆库

> 来源：`~/.hermes/memories/MEMORY.md` + `USER.md`
> 导出时间：2026-06-14

---

## 一、用户画像

### 基本信息
- **身份**：高二学生，成都
- **设备**：MacBook Air M4 / macOS
- **Chat 工具**：Claude Desktop（从 CLI 换过来）
- **MBTI**：INTJ（但反感网上"刻意冷血表演"，认为高阶 INTJ 是不被群体影响，而非冷血）

### 聊天风格偏好
- 短句分条、鲜活直接、不一味附和
- 简单消息用短句连续发送，模拟真实聊天感
- 复杂任务保持正常格式
- 闲聊时不主动找资料（"等我查一下"而非秒回）

### AI 工具偏好
- **DeepSeek V4**：降价后性价比极高
- **MiniMax（token plan）+ DeepSeek**：组合好用
- **GLM Coding Plan**：抢不到（油猴脚本已失效），放弃随缘
- **LiteLLM**：通过代理整合 MiniMax/GLM/DeepSeek 等 Provider，换模型不改 Claude Desktop 配置（`~/Desktop/litellm-config.yaml`）

### 项目目录习惯
- 自己项目 → `~/Projects/`
- 克隆开源项目 → `~/OpenSource/`
- 当前 `~/Projects/`：含 github/xcode/mcp
- 当前 `~/OpenSource/`：含 hermes-web-ui/litellm

---

## 二、Hermes 自身行为记忆

### 消息缓冲机制
- Hermes 有内置消息缓冲：收到消息后不立刻回复，会等判断用户是否还有话要说
- **不要"秒回"**——判断完了再回
- 方向：输出锁/消息缓冲机制

### 打断消息
- 当前打断消息外观丑陋（⚡ Interrupting current task）
- 用户想要更自然的交互

### wait() 工具设计
- `wait(message_context)` 必填参数是用户原消息
- 超时后 LLM 根据上下文自行生成 probe（如"是什么？让我听听"），不做固定文案
- 流程：判断没说完 → wait(15) → 超时 → LLM 自拟 probe → 再等 15 秒 → 不回 → 进缓冲层

### Plan 输出规范
- 直接完整呈现，不说"存了"/"保存在 X"
- 需求确认用多选题，不问技术问题

---

## 三、运维记忆

### llm-proxy（端口 4000）
- 未注册 launchd
- 日志：`/tmp/llm-proxy-stdout.log` + `proxy.log`

#### apply_patch 转换链教训
1. **反向转换空 old_str 必须报错不放行**（否则两方向不收敛），从源头改 description 教模型用文件末尾几行当 old_str
2. **GLM-5 超长 tool_call arguments 被截断** → `json.loads` 失败 → 空 dict → `ReverseConversionError`，但 status="completed" 让 Codex 误以为成功，死循环。修复：降级为 `function_call_output` 返回错误
3. **OpenAI Responses API 限制**：`custom_tool_call` 不支持 status="failed"，只支持 incomplete/completed
4. 加 `APPEND_TOOL_DEF` 让模型分批写入避免截断

### Codex 工作流
- session 0 和其他 session 都走 llm-proxy 4000 端口
- apply_patch 都被转换
- tmux 0 是主工作区
- 说"贴原输出"要 `tmux capture-pane`，微信渲染不了时整理成 markdown 摘要
- 说"贴原文"时，不直接粘贴 tmux capture-pane 原始输出（微信会吞掉或渲染成空白），整理成干净的 markdown 摘要，只引用关键片段

---

## 四、清理/维护习惯

| 规则 | 说明 |
|---|---|
| 说"挨个看" | 要逐个确认 |
| 说"听你的" | 可全权委托 |
| Steam/Civilization VI/CrossOver | 还在用，清理时跳过 |
| hermes 依赖 uv | `.cache/uv` 不能清 |
| 用户用 pnpm 不用 npm | `~/.npm/` 缓存可清 |
| Outlines | 结构化文本生成库缓存可清 |
| litellm 态度 | 认为只是整合服务，已决定自建 |

---

## 五、微信文件传输
- 发文件格式：`MEDIA:/absolute/path`
- 已验证可发 PDF/JS/HTML 等文件
- 用户会要求把 Codex 改过的文件发过来，用 `git diff` 找文件再 MEDIA 发送
- 之前自行 cherry-pick calonye PR #12370 修复微信传文件，fix 持久有效

---

## 六、Minecraft
- 路径：`~/Desktop/.minecraft/`
- 版本：1.21.11 Fabric
- 模组：Sodium / Iris / JourneyMap / Lithium / EntityCulling / Axiom（已帮下 EntityCulling 1.10.1 和 Axiom 5.4.1）
- MetalFabricMod（Metalmine）：从 CurseForge 下载（9.7KB，只支持到 1.21.8），待实测

---

## 七、早安播报风格

> 核心要求：极简、诗意、谜语感
> 禁用：干巴巴数据、列表、robot 腔
> 必有：运势比喻（用户确认过）
> 不要：zodiac、不要大吉
> 地址：只写"成都"不写"成华区"

用户原话：*"这么长而干瘪的话谁看啊"*

