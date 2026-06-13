# field-memory

**统一势能场记忆引擎。** 给 LLM 应用装上会自己长、自己忘、自己演化的长期记忆。

## Features

- **物理驱动的记忆**。没有 Gap、Valence、Emotion 之类的硬编码分类。事件只是方向，记忆是一个势能场，系统被扰动后自然收敛到平衡态。
- **核心方程只有一行**。`impact = cos_sim × sqrt(density)`。范式转移、回忆加固、遗忘演化全部由此涌现，不需要独立模块。
- **不需要额外 LLM 调用**。不做语义提取、情绪分类、gap 评估。embedding 一步到位，embedding 之后全是纯向量运算。
- **5 个配置参数**。向量维度、时间窗口、阻尼系数、刚度系数、收敛阈值。没了。
- **Rust 实现**。axum 服务器，sled 嵌入式持久化，单二进制部署，无外部依赖。

## 快速开始

```bash
# 初始化 -> 写入 -> 松弛 -> 召回 -> 持久化 完整链路
cargo run -p dse-cli

# 启动聊天服务器
LLM_BACKEND=https://api.deepseek.com/v1/chat/completions \
LLM_MODEL=deepseek-chat \
LLM_API_KEY=sk-your-key-here \
cargo run -p dse-server
```

浏览器打开 `http://localhost:3000`。

## API

同时支持 OpenAI 和 Anthropic 格式。

```bash
# OpenAI 格式
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"messages":[{"role":"user","content":"你好"}],"stream":true}'

# Anthropic 格式
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -d '{"messages":[{"role":"user","content":"你好"}],"stream":true}'
```

每条消息自动经过：召回相关记忆 → 注入上下文 → 转发后端 LLM → 流式返回。后台异步完成记忆写入和演化。

## 部署

```bash
# 单二进制部署
cargo build --release
./target/release/dse-server

# 或指定后端 LLM
LLM_BACKEND=https://api.openai.com/v1/chat/completions \
LLM_MODEL=gpt-4o \
LLM_API_KEY=sk-xxx \
cargo run -p dse-server
```

数据默认存在本地 sled 数据库文件中。重启不丢记忆。

## 技术栈

| 组件 | 选型 |
|---|---|
| 语言 | Rust 2021 |
| HTTP | axum |
| 持久化 | sled（嵌入式 KV，纯 Rust） |
| 序列化 | serde + bincode |
| 前端 | 静态 HTML/CSS/JS，零构建依赖 |
| embedding | EmbedProvider trait，可对接任意模型 |

## License

Copyright (C) 2026 Tingung <TingungX@outlook.com>

**GNU Affero General Public License v3.0** — 如果你将本引擎作为网络服务提供修改版本，必须向服务用户提供修改后的源代码。
