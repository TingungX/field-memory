'use client';

import { useState, useCallback, useEffect } from 'react';

interface PanelState {
  [panelId: string]: boolean; // true = expanded, false = collapsed
}

function getDefault(id: string): boolean {
  if (typeof window === 'undefined') return id === 'session-sidebar';
  const stored = sessionStorage.getItem(`panel-${id}`);
  if (stored !== null) return stored !== 'collapsed';
  if (window.innerWidth <= 768) return false;
  return id === 'session-sidebar';
}

export function usePanelState(panelIds: string[]) {
  const [states, setStates] = useState<PanelState>(() => {
    const initial: PanelState = {};
    for (const id of panelIds) {
      initial[id] = getDefault(id);
    }
    return initial;
  });

  const [isMobile, setIsMobile] = useState(false);

  useEffect(() => {
    const check = () => setIsMobile(window.innerWidth <= 768);
    check();
    window.addEventListener('resize', check);
    return () => window.removeEventListener('resize', check);
  }, []);

  const togglePanel = useCallback((id: string) => {
    setStates(prev => {
      const current = !prev[id];
      sessionStorage.setItem(`panel-${id}`, current ? 'expanded' : 'collapsed');

      // On mobile, close other panels when opening one
      if (current && window.innerWidth <= 768) {
        const next: PanelState = { ...prev, [id]: true };
        for (const pid of panelIds) {
          if (pid !== id) {
            next[pid] = false;
            sessionStorage.setItem(`panel-${pid}`, 'collapsed');
          }
        }
        return next;
      }

      return { ...prev, [id]: current };
    });
  }, [panelIds]);

  const closeAll = useCallback(() => {
    setStates(prev => {
      const next: PanelState = {};
      for (const id of panelIds) {
        next[id] = false;
        sessionStorage.setItem(`panel-${id}`, 'collapsed');
      }
      return next;
    });
  }, [panelIds]);

  const anyOpen = Object.values(states).some(v => v);

  return { states, togglePanel, closeAll, anyOpen, isMobile };
}

