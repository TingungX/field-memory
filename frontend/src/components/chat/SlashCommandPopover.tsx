'use client';

import type { SlashCommand } from '@/types';
import styles from './SlashCommandPopover.module.css';

interface SlashCommandPopoverProps {
  items: SlashCommand[];
  selectedIndex: number;
  onSelect: () => void;
}

export default function SlashCommandPopover({ items, selectedIndex, onSelect }: SlashCommandPopoverProps) {
  return (
    <div id="cmd-popover" className={styles.popover}>
      {items.map((c, i) => (
        <div
          key={c.name}
          className={`${styles.item} ${i === selectedIndex ? styles.selected : ''}`}
          data-index={i}
          onMouseDown={e => { e.preventDefault(); onSelect(); }}
        >
          <span className={styles.name}>{c.name}</span>
          {c.alias && <span className={styles.alias}>{c.alias}</span>}
          <span className={styles.desc}>{c.desc}</span>
        </div>
      ))}
    </div>
  );
}

