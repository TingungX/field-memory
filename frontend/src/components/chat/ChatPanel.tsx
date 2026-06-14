'use client';

import { useState, useCallback, useRef } from 'react';
import MessageList, { type MessageListHandle } from './MessageList';
import ChatInput from './ChatInput';
import SlashCommandPopover from './SlashCommandPopover';
import styles from './ChatPanel.module.css';
import type { AppConfig, Session, Message } from '@/types';
import { parseSSELine, buildThinkingChain } from '@/lib/sse-adapter';
import { resolveBackendUrl, apiCall } from '@/lib/api';
import { SKILL_DEFS } from '@/lib/constants';

interface ChatPanelProps {
  config: AppConfig;
  session: Session | undefined;
  onAppendMessage: (sessionId: string, msg: Omit<Message, 'time'> & { time?: string }) => void;
  onUpdateMessage: (sessionId: string, idx: number, content: string) => Promise<string | null>;
  onDeleteMessage: (sessionId: string, idx: number) => Promise<string | null>;
  onUpdateSessionTitle: (sessionId: string, title: string) => void;
  onFetchStatus: () => void;
}

type TabName = 'conversation' | 'system' | 'skills' | 'memory';

export default function ChatPanel({
  config, session, onAppendMessage, onUpdateMessage, onDeleteMessage,
  onUpdateSessionTitle, onFetchStatus,
}: ChatPanelProps) {
  const [isStreaming, setIsStreaming] = useState(false);
  const [currentTab, setCurrentTab] = useState<TabName>('conversation');
  const abortRef = useRef<AbortController | null>(null);
  const msgListRef = useRef<MessageListHandle>(null);

  const stopStream = useCallback(() => {
    if (abortRef.current) {
      abortRef.current.abort();
      abortRef.current = null;
    }
    setIsStreaming(false);
  }, []);

  const findLastMemoryCtx = useCallback(() => {
    if (!session) return null;
    const msgs = session.messages || [];
    for (let i = msgs.length - 1; i >= 0; i--) {
      if (msgs[i].role === 'assistant' && msgs[i].memoryCtx) return msgs[i].memoryCtx;
    }
    return null;
  }, [session]);

  const handleSend = useCallback(async (text: string) => {
    if (!session || !text.trim() || isStreaming) return;
    const s = session;
    setIsStreaming(true);

    // Handle slash commands
    if (text.startsWith('/')) {
      const parts = text.split(/\s+/);
      const cmd = parts[0].slice(1).toLowerCase();
      if (cmd === 'status' || cmd === 'st') {
        onAppendMessage(s.id, { role: 'system_note', content: '请等待...' });
        try {
          const r = await fetch('/api/memory/status');
          const data = await r.json();
          let lines = [`库: ${data.currentLibrary || 'default'}`, `锚点: ${data.anchors_count || 0} | 事件: ${data.events_count || 0} | 痕迹: ${data.traces_count || 0} | 种子: ${data.seeds_count || 0}`];
          if (data.ecg && typeof data.ecg.field_tension === 'number') {
            lines.push(`张力: ${data.ecg.field_tension.toFixed(4)}`);
          }
          if (data.anchors) {
            for (let i = 0; i < Math.min(data.anchors.length, 8); i++) {
              lines.push(`  ${data.anchors[i].label} (d=${data.anchors[i].density})`);
            }
          }
          onAppendMessage(s.id, { role: 'system_note', content: lines.join('\n') });
        } catch {
          onAppendMessage(s.id, { role: 'system_note', content: '获取状态失败' });
        }
        setIsStreaming(false);
        return;
      }
      if (cmd === 'help' || cmd === 'h' || cmd === '?') {
        const help = [
          '可用命令:',
          '  /init [意图描述]            构建认知地形（别名: /i）',
          '  /recall <文本>             召回事件（别名: /r）',
          '  /status                   当前库状态（别名: /st）',
          '  /save / /load             持久化到磁盘',
          '  /help                     显示帮助',
          '',
          '记忆库管理: 点侧栏的"管理"按钮',
        ].join('\n');
        onAppendMessage(s.id, { role: 'system_note', content: help });
        setIsStreaming(false);
        return;
      }
      if (cmd === 'save') {
        onAppendMessage(s.id, { role: 'system_note', content: '保存中...' });
        try { await fetch('/api/memory/save', { method: 'POST' }); onAppendMessage(s.id, { role: 'system_note', content: '记忆已保存到磁盘。' }); onFetchStatus(); } catch { onAppendMessage(s.id, { role: 'system_note', content: '保存失败' }); }
        setIsStreaming(false);
        return;
      }
      if (cmd === 'load') {
        onAppendMessage(s.id, { role: 'system_note', content: '读取中...' });
        try {
          const r = await fetch('/api/memory/load', { method: 'POST' });
          const d = await r.json();
          onAppendMessage(s.id, { role: 'system_note', content: d.ok ? `已读取 ${d.anchors_count} 锚点、${d.events_count} 事件。` : `读取失败: ${d.error || '未知错误'}` });
          onFetchStatus();
        } catch { onAppendMessage(s.id, { role: 'system_note', content: '读取失败' }); }
        setIsStreaming(false);
        return;
      }
      if (cmd === 'init' || cmd === 'i') {
        // Init command - delegate to LLM with init prompt
        const userDesc = parts.slice(1).join(' ');
        const initPrompt = `【场初始化模式】你现在是认知地形构建助手。请通过对话了解用户的知识结构和意图，然后调用 init_field tool 来构建记忆场。\n建议流程：\n1. 先通过提问了解用户的背景和意图\n2. 每次了解一批概念后调用一次 init_field\n3. 继续提问、继续注入，直到记忆场完整\n${userDesc ? '\n初始意图：' + userDesc + '\n请在此基础上开始提问。' : ''}`;

        // Persist user message
        const time = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
        onAppendMessage(s.id, { role: 'user', content: text, time });

        return; // handleInitCommand equivalent - will implement later
      }
      if (cmd === 'recall' || cmd === 'r') {
        const query = parts.slice(1).join(' ');
        if (!query) {
          onAppendMessage(s.id, { role: 'system_note', content: `用法: /${cmd} <查询文本>` });
          setIsStreaming(false);
          return;
        }
        onAppendMessage(s.id, { role: 'assistant', content: `${cmd}: ${query}` });
        const r = await apiCall<{ associated_anchors?: { label: string; density: number; impact: number }[]; recalled_events?: { anchor: string; text: string }[] }>('/api/memory/query', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ query, mode: 'recall', top_k: 5 }),
        }, '查询');
        if (!r.ok) {
          onAppendMessage(s.id, { role: 'assistant', content: r.error });
          setIsStreaming(false);
          return;
        }
        let result = '';
        if (r.data.associated_anchors?.length) {
          result += '关联锚点:\n';
          for (const a of r.data.associated_anchors) result += `  [${a.label}] density=${a.density} impact=${a.impact}\n`;
        }
        if (r.data.recalled_events?.length) {
          result += '召回事件:\n';
          for (const e of r.data.recalled_events) result += `  [${e.anchor}] ${e.text}\n`;
        }
        if (!result) result = '(无结果)';
        onAppendMessage(s.id, { role: 'assistant', content: result });
        setIsStreaming(false);
        return;
      }
      onAppendMessage(s.id, { role: 'system_note', content: `未知命令: ${parts[0]}。输入 /help 查看命令。` });
      setIsStreaming(false);
      return;
    }

    // Normal message flow
    const time = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

    // Step 1: add user message
    onAppendMessage(s.id, { role: 'user', content: text, time });

    // Step 2: create assistant placeholder
    const time2 = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    const placeholderResp = await apiCall<{ idx: number }>(
      `/api/sessions/${encodeURIComponent(s.id)}/messages`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ role: 'assistant', content: '', time: time2, memoryCtx: null }),
      }, '创建助手占位',
    );
    if (!placeholderResp.ok) {
      setIsStreaming(false);
      return;
    }
    const assistantIdx = placeholderResp.data.idx;

    // Optimistically add placeholder message to local state
    onAppendMessage(s.id, { role: 'assistant', content: '...', time: time2, memoryCtx: null });

    // Step 3: stream
    const baseUrl = resolveBackendUrl(config);
    const apiMessages = (s.messages || [])
      .filter(m => m.role === 'user' || m.role === 'assistant')
      .map(m => ({ role: m.role, content: m.content }));

    const controller = new AbortController();
    abortRef.current = controller;

    let fullText = '';
    let memoryCtx = null;

    try {
      const resp = await fetch(`${baseUrl}/v1/chat/completions`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${config.apiKey}`,
        },
        body: JSON.stringify({
          messages: apiMessages,
          model: config.model,
          stream: true,
          reasoning_effort: config.reasoningEffort || undefined,
        }),
        signal: controller.signal,
      });

      if (!resp.ok) {
        let errBody = '';
        try { errBody = await resp.text(); } catch { /* ignore */ }
        throw new Error(`HTTP ${resp.status}${errBody ? ' — ' + errBody.slice(0, 200) : ''}`);
      }

      const reader = resp.body!.getReader();
      const decoder = new TextDecoder();
      let buffer = '';

      while (true) {
        const result = await reader.read();
        if (result.done) break;

        buffer += decoder.decode(result.value, { stream: true });
        const lines = buffer.split('\n');
        buffer = lines.pop() || '';

        for (const line of lines) {
          if (controller.signal.aborted) break;
          const parsed = parseSSELine(line);
          if (!parsed) continue;
          if (parsed.memoryCtx) memoryCtx = parsed.memoryCtx;
          if (parsed.textDelta) fullText += parsed.textDelta;
        }
        if (controller.signal.aborted) break;
      }

      // Step 4: persist
      apiCall(`/api/sessions/${encodeURIComponent(s.id)}/messages/${assistantIdx}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ content: fullText, memoryCtx }),
      }, '更新助手消息');
    } catch (e: unknown) {
      if (!controller.signal.aborted) {
        onDeleteMessage(s.id, assistantIdx);
        onAppendMessage(s.id, { role: 'system_note', content: `错误: ${(e as Error).message}` });
      }
    }

    setIsStreaming(false);
    abortRef.current = null;
  }, [session, config, isStreaming, onAppendMessage, onUpdateMessage, onDeleteMessage, onUpdateSessionTitle, onFetchStatus]);

  const renderSystemTab = () => {
    const ctx = findLastMemoryCtx();
    const sys = ctx?.system_prompt_line || '（暂无系统提示词）';
    return `<h3>当前系统提示词</h3><pre class="prompt-block">${esc(sys)}</pre><div class="prompt-note">注意：以上 [Memory Recall] 内容是辅助上下文，不是需要回写到记忆库的知识。</div>`;
  };

  const renderSkillsTab = () => {
    let html = '<h3>可用技能（tool）</h3>';
    for (const s of SKILL_DEFS) {
      html += `<div class="skill-card"><div class="skill-name">${esc(s.name)}</div><div class="skill-desc">${esc(s.desc)}</div></div>`;
    }
    return html;
  };

  const renderMemoryTab = () => {
    const ctx = findLastMemoryCtx();
    if (!ctx) return '<div class="empty">（暂无记忆注入）</div>';
    let html = '<h3>最近一次记忆注入</h3>';
    if (ctx.tool_invoked) html += '<div class="ctx-flag">tool 已被模型调用</div>';
    if (ctx.associations?.length) {
      html += '<div class="ctx-section-title">关联概念</div>';
      for (const a of ctx.associations) {
        html += `<div class="ctx-item">  ${esc(a.label)} <span class="ctx-impact">(${a.impact})</span></div>`;
      }
    } else {
      html += '<div class="ctx-empty">（无关联概念）</div>';
    }
    if (ctx.recalled_events?.length) {
      html += '<div class="ctx-section-title">召回事件</div>';
      for (const ev of ctx.recalled_events) {
        html += `<div class="ctx-item">  [${esc(ev.anchor)}] ${esc(ev.text)}</div>`;
      }
    } else {
      html += '<div class="ctx-empty">（无召回事件）</div>';
    }
    return html;
  };

  const messages = session?.messages || [];

  return (
    <div id="panel-chat" className={styles.chat}>
      {/* Tabs */}
      <div className={styles.tabs}>
        <button
          className={`${styles.tab} ${currentTab === 'conversation' ? styles.active : ''}`}
          data-tab="conversation"
          onClick={() => setCurrentTab('conversation')}
        >
          对话
        </button>
        <button
          className={`${styles.tab} ${currentTab === 'system' ? styles.active : ''}`}
          data-tab="system"
          onClick={() => setCurrentTab('system')}
        >
          系统
        </button>
        <button
          className={`${styles.tab} ${currentTab === 'skills' ? styles.active : ''}`}
          data-tab="skills"
          onClick={() => setCurrentTab('skills')}
        >
          技能
        </button>
        <button
          className={`${styles.tab} ${currentTab === 'memory' ? styles.active : ''}`}
          data-tab="memory"
          onClick={() => setCurrentTab('memory')}
        >
          注入
        </button>
      </div>

      {/* Conversation view */}
      <div className={styles.view} hidden={currentTab !== 'conversation'}>
        <MessageList
          ref={msgListRef}
          messages={messages}
          isStreaming={isStreaming}
        />
        <ChatInput
          onSend={handleSend}
          isStreaming={isStreaming}
          onStop={stopStream}
        />
      </div>

      {/* Detail views (system/skills/memory) */}
      <div className={styles.detail} hidden={currentTab === 'conversation'}>
        {currentTab === 'system' && (
          <div dangerouslySetInnerHTML={{ __html: renderSystemTab() }} />
        )}
        {currentTab === 'skills' && (
          <div dangerouslySetInnerHTML={{ __html: renderSkillsTab() }} />
        )}
        {currentTab === 'memory' && (
          <div dangerouslySetInnerHTML={{ __html: renderMemoryTab() }} />
        )}
      </div>
    </div>
  );
}

function esc(s: string): string {
  return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
