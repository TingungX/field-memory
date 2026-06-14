'use client';

import { useState, useCallback, useEffect } from 'react';
import type { AppConfig } from '@/types';
import { loadConfig, saveConfig } from '@/lib/api';

export function useConfig() {
  const [config, setConfig] = useState<AppConfig>(() => {
    // SSR guard
    if (typeof window === 'undefined') {
      return { backendUrl: '', model: 'test-model-1', apiKey: 'sk-fm', reasoningEffort: 'medium' };
    }
    return loadConfig();
  });

  const updateConfig = useCallback((partial: Partial<AppConfig>) => {
    setConfig(prev => {
      const next = { ...prev, ...partial };
      saveConfig(next);
      return next;
    });
  }, []);

  // Sync across tabs
  useEffect(() => {
    const handler = (e: StorageEvent) => {
      if (e.key === 'fm-config') {
        setConfig(loadConfig());
      }
    };
    window.addEventListener('storage', handler);
    return () => window.removeEventListener('storage', handler);
  }, []);

  return { config, updateConfig };
}

