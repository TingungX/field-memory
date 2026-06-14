'use client';

import { useRef } from 'react';
import Rail from './Rail';
import SessionSidebar from './SessionSidebar';
import CorePanel from './CorePanel';
import MemoryPanel from './MemoryPanel';
import ChatPanel from '../chat/ChatPanel';
import SettingsModal from './SettingsModal';
import styles from './AppShell.module.css';
import type { AppConfig, Session } from '@/types';

interface AppShellProps {
  config: AppConfig;
  sessions: Session[];
  activeSessionId: string | null;
  loading: boolean;
  getActiveSession: () => Session | undefined;
  onRefreshSessions: () => void | Promise<void>;
  onCreateSession: () => Promise<{ error?: string | null; session?: Session }>;
  onSwitchSession: (id: string) => void | Promise<void>;
  onDeleteSession: (id: string) => Promise<void>;
  onAppendMessage: (sessionId: string, msg: Omit<Session['messages'][0], 'time'> & { time?: string }) => void;
  onUpdateMessage: (sessionId: string, idx: number, content: string) => Promise<string | null>;
  onDeleteMessage: (sessionId: string, idx: number) => Promise<string | null>;
  onUpdateSessionTitle: (sessionId: string, title: string) => void;
  onUpdateConfig: (partial: Partial<AppConfig>) => void;
  panelStates: Record<string, boolean>;
  onTogglePanel: (id: string) => void;
  onCloseAllPanels: () => void;
  anyPanelOpen: boolean;
  isMobile: boolean;
  status: import('@/types').MemoryStatus | null;
  currentLibrary: string;
  onFetchStatus: () => void;
}

export default function AppShell({
  config, sessions, activeSessionId, loading, getActiveSession,
  onRefreshSessions, onCreateSession, onSwitchSession, onDeleteSession,
  onAppendMessage, onUpdateMessage, onDeleteMessage, onUpdateSessionTitle,
  onUpdateConfig, panelStates, onTogglePanel, onCloseAllPanels, anyPanelOpen, isMobile,
  status, currentLibrary, onFetchStatus,
}: AppShellProps) {
  const settingsBtnRef = useRef<HTMLButtonElement>(null);

  return (
    <>
      <header className={styles.header}>
        <h1>field-memory</h1>
        <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>
          库: {currentLibrary}
        </span>
        <button
          ref={settingsBtnRef}
          className={styles.headerBtn}
          onClick={() => {
            const el = document.getElementById('settings-modal-trigger');
            if (el) (el as HTMLButtonElement).click();
          }}
        >
          设置
        </button>
      </header>

      <div className={styles.columns}>
        <Rail
          panelStates={panelStates}
          onTogglePanel={onTogglePanel}
          onCloseAllPanels={onCloseAllPanels}
          onSettingsClick={() => {
            const el = document.getElementById('settings-modal-trigger');
            if (el) (el as HTMLButtonElement).click();
          }}
        />

        <SessionSidebar
          sessions={sessions}
          activeSessionId={activeSessionId}
          collapsed={!panelStates['session-sidebar']}
          onSwitchSession={onSwitchSession}
          onCreateSession={onCreateSession}
          onDeleteSession={onDeleteSession}
          onRefreshSessions={onRefreshSessions}
        />

        <CorePanel
          collapsed={!panelStates['panel-core']}
        />

        <ChatPanel
          config={config}
          session={getActiveSession()}
          onAppendMessage={onAppendMessage}
          onUpdateMessage={onUpdateMessage}
          onDeleteMessage={onDeleteMessage}
          onUpdateSessionTitle={onUpdateSessionTitle}
          onFetchStatus={onFetchStatus}
        />

        <MemoryPanel
          collapsed={!panelStates['panel-mem']}
          currentLibrary={currentLibrary}
          status={status}
          onSave={() => {
            fetch('/api/memory/save', { method: 'POST' });
          }}
          onLoad={() => {
            fetch('/api/memory/load', { method: 'POST' });
          }}
        />
      </div>

      {/* Hidden trigger for settings modal */}
      <button
        id="settings-modal-trigger"
        style={{ display: 'none' }}
        onClick={() => {
          const evt = new CustomEvent('fm-open-settings');
          window.dispatchEvent(evt);
        }}
      />
      <SettingsModal config={config} onUpdateConfig={onUpdateConfig} />
    </>
  );
}
