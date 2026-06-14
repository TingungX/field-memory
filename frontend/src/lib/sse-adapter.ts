// ── Custom SSE adapter for field-memory ext-server ──
//
// The ext-server uses standard OpenAI-compatible SSE streaming (data: prefix),
// but adds three custom event prefixes within data:
//
//   data: __MEMORY__{"tool_invoked":true,...}
//   data: __REASONING__{"delta":"思考文本..."}
//   data: __TOOL_CALLS__[{"function":{"name":"recall_memory",...}}]
//
// This adapter parses the raw stream and extracts these custom events,
// returning structured results to the caller.

import type { MemoryCtx, ThinkingChainStep, ToolCall } from '@/types';

export interface ParsedSSEChunk {
  /** Regular text delta (appended to assistant content) */
  textDelta?: string;
  /** Memory context injected by the server */
  memoryCtx?: MemoryCtx;
  /** Reasoning delta for thinking chain */
  reasoningDelta?: string;
  /** Tool calls for thinking chain */
  toolCalls?: ToolCall[];
}

/**
 * Parse a single SSE data line and extract any known field-memory events.
 * Returns null if the line should be ignored (empty, [DONE], unrecognized).
 */
export function parseSSELine(line: string): ParsedSSEChunk | null {
  const trimmed = line.trim();
  if (!trimmed || trimmed === '[DONE]') return null;

  // Must start with 'data: '
  if (!trimmed.startsWith('data: ')) return null;

  const data = trimmed.slice(6);

  // Custom memory event
  if (data.startsWith('__MEMORY__')) {
    try {
      const ctx = JSON.parse(data.slice(10)) as MemoryCtx;
      return { memoryCtx: ctx };
    } catch {
      return null;
    }
  }

  // Custom reasoning event
  if (data.startsWith('__REASONING__')) {
    try {
      const rp = JSON.parse(data.slice('__REASONING__'.length));
      if (rp && typeof rp.delta === 'string') {
        return { reasoningDelta: rp.delta };
      }
    } catch {
      // ignore parse errors
    }
    return null;
  }

  // Custom tool calls event
  if (data.startsWith('__TOOL_CALLS__')) {
    try {
      const tc = JSON.parse(data.slice('__TOOL_CALLS__'.length)) as ToolCall[];
      if (Array.isArray(tc)) {
        return { toolCalls: tc };
      }
    } catch {
      // ignore parse errors
    }
    return null;
  }

  // Standard text content
  return { textDelta: data };
}

/**
 * Build a thinking chain array incrementally.
 * reasoningDelta appends to the last reasoning node or creates a new one.
 * newToolCalls appends a tool_call node.
 */
export function buildThinkingChain(
  chain: ThinkingChainStep[],
  reasoningDelta: string | null,
  newToolCalls: ToolCall[] | null,
): ThinkingChainStep[] {
  if (reasoningDelta) {
    if (chain.length > 0 && chain[chain.length - 1].type === 'reasoning') {
      (chain[chain.length - 1].content as string) += reasoningDelta;
    } else {
      chain.push({ type: 'reasoning', content: reasoningDelta });
    }
  }
  if (newToolCalls && Array.isArray(newToolCalls) && newToolCalls.length > 0) {
    chain.push({ type: 'tool_call', content: newToolCalls });
  }
  return chain;
}

/**
 * Backwards compatibility: convert legacy (reasoning string, toolCalls array)
 * into a thinkingChain array.
 */
export function legacyToThinkingChain(
  reasoning: string | null | undefined,
  toolCalls: ToolCall[] | null | undefined,
): ThinkingChainStep[] {
  const chain: ThinkingChainStep[] = [];
  if (reasoning && typeof reasoning === 'string' && reasoning.trim()) {
    chain.push({ type: 'reasoning', content: reasoning });
  }
  if (toolCalls && Array.isArray(toolCalls) && toolCalls.length > 0) {
    chain.push({ type: 'tool_call', content: toolCalls });
  }
  return chain;
}

