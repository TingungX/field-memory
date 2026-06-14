'use client';

import { useState, useCallback } from 'react';
import type { LibraryInfo } from '@/types';
import { apiCall } from '@/lib/api';

export function useLibraries() {
  const [libraries, setLibraries] = useState<LibraryInfo[]>([]);
  const [currentLibrary, setCurrentLibrary] = useState('default');

  const loadLibraryList = useCallback(async () => {
    const r = await apiCall<{ active: string; libraries: LibraryInfo[] }>('/api/memory/libraries', { method: 'GET' }, '加载记忆库列表');
    if (r.ok) {
      setCurrentLibrary(r.data.active || 'default');
      setLibraries(r.data.libraries || []);
    }
  }, []);

  const onLibraryChange = useCallback(async (name: string) => {
    if (!name || name === currentLibrary) return;
    const r = await apiCall<{ ok: boolean; name: string; anchors_count: number; events_count: number }>('/api/memory/library/load', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name }),
    }, '切换记忆库');
    if (r.ok && r.data.ok) {
      setCurrentLibrary(r.data.name);
      await loadLibraryList();
    }
  }, [currentLibrary, loadLibraryList]);

  const createLibrary = useCallback(async (name: string) => {
    const r = await apiCall<{ ok: boolean; name: string }>('/api/memory/library/create', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name }),
    }, '新建记忆库');
    if (r.ok && r.data.ok) {
      setCurrentLibrary(r.data.name);
      await loadLibraryList();
    }
  }, [loadLibraryList]);

  const deleteLibrary = useCallback(async (name: string) => {
    if (!confirm(`删除记忆库 "${name}" ?`)) return;
    const r = await apiCall<{ ok: boolean }>('/api/memory/library/delete', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name }),
    }, '删除记忆库');
    if (r.ok && r.data.ok) {
      await loadLibraryList();
    }
  }, [loadLibraryList]);

  const saveAsLibrary = useCallback(async (name: string) => {
    const r = await apiCall<{ ok: boolean; name: string }>('/api/memory/library/save', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name }),
    }, '保存记忆库');
    if (r.ok && r.data.ok) {
      await loadLibraryList();
    }
  }, [loadLibraryList]);

  return { libraries, currentLibrary, loadLibraryList, onLibraryChange, createLibrary, deleteLibrary, saveAsLibrary };
}

