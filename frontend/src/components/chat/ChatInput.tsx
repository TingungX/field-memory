'use client';

import { useState, useRef, useCallback } from 'react';
import SlashCommandPopover from './SlashCommandPopover';
import styles from './ChatInput.module.css';
import { EFFORT_OPTIONS, SLASH_COMMANDS } from '@/lib/constants';

interface ChatInputProps {
  onSend: (text: string) => void;
  isStreaming: boolean;
  onStop: () => void;
}

export default function ChatInput({ onSend, isStreaming, onStop }: ChatInputProps) {
  const [text, setText] = useState('');
  const [showCmd, setShowCmd] = useState(false);
  const [cmdFiltered, setCmdFiltered] = useState<typeof SLASH_COMMANDS>([]);
  const [cmdIndex, setCmdIndex] = useState(-1);
  const [effort, setEffort] = useState('medium');
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const handleInput = useCallback((val: string) => {
    setText(val);
    if (val.startsWith('/') && !val.includes(' ')) {
      const query = val.toLowerCase();
      const filtered = SLASH_COMMANDS.filter(
        c => c.name.startsWith(query) || (c.alias && c.alias.startsWith(query))
      );
      if (filtered.length > 0) {
        setCmdFiltered(filtered);
        setCmdIndex(0);
        setShowCmd(true);
        return;
      }
    }
    setShowCmd(false);
  }, []);

  const confirmCmd = useCallback(() => {
    if (cmdIndex >= 0 && cmdIndex < cmdFiltered.length) {
      setText(cmdFiltered[cmdIndex].name + ' ');
      setShowCmd(false);
      textareaRef.current?.focus();
    }
  }, [cmdIndex, cmdFiltered]);

  const handleKey = useCallback((e: React.KeyboardEvent) => {
    if (showCmd) {
      if (e.key === 'ArrowDown') { e.preventDefault(); setCmdIndex(i => Math.min(i + 1, cmdFiltered.length - 1)); return; }
      if (e.key === 'ArrowUp') { e.preventDefault(); setCmdIndex(i => Math.max(i - 1, 0)); return; }
      if (e.key === 'Enter' || e.key === 'Tab') { e.preventDefault(); confirmCmd(); return; }
      if (e.key === 'Escape') { e.preventDefault(); setShowCmd(false); return; }
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      if (isStreaming) { onStop(); return; }
      if (text.trim()) {
        onSend(text.trim());
        setText('');
      }
    }
    // Auto-resize
    setTimeout(() => {
      if (textareaRef.current) {
        textareaRef.current.style.height = 'auto';
        textareaRef.current.style.height = Math.min(textareaRef.current.scrollHeight, 120) + 'px';
      }
    }, 0);
  }, [showCmd, cmdFiltered, cmdIndex, confirmCmd, isStreaming, text, onSend, onStop]);

  const handleSendClick = useCallback(() => {
    if (isStreaming) { onStop(); return; }
    if (text.trim()) {
      onSend(text.trim());
      setText('');
    }
  }, [isStreaming, text, onSend, onStop]);

  return (
    <div id="input-area" className={styles.area}>
      <select
        className={styles.effort}
        title="思考强度"
        value={effort}
        onChange={e => setEffort(e.target.value)}
      >
        {EFFORT_OPTIONS.map(opt => (
          <option key={opt.value} value={opt.value}>{opt.label}</option>
        ))}
      </select>
      <div className={styles.inputWrap}>
        <textarea
          ref={textareaRef}
          id="user-input"
          className={styles.input}
          rows={1}
          placeholder="输入消息，/ 查看命令…"
          value={text}
          onChange={e => handleInput(e.target.value)}
          onKeyDown={handleKey}
        />
        {showCmd && (
          <SlashCommandPopover
            items={cmdFiltered}
            selectedIndex={cmdIndex}
            onSelect={confirmCmd}
          />
        )}
      </div>
      <button
        id="send-btn"
        className={`${styles.sendBtn} ${isStreaming ? styles.stopBtn : ''}`}
        onClick={handleSendClick}
        title={isStreaming ? '停止' : '发送'}
      >
        {isStreaming ? '■' : '↑'}
      </button>
    </div>
  );
}

