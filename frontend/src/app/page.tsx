'use client';

import dynamic from 'next/dynamic';
import { useConfig } from '@/hooks/useConfig';
import { useSessions } from '@/hooks/useSessions';
import { useMemoryStatus, useLibraries } from '@/hooks/useMemoryStatus';
import { usePanelState } from '@/hooks/usePanelState';

// Dynamically import AppShell to avoid SSR issues with browser-only APIs
const AppShell = dynamic(() => import('@/components/layout/AppShell'), {
  ssr: false,
});

export default function Home() {
  const { config, updateConfig } = useConfig();
  const {
    sessions, activeSessionId, loading,
    getActiveSession, refreshSessions, createSession,
    switchSession, deleteSession,
    appendMessage, updateMessage, deleteMessage,
    updateSessionTitle,
  } = useSessions();

  const { status, refetch: fetchStatus } = useMemoryStatus();
  const { currentLibrary } = useLibraries();

  const { states: panelStates, togglePanel, closeAll, anyOpen: anyPanelOpen, isMobile } = usePanelState([
    'session-sidebar', 'panel-core', 'panel-mem',
  ]);

  return (
    <AppShell
      config={config}
      sessions={sessions}
      activeSessionId={activeSessionId}
      loading={loading}
      getActiveSession={getActiveSession}
      onRefreshSessions={refreshSessions}
      onCreateSession={createSession}
      onSwitchSession={switchSession}
      onDeleteSession={deleteSession}
      onAppendMessage={appendMessage}
      onUpdateMessage={updateMessage}
      onDeleteMessage={deleteMessage}
      onUpdateSessionTitle={updateSessionTitle}
      onUpdateConfig={updateConfig}
      panelStates={panelStates}
      onTogglePanel={togglePanel}
      onCloseAllPanels={closeAll}
      anyPanelOpen={anyPanelOpen}
      isMobile={isMobile}
      status={status}
      currentLibrary={currentLibrary}
      onFetchStatus={fetchStatus}
    />
  );
}

