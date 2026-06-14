'use client';

import { useState } from 'react';
import styles from './CollapsePanel.module.css';

interface CollapsePanelProps {
  label: string;
  children: React.ReactNode;
  defaultOpen?: boolean;
  extraClass?: string;
}

export default function CollapsePanel({ label, children, defaultOpen = false, extraClass }: CollapsePanelProps) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div className={`${styles.wrap} ${extraClass || ''}`}>
      <button className={`${styles.toggle} ${open ? styles.toggleOpen : ''}`} onClick={() => setOpen(!open)}>
        <svg className={styles.chevron} viewBox="0 0 8 8" fill="currentColor" style={{ transform: open ? 'rotate(90deg)' : 'none' }}>
          <path d="M2 1 L6 4 L2 7 Z" />
        </svg>
        <span>{label}</span>
      </button>
      {open && <div className={styles.content}>{children}</div>}
    </div>
  );
}

