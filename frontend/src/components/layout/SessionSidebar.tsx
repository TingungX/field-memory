'use client';

import { useState } from 'react';
import type { Session } from '@/types';
import styles from './SessionSidebar.module.css';

interface SessionSidebarProps {
  sessions: Session[];
  activeSessionId: string | null;
  collapsed: boolean;
  onSwitchSession: (id: string) => void | Promise<void>;
  onCreateSession: () => Promise<{ error?: string | null; session?: Session }>;
  onDeleteSession: (id: string) => Promise<void>;
  onRefreshSessions: () => void | Promise<void>;
}

export default function SessionSidebar({
  sessions, activeSessionId, collapsed,
  onSwitchSession, onCreateSession, onDeleteSession, onRefreshSessions,
}: SessionSidebarProps) {
  const [deleting, setDeleting] = useState<string | null>(null);

  const handleDelete = async (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    if (deleting === id) {
      await onDeleteSession(id);
      setDeleting(null);
    } else {
      setDeleting(id);
      setTimeout(() => setDeleting(null), 3000);
    }
  };

  return (
    <div
      id="session-sidebar"
      className={`${styles.sidebar} ${collapsed ? styles.collapsed : ''}`}
    >
      <div className={styles.header}>
        <h3>会话</h3>
        <div className={styles.actions}>
          <button
            className={styles.btn}
            title="同步"
            onClick={() => onRefreshSessions()}
          >
            ↻
          </button>
          <button
            className={`${styles.btn} ${styles.newBtn}`}
            title="新建会话"
            onClick={() => onCreateSession()}
          >
            +
          </button>
        </div>
      </div>
      <div className={styles.list}>
        {sessions.map(s => (
          <div
            key={s.id}
            className={`${styles.item} ${s.id === activeSessionId ? styles.active : ''}`}
            onClick={() => onSwitchSession(s.id)}
          >
            <span className={styles.label} title={s.title}>
              {s.title || '新会话'}
            </span>
            <button
              className={styles.delBtn}
              onClick={e => handleDelete(e, s.id)}
              title={deleting === s.id ? '再次点击确认删除' : '删除会话'}
            >
              {deleting === s.id ? '确认?' : '×'}
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
