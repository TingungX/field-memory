'use client';

import styles from './Rail.module.css';

interface RailProps {
  panelStates: Record<string, boolean>;
  onTogglePanel: (id: string) => void;
  onCloseAllPanels: () => void;
  onSettingsClick: () => void;
}

const PANEL_BTNS = [
  {
    id: 'session-sidebar',
    side: 'left' as const,
    title: '会话列表',
    svg: '<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>',
  },
  {
    id: 'panel-core',
    side: 'left' as const,
    title: '引擎参数',
    svg: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>',
  },
  {
    id: '__spacer',
    side: 'left' as const,
    title: '',
    svg: '',
  },
  {
    id: '__settings',
    side: 'left' as const,
    title: '设置',
    svg: '<rect x="3" y="4" width="18" height="6" rx="1.5"/><rect x="3" y="14" width="18" height="6" rx="1.5"/><line x1="7" y1="7" x2="7.01" y2="7"/><line x1="7" y1="17" x2="7.01" y2="17"/>',
  },
  {
    id: 'panel-mem',
    side: 'right' as const,
    title: '记忆面板',
    svg: '<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/><path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/>',
  },
];

export default function Rail({ panelStates, onTogglePanel, onCloseAllPanels, onSettingsClick }: RailProps) {
  const handleClick = (id: string) => {
    if (id === '__settings') {
      onSettingsClick();
    } else if (id === '__close-all') {
      onCloseAllPanels();
    } else {
      onTogglePanel(id);
    }
  };

  return (
    <nav id="rail" className={styles.rail}>
      {PANEL_BTNS.map(btn => {
        if (btn.id === '__spacer') {
          return <div key={btn.id} className={styles.spacer} />;
        }
        if (btn.id === '__settings') {
          return (
            <button
              key={btn.id}
              className={styles.btn}
              title={btn.title}
              onClick={() => onSettingsClick()}
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"
                dangerouslySetInnerHTML={{ __html: btn.svg }}
              />
            </button>
          );
        }
        return (
          <button
            key={btn.id}
            className={`${styles.btn}${panelStates[btn.id] ? ' ' + styles.active : ''}`}
            title={btn.title}
            data-panel={btn.id}
            onClick={() => onTogglePanel(btn.id)}
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
              strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"
              dangerouslySetInnerHTML={{ __html: btn.svg }}
            />
          </button>
        );
      })}
    </nav>
  );
}

