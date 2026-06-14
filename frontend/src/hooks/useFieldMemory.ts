'use client';

import { useState, useCallback, useRef } from 'react';
import type { Session, Message, AppConfig } from '@/types';
import { parseSSELine, buildThinkingChain } from '@/lib/sse-adapter';
import { resolveBackendUrl, apiCall } from '@/lib/api';

interface UseFieldMemoryOptions {
  session: Session | undefined;
  config: AppConfig;
  appendMessage: (sessionId: string, msg: Omit<Message, 'time'> & { time?: string }) => Promise<void>;
  updateMessage: (sessionId: string, idx: number, content: string) => Promise<string | null>;
  deleteMessage: (sessionId: string, idx: number) => Promise<string | null>;
}

export function useFieldMemory({
  session,
  config,
  appendMessage,
  updateMessage,
  deleteMessage,
}: UseFieldMemoryOptions) {
  const [isStreaming, setIsStreaming] = useState(false);
  const abortRef = useRef<AbortController | null>(null);

  // Stop streaming
  const stopStream = useCallback(() => {
    if (abortRef.current) {
      abortRef.current.abort();
      abortRef.current = null;
    }
    setIsStreaming(false);
  }, []);

  const send = useCallback(async (text: string) => {
    if (!session || !text.trim() || isStreaming) return;
    const s = session;
    if (!s) return;

    setIsStreaming(true);

    // Step 1: persist user message
    const time = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    appendMessage(s.id, { role: 'user', content: text, time });

    // Step 2: create assistant placeholder on server — must await for idx
    const time2 = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    const placeholderResp = await apiCall<{ idx: number }>(
      `/api/sessions/${encodeURIComponent(s.id)}/messages`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ role: 'assistant', content: '', time: time2, memoryCtx: null }),
      },
      '创建助手占位',
    );
    if (!placeholderResp.ok) {
      setIsStreaming(false);
      return;
    }
    const assistantIdx = placeholderResp.data.idx;

    // Add placeholder to local cache
    appendMessage(s.id, { role: 'assistant', content: '...', time: time2, memoryCtx: null });

    // Step 3: stream from LLM backend
    const baseUrl = resolveBackendUrl(config);
    const apiMessages = (s.messages || [])
      .filter(m => m.role === 'user' || m.role === 'assistant')
      .map(m => ({ role: m.role, content: m.content }));

    const controller = new AbortController();
    abortRef.current = controller;

    let fullText = '';
    let memoryCtx = null;
    let thinkingChain: import('@/types').ThinkingChainStep[] = [];

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

          if (parsed.memoryCtx) {
            memoryCtx = parsed.memoryCtx;
          }
          if (parsed.reasoningDelta) {
            buildThinkingChain(thinkingChain, parsed.reasoningDelta, null);
          }
          if (parsed.toolCalls) {
            buildThinkingChain(thinkingChain, null, parsed.toolCalls);
          }
          if (parsed.textDelta) {
            fullText += parsed.textDelta;
          }
        }
        if (controller.signal.aborted) break;
      }

      // Step 4: persist final content via PATCH (fire-and-forget)
      apiCall(`/api/sessions/${encodeURIComponent(s.id)}/messages/${assistantIdx}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          content: fullText,
          memoryCtx,
          thinkingChain: thinkingChain.length > 0 ? thinkingChain : null,
        }),
      }, '更新助手消息');

      // Update local cache
      const patchedR = await apiCall(`/api/sessions/${encodeURIComponent(s.id)}/messages/${assistantIdx}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          content: fullText,
          memoryCtx: memoryCtx,
          thinkingChain: thinkingChain.length > 0 ? thinkingChain : null,
        }),
      }, '更新助手消息');

    } catch (e: unknown) {
      // Not aborted → rollback placeholder
      if (!controller.signal.aborted) {
        deleteMessage(s.id, assistantIdx);
        appendMessage(s.id, { role: 'system_note', content: `错误: ${(e as Error).message}` });
      }
    }

    setIsStreaming(false);
    abortRef.current = null;

    return { fullText, memoryCtx, thinkingChain };
  }, [session, config, isStreaming, appendMessage, updateMessage, deleteMessage]);

  return { isStreaming, send, stopStream };
}

