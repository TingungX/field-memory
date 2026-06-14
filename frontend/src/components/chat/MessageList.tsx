'use client';

import { forwardRef, useEffect, useRef, useImperativeHandle, useState } from 'react';
import type { Message, ThinkingChainStep, ToolCall } from '@/types';
import ReactMarkdown from 'react-markdown';
import rehypeHighlight from 'rehype-highlight';
import styles from './MessageList.module.css';

interface MessageListProps {
  messages: Message[];
  isStreaming: boolean;
}

const ICON_INFO = '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="6.5"/><line x1="8" y1="7" x2="8" y2="11.5"/><circle cx="8" cy="4.7" r="0.6" fill="currentColor" stroke="none"/></svg>';

const ICON_CHEVRON = '<svg viewBox="0 0 8 8" fill="currentColor"><path d="M2 1 L6 4 L2 7 Z"/></svg>';

export interface MessageListHandle {
  scrollToBottom: () => void;
}

const MessageList = forwardRef<MessageListHandle, MessageListProps>(
  ({ messages, isStreaming }, ref) => {
    const elRef = useRef<HTMLDivElement>(null);
    const [detailIndex, setDetailIndex] = useState<number | null>(null);

    const scrollToBottom = () => {
      if (elRef.current) {
        elRef.current.scrollTop = elRef.current.scrollHeight;
      }
    };

    useImperativeHandle(ref, () => ({ scrollToBottom }));

    useEffect(() => {
      scrollToBottom();
    }, [messages]);

    if (!messages || messages.length === 0) {
      return (
        <div ref={elRef} className={styles.list}>
          <div className={styles.empty}>
            <strong>field-memory</strong><br />
            发送消息开始对话。<br />
            输入 / 查看命令。
          </div>
        </div>
      );
    }

    return (
      <div ref={elRef} id="messages" className={styles.list}>
        {messages.map((m, idx) => {
          if (m.role === 'system_note') {
            return (
              <div key={idx} className={`${styles.message} ${styles.systemNote}`}>
                <div className={styles.content}>
                  <svg className={styles.noteIcon} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                    <circle cx="8" cy="8" r="6.5" />
                    <line x1="8" y1="7" x2="8" y2="11.5" />
                    <circle cx="8" cy="4.7" r="0.6" fill="currentColor" stroke="none" />
                  </svg>
                  <span className={styles.noteText}>{m.content}</span>
                </div>
              </div>
            );
          }

          const isUser = m.role === 'user';
          const isAssistant = m.role === 'assistant';
          const thinkingChain = m.thinkingChain;
          const hasChain = thinkingChain && Array.isArray(thinkingChain) && thinkingChain.length > 0;

          return (
            <div
              key={idx}
              className={`${styles.message} ${isUser ? styles.user : styles.assistant}`}
            >
              {isAssistant && hasChain && (
                <ThinkingChainItem steps={thinkingChain!} />
              )}
              <div className={styles.content}>
                {isUser ? (
                  m.content
                ) : (
                  <ReactMarkdown rehypePlugins={[[rehypeHighlight, { ignoreMissing: true }]]}>
                    {m.content}
                  </ReactMarkdown>
                )}
              </div>
              {m.memoryCtx && (
                <MemoryContextItem ctx={m.memoryCtx} />
              )}
              <div className={styles.meta}>
                {isUser ? '你' : 'FM'} · {m.time || ''}
              </div>
            </div>
          );
        })}
        {isStreaming && (
          <div className={`${styles.message} ${styles.assistant} ${styles.typing}`}>
            <div className={styles.content}>
              正在思考
              <span className={styles.typingDot} />
            </div>
          </div>
        )}
      </div>
    );
  },
);

MessageList.displayName = 'MessageList';

// ── Thinking Chain (vertical timeline) ──

function ThinkingChainItem({ steps }: { steps: ThinkingChainStep[] }) {
  const [openIdx, setOpenIdx] = useState<number | null>(null);

  return (
    <div className={styles.chainWrap}>
      {steps.map((step, i) => {
        const isLast = i === steps.length - 1;
        const isOpen = openIdx === i;
        const isToolCall = step.type === 'tool_call';

        return (
          <div key={i} className={styles.chainNode}>
            <div className={`${styles.chainDot} ${isToolCall ? styles.dotTool : styles.dotReasoning}`} />
            <button
              type="button"
              className={`${styles.chainLabel} ${isOpen ? 'open' : ''}`}
              onClick={() => setOpenIdx(isOpen ? null : i)}
            >
              <span dangerouslySetInnerHTML={{ __html: ICON_CHEVRON }} />
              <span>
                {isToolCall
                  ? `工具调用 · ${getToolCallNames(step.content as ToolCall[])}`
                  : `思考 · ${(step.content as string).length} 字`}
              </span>
            </button>
            <div className={`${styles.chainDetail} ${isOpen ? styles.open : ''}`}>
              {isToolCall
                ? <ToolCallsBody calls={step.content as ToolCall[]} />
                : step.content as string
              }
            </div>
            {!isLast && <div className={styles.chainConnector} />}
          </div>
        );
      })}
    </div>
  );
}

function getToolCallNames(calls: ToolCall[]): string {
  return calls.map(c => c.function?.name || '(unnamed)').join(', ');
}

function ToolCallsBody({ calls }: { calls: ToolCall[] }) {
  return (
    <div>
      {calls.map((tc, i) => (
        <div key={i} className={styles.toolCallItem}>
          <div className={styles.toolCallName}>{tc.function?.name || '(unnamed)'}</div>
          {tc.function?.arguments && (
            <div className={styles.toolCallArgs}>
              {(() => {
                try { return JSON.stringify(JSON.parse(tc.function.arguments), null, 2); }
                catch { return tc.function.arguments; }
              })()}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

// ── Memory Context ──

function MemoryContextItem({ ctx }: { ctx: NonNullable<Message['memoryCtx']> }) {
  const [open, setOpen] = useState(false);

  return (
    <>
      <button
        type="button"
        className={`${styles.memToggle} ${open ? styles.open : ''}`}
        onClick={() => setOpen(!open)}
      >
        {ctx.tool_invoked ? '已调用 tool' : '记忆注入详情'}
      </button>
      <div className={`${styles.memDetail} ${open ? styles.open : ''}`}>
        {ctx.tool_invoked && <div className={styles.memFlag}>tool 已被模型调用</div>}
        {ctx.system_prompt_line && (
          <div><span className={styles.label}>注入：</span>{ctx.system_prompt_line}</div>
        )}
        {ctx.associations?.map((a, i) => (
          <div key={i} className={styles.item}>{a.label} ({a.impact})</div>
        ))}
        {ctx.recalled_events?.map((e, i) => (
          <div key={i} className={styles.item}>[{e.anchor}] {e.text}</div>
        ))}
      </div>
    </>
  );
}

export default MessageList;

