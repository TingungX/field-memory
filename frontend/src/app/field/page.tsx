'use client';

import { useEffect, useState, useCallback } from 'react';
import dynamic from 'next/dynamic';
import styles from './page.module.css';

const FieldCanvas = dynamic(() => import('@/components/field/FieldCanvas'), {
  ssr: false,
});

interface AnchorData {
  label: string;
  density: number;
  stiffness?: number;
  damping?: number;
  direction_n: number[];
}

export default function FieldPage() {
  const [anchors, setAnchors] = useState<AnchorData[]>([]);
  const [tension, setTension] = useState(0);
  const [stats, setStats] = useState({ anchors: 0, events: 0, seeds: 0 });
  const [showLabels, setShowLabels] = useState(true);
  const [showConnections, setShowConnections] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(async () => {
    try {
      const r = await fetch('/api/memory/status');
      const data = await r.json();
      if (data.anchors) {
        setAnchors(data.anchors.map((a: any) => ({
          label: a.label,
          density: a.density,
          direction_n: a.direction_n || [],
        })));
      }
      if (data.ecg?.field_tension !== undefined) {
        setTension(data.ecg.field_tension);
      }
      setStats({
        anchors: data.anchors_count || 0,
        events: data.events_count || 0,
        seeds: data.seeds_count || 0,
      });
      setError(null);
    } catch (e) {
      setError('无法连接到服务器。确保 ext-server 正在运行。');
    }
  }, []);

  useEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 3000);
    return () => clearInterval(interval);
  }, [fetchData]);

  // Handle keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'l' || e.key === 'L') setShowLabels(v => !v);
      if (e.key === 'c' || e.key === 'C') setShowConnections(v => !v);
      if (e.key === 'r' || e.key === 'R') fetchData();
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [fetchData]);

  return (
    <div className={styles.page}>
      {/* Top bar */}
      <header className={styles.topbar}>
        <div className={styles.brand}>
          <span className={styles.dot} />
          场可视化
          <span className={styles.sub}>field-memory · 3d</span>
        </div>
        <div className={styles.stats}>
          <div className={styles.statItem}>
            <span className={styles.statLbl}>anchors</span>
            <span className={styles.statVal}>{stats.anchors}</span>
          </div>
          <div className={styles.statItem}>
            <span className={styles.statLbl}>events</span>
            <span className={styles.statVal}>{stats.events}</span>
          </div>
          {tension > 0 && (
            <div className={styles.statItem}>
              <span className={styles.statLbl}>tension</span>
              <span className={styles.statVal}>{tension.toFixed(2)}</span>
            </div>
          )}
        </div>
        <div className={styles.controls}>
          <button
            className={`${styles.ctrlBtn} ${showLabels ? styles.active : ''}`}
            onClick={() => setShowLabels(v => !v)}
          >
            标签
          </button>
          <button
            className={`${styles.ctrlBtn} ${showConnections ? styles.active : ''}`}
            onClick={() => setShowConnections(v => !v)}
          >
            连线
          </button>
          <button className={styles.ctrlBtn} onClick={() => window.location.href = '/'}>
            返回
          </button>
        </div>
      </header>

      {/* 3D Canvas */}
      <div className={styles.canvasContainer}>
        {error ? (
          <div style={{
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            height: '100%', color: '#667', fontSize: 13,
          }}>
            {error}
          </div>
        ) : (
          <FieldCanvas anchors={anchors} tension={tension} />
        )}
      </div>

      {/* Bottom bar */}
      <div className={styles.bottomBar}>
        <span>鼠标拖动旋转 · 滚轮缩放 · 右键平移</span>
        <span>L: 标签 · C: 连线 · R: 刷新</span>
      </div>
    </div>
  );
}

