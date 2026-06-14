'use client';

import styles from './CorePanel.module.css';

interface CorePanelProps {
  collapsed: boolean;
}

export default function CorePanel({ collapsed }: CorePanelProps) {
  return (
    <div
      id="panel-core"
      className={`${styles.panel} ${collapsed ? styles.collapsed : ''}`}
    >
      <div className={styles.content}>
        <h3>引擎参数</h3>
        <p className={styles.hint}>
          引擎参数面板将在后续版本中添加完整内容。<br />
          当前引擎统计信息见右侧记忆面板。
        </p>
      </div>
    </div>
  );
}

