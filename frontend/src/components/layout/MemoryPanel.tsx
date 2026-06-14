'use client';

import { useRef, useEffect } from 'react';
import type { MemoryStatus } from '@/types';
import styles from './MemoryPanel.module.css';

interface MemoryPanelProps {
  collapsed: boolean;
  currentLibrary: string;
  status: MemoryStatus | null;
  onSave: () => void;
  onLoad: () => void;
}

export default function MemoryPanel({ collapsed, currentLibrary, status, onSave, onLoad }: MemoryPanelProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  // Render 2D field visualization
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || collapsed || !status?.anchors) return;

    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const anchors = status.anchors;
    const tension = status.ecg?.field_tension || 0;

    // Background
    ctx.fillStyle = '#0e1116';
    ctx.fillRect(0, 0, w, h);

    // Radial gradient
    const tnorm = Math.min(tension / 30, 1);
    const grad = ctx.createRadialGradient(w / 2, h / 2, 0, w / 2, h / 2, Math.min(w, h) * 0.55);
    grad.addColorStop(0, `rgba(74, 111, 165, ${0.1 + tnorm * 0.4})`);
    grad.addColorStop(1, 'rgba(14, 17, 22, 0)');
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, w, h);

    // Grid
    ctx.strokeStyle = 'rgba(255,255,255,0.04)';
    ctx.lineWidth = 1;
    for (let x = 0; x < w; x += 30) {
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, h);
      ctx.stroke();
    }
    for (let y = 0; y < h; y += 30) {
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(w, y);
      ctx.stroke();
    }

    // Map anchors to 2D positions
    const cx = w / 2, cy = h / 2, scale = Math.min(w, h) * 0.36;
    const positions = anchors.map(a => ({
      x: cx + (a.direction_xy?.[0] || 0) * scale,
      y: cy - (a.direction_xy?.[1] || 0) * scale,
      a,
    }));

    // Connection lines
    for (let i = 0; i < positions.length; i++) {
      for (let k = i + 1; k < positions.length; k++) {
        const a = positions[i].a, b = positions[k].a;
        const dx = a.direction_xy?.[0] || 0;
        const dy = a.direction_xy?.[1] || 0;
        const bx = b.direction_xy?.[0] || 0;
        const by = b.direction_xy?.[1] || 0;
        const dot = dx * bx + dy * by;
        if (dot > 0.5) {
          const alpha = (dot - 0.5) * 0.5;
          ctx.strokeStyle = `rgba(74, 111, 165, ${alpha})`;
          ctx.lineWidth = dot * 1.5;
          ctx.beginPath();
          ctx.moveTo(positions[i].x, positions[i].y);
          ctx.lineTo(positions[k].x, positions[k].y);
          ctx.stroke();
        }
      }
    }

    // Anchor dots
    const LAYER_COLORS: Record<string, string> = {
      L1: '#ff6b6b', L2: '#ffa94d', L3: '#ffd43b', L4: '#74c0fc',
    };
    for (const p of positions) {
      const d = p.a.density;
      const layer = d > 15 ? 'L1' : d > 8 ? 'L2' : d > 3 ? 'L3' : 'L4';
      const color = LAYER_COLORS[layer] || '#74c0fc';
      const size = Math.max(2, Math.min(8, 2 + d * 0.15));
      ctx.beginPath();
      ctx.arc(p.x, p.y, size, 0, Math.PI * 2);
      ctx.fillStyle = color;
      ctx.fill();
      ctx.strokeStyle = 'rgba(255,255,255,0.3)';
      ctx.lineWidth = 0.5;
      ctx.stroke();
    }
  }, [status, collapsed]);

  const sortedAnchors = status?.anchors
    ? [...status.anchors].sort((a, b) => b.density - a.density)
    : [];
  const maxD = sortedAnchors[0]?.density || 1;

  return (
    <div
      id="panel-mem"
      className={`${styles.panel} ${collapsed ? styles.collapsed : ''}`}
    >
      <div className={styles.content}>
        {/* Stats */}
        <div className={styles.row}>
          <span className={styles.l}>锚点</span>
          <span className={styles.v}>{status?.anchors_count ?? '—'}</span>
        </div>
        <div className={styles.row}>
          <span className={styles.l}>事件</span>
          <span className={styles.v}>{status?.events_count ?? '—'}</span>
        </div>
        <div className={styles.row}>
          <span className={styles.l}>痕迹</span>
          <span className={styles.v}>{status?.traces_count ?? '—'}</span>
        </div>
        {status?.ecg?.field_tension !== undefined && status.ecg.field_tension !== null && (
          <div className={styles.row}>
            <span className={styles.l}>场张力</span>
            <span className={styles.v}>{status.ecg.field_tension.toFixed(4)}</span>
          </div>
        )}

        {/* Library selector */}
        <h3>记忆库</h3>
        <div className={styles.libRow}>
          <span className={styles.libName}>{currentLibrary}</span>
          <button className={styles.smallBtn} onClick={onSave}>保存</button>
          <button className={styles.smallBtn} onClick={onLoad}>读取</button>
        </div>

        {/* Anchor density bars */}
        {sortedAnchors.length > 0 && (
          <>
            <h3>锚点密度</h3>
            {sortedAnchors.slice(0, 12).map((a, i) => {
              const pct = (a.density / maxD * 100).toFixed(0);
              return (
                <div key={i} className={styles.anchor}>
                  <span className={styles.lbl} title={a.label}>{a.label}</span>
                  <div className={styles.bar}>
                    <div className={styles.fill} style={{ width: `${pct}%` }} />
                  </div>
                  <span className={styles.val}>{a.density}</span>
                </div>
              );
            })}
          </>
        )}

        {/* Field visualization */}
        <h3>场可视化</h3>
        <div className={styles.vizWrap}>
          <canvas
            ref={canvasRef}
            className={styles.vizCanvas}
          />
          <div className={styles.vizOverlay}>
            {status?.anchors_count ?? 0} 锚点
            {status?.ecg?.field_tension !== undefined && status.ecg.field_tension !== null
              ? ` · 张力 ${status.ecg.field_tension.toFixed(2)}`
              : ''}
          </div>
        </div>

        {/* 3D viz link */}
        <div className={styles.actions}>
          <button className={styles.smallBtn} onClick={() => window.open('/field', '_blank')}>
            3D 可视化
          </button>
        </div>
      </div>
    </div>
  );
}

