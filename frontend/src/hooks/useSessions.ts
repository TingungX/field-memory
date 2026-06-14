'use client';

import { useState, useCallback, useEffect, useRef } from 'react';
import type { Session, Message } from '@/types';
import { apiCall } from '@/lib/api';

/**
 * Session persistence hook.
 *
 * Server is source of truth (Arc<Mutex<SessionsData>> + atomic write).
 * Client keeps an optimistic cache initialized from the server on load.
 * Every mutation immediately goes through a dedicated endpoint.
 */

export function useSessions() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [activeSessionId, setActiveSessionIdState] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const cacheRef = useRef<Session[]>([]);
  const initializedRef = useRef(false);

  // Lazy init from localStorage (SSR-safe)
  useEffect(() => {
    if (!initializedRef.current) {
      initializedRef.current = true;
      const stored = localStorage.getItem('fm-active-id');
      if (stored) {
        setActiveSessionIdState(stored);
      }
    }
  }, []);

  // Helper: set active id + persist to localStorage
  const setActiveSessionId = useCallback((id: string | null) => {
    setActiveSessionIdState(id);
    if (id) {
      localStorage.setItem('fm-active-id', id);
    } else {
      localStorage.removeItem('fm-active-id');
    }
  }, []);

  // Get active session from cache
  const getActiveSession = useCallback((): Session | undefined => {
    return cacheRef.current.find(s => s.id === activeSessionId);
  }, [activeSessionId]);

  // Refresh from server
  const refreshSessions = useCallback(async () => {
    const r = await apiCall<{ sessions: Session[] }>('/api/sessions', { method: 'GET' }, '刷新会话');
    if (!r.ok) {
      console.warn('refreshSessions:', r.error);
      return;
    }
    const data = r.data;
    if (!data || !data.sessions) return;

    const fetched = data.sessions;
    cacheRef.current = fetched;
    setSessions(fetched);

    // Handle active session fallback
    const currentId = activeSessionId;
    if (currentId && !fetched.find(s => s.id === currentId)) {
      const newId = fetched.length > 0 ? fetched[0].id : null;
      if (newId) setActiveSessionId(newId);
    } else if (!currentId && fetched.length > 0) {
      setActiveSessionId(fetched[0].id);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Create session
  const createSession = useCallback(async () => {
    const r = await apiCall<{ session: Session }>('/api/sessions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title: '新会话' }),
    }, '创建会话');
    if (!r.ok) return { error: r.error };

    const session = r.data.session;
    cacheRef.current = [...cacheRef.current, session];
    setSessions(cacheRef.current);
    setActiveSessionId(session.id);
    return { session, error: null };
  }, [setActiveSessionId]);

  // Switch session
  const switchSession = useCallback((id: string) => {
    setActiveSessionId(id);
    // Fire-and-forget: tell server
    apiCall(`/api/sessions/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ active: true }),
    }, '切换会话');
  }, [setActiveSessionId]);

  // Delete session
  const deleteSession = useCallback(async (id: string) => {
    // Optimistic: remove locally first
    const filtered = cacheRef.current.filter(s => s.id !== id);
    cacheRef.current = filtered;
    setSessions(filtered);

    if (activeSessionId === id) {
      const newId = filtered.length > 0 ? filtered[0].id : null;
      setActiveSessionId(newId);
    }

    // Then tell server
    const r = await apiCall(`/api/sessions/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    }, '删除会话');
    if (!r.ok) {
      // Rollback on failure
      await refreshSessions();
    }
  }, [activeSessionId, setActiveSessionId, refreshSessions]);

  // Append message to session (optimistic local + fire-and-forget)
  const appendMessage = useCallback((sessionId: string, msg: Omit<Message, 'time'> & { time?: string }) => {
    const time = msg.time || new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    const fullMsg = { ...msg, time };

    // Optimistic local update
    const updated = cacheRef.current.map(s => {
      if (s.id !== sessionId) return s;
      return { ...s, messages: [...(s.messages || []), fullMsg] };
    });
    cacheRef.current = updated;
    setSessions(updated);

    // Fire-and-forget: persist server-side
    apiCall(`/api/sessions/${encodeURIComponent(sessionId)}/messages`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(fullMsg),
    }, '追加消息');
  }, []);

  // Update message content (for edit-and-resend)
  const updateMessage = useCallback(async (sessionId: string, idx: number, content: string) => {
    const r = await apiCall(`/api/sessions/${encodeURIComponent(sessionId)}/messages/${idx}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ content }),
    }, '更新消息');
    if (!r.ok) return r.error;

    // Local update
    const updated = cacheRef.current.map(s => {
      if (s.id !== sessionId) return s;
      const msgs = [...(s.messages || [])];
      if (msgs[idx]) msgs[idx] = { ...msgs[idx], content };
      return { ...s, messages: msgs };
    });
    cacheRef.current = updated;
    setSessions(updated);
    return null;
  }, []);

  // Delete message (for truncation after edit)
  const deleteMessage = useCallback(async (sessionId: string, idx: number) => {
    const r = await apiCall(`/api/sessions/${encodeURIComponent(sessionId)}/messages/${idx}`, {
      method: 'DELETE',
    }, '删除消息');
    if (!r.ok) return r.error;

    const updated = cacheRef.current.map(s => {
      if (s.id !== sessionId) return s;
      const msgs = [...(s.messages || [])];
      if (idx < msgs.length) msgs.splice(idx, 1);
      return { ...s, messages: msgs };
    });
    cacheRef.current = updated;
    setSessions(updated);
    return null;
  }, []);

  // Update session title
  const updateSessionTitle = useCallback((sessionId: string, title: string) => {
    const updated = cacheRef.current.map(s => {
      if (s.id !== sessionId) return s;
      return { ...s, title };
    });
    cacheRef.current = updated;
    setSessions(updated);

    apiCall(`/api/sessions/${encodeURIComponent(sessionId)}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title }),
    }, '更新标题');
  }, []);

  // Refresh on visibility change
  useEffect(() => {
    const handler = () => {
      if (!document.hidden) refreshSessions();
    };
    document.addEventListener('visibilitychange', handler);
    return () => document.removeEventListener('visibilitychange', handler);
  }, [refreshSessions]);

  // Initial load
  useEffect(() => {
    refreshSessions().then(() => setLoading(false));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return {
    sessions,
    activeSessionId,
    loading,
    getActiveSession,
    refreshSessions,
    createSession,
    switchSession,
    deleteSession,
    appendMessage,
    updateMessage,
    deleteMessage,
    updateSessionTitle,
  };
}

