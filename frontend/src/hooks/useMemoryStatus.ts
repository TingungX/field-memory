'use client';

import { useState, useEffect, useRef, useCallback } from 'react';
import type { MemoryStatus, LibraryList } from '@/types';
import { apiCall } from '@/lib/api';

export function useMemoryStatus() {
  const [status, setStatus] = useState<MemoryStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchStatus = useCallback(async () => {
    const r = await apiCall<MemoryStatus>('/api/memory/status', { method: 'GET' }, '获取状态');
    if (r.ok && r.data) {
      setStatus(r.data);
    }
  }, []);

  // Start / stop polling
  useEffect(() => {
    fetchStatus().then(() => setLoading(false));
    pollRef.current = setInterval(fetchStatus, 1500);
    return () => {
      if (pollRef.current) clearInterval(pollRef.current);
    };
  }, [fetchStatus]);

  return { status, loading, refetch: fetchStatus };
}

export function useLibraries() {
  const [currentLibrary, setCurrentLibrary] = useState('default');
  const [libraries, setLibraries] = useState<LibraryList['libraries']>([]);
  const [loading, setLoading] = useState(true);

  const loadLibraryList = useCallback(async () => {
    const r = await apiCall<LibraryList>('/api/memory/libraries', { method: 'GET' }, '加载记忆库列表');
    if (!r.ok) return;
    const data = r.data;
    setCurrentLibrary(data.active || 'default');
    setLibraries(data.libraries || []);
  }, []);

  useEffect(() => {
    loadLibraryList().then(() => setLoading(false));
  }, [loadLibraryList]);

  const switchLibrary = useCallback(async (name: string) => {
    if (!name || name === currentLibrary) return;
    const r = await apiCall<{ ok: boolean; name: string; anchors_count: number; events_count: number }>(
      '/api/memory/library/load',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name }),
      },
      '切换记忆库',
    );
    if (!r.ok) return r.error;
    setCurrentLibrary(r.data.name);
    await loadLibraryList();
    return null;
  }, [currentLibrary, loadLibraryList]);

  const createLibrary = useCallback(async (name: string) => {
    const r = await apiCall<{ ok: boolean; name: string }>(
      '/api/memory/library/create',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name }),
      },
      '新建记忆库',
    );
    if (!r.ok) return r.error;
    setCurrentLibrary(r.data.name);
    await loadLibraryList();
    return null;
  }, [loadLibraryList]);

  const saveLibrary = useCallback(async (name: string) => {
    const r = await apiCall<{ ok: boolean; name: string }>(
      '/api/memory/library/save',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name }),
      },
      '保存记忆库',
    );
    if (!r.ok) return r.error;
    await loadLibraryList();
    return null;
  }, [loadLibraryList]);

  const deleteLibrary = useCallback(async (name: string) => {
    const r = await apiCall<{ ok: boolean }>(
      '/api/memory/library/delete',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name }),
      },
      '删除记忆库',
    );
    if (!r.ok) return r.error;
    await loadLibraryList();
    return null;
  }, [loadLibraryList]);

  return {
    currentLibrary,
    libraries,
    loading,
    loadLibraryList,
    switchLibrary,
    createLibrary,
    saveLibrary,
    deleteLibrary,
  };
}

