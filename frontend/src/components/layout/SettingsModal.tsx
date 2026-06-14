'use client';

import { useState, useEffect, useCallback } from 'react';
import type { AppConfig } from '@/types';
import { EFFORT_OPTIONS } from '@/lib/constants';

interface SettingsModalProps {
  config: AppConfig;
  onUpdateConfig: (partial: Partial<AppConfig>) => void;
}

export default function SettingsModal({ config, onUpdateConfig }: SettingsModalProps) {
  const [open, setOpen] = useState(false);
  const [local, setLocal] = useState(config);

  useEffect(() => {
    setLocal(config);
  }, [config]);

  // Listen for the custom open event
  useEffect(() => {
    const handler = () => setOpen(true);
    window.addEventListener('fm-open-settings', handler);
    return () => window.removeEventListener('fm-open-settings', handler);
  }, []);

  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [open]);

  const handleSave = useCallback(() => {
    onUpdateConfig(local);
    setOpen(false);
  }, [local, onUpdateConfig]);

  if (!open) return null;

  return (
    <div className="modal-backdrop" onClick={() => setOpen(false)}>
      <div
        className="modal-content"
        onClick={e => e.stopPropagation()}
        style={{ maxHeight: '80vh', overflowY: 'auto' }}
      >
        <div className="modal-header">
          <h2>后端 LLM 接入</h2>
          <button className="modal-close" onClick={() => setOpen(false)} aria-label="关闭">×</button>
        </div>
        <div className="modal-body">
          <div className="form-row">
            <label htmlFor="settings-backend-url">代理地址</label>
            <input
              id="settings-backend-url"
              type="text"
              placeholder="(留空 = 当前页面地址)"
              value={local.backendUrl}
              onChange={e => setLocal({ ...local, backendUrl: e.target.value })}
            />
            <span className="form-hint">前端请求的目标地址（留空 = 同源）</span>
          </div>
          <div className="form-row">
            <label htmlFor="settings-model">模型</label>
            <input
              id="settings-model"
              type="text"
              placeholder="test-model-1"
              value={local.model}
              onChange={e => setLocal({ ...local, model: e.target.value })}
            />
          </div>
          <div className="form-row">
            <label htmlFor="settings-api-key">API Key</label>
            <input
              id="settings-api-key"
              type="password"
              placeholder="sk-fm"
              value={local.apiKey}
              onChange={e => setLocal({ ...local, apiKey: e.target.value })}
            />
            <span className="form-hint">ext-server 用此 key 调上游 LLM</span>
          </div>
          <div className="form-row">
            <label htmlFor="settings-reasoning-effort">思考强度</label>
            <select
              id="settings-reasoning-effort"
              value={local.reasoningEffort}
              onChange={e => setLocal({ ...local, reasoningEffort: e.target.value })}
            >
              {EFFORT_OPTIONS.map(opt => (
                <option key={opt.value} value={opt.value}>{opt.label}</option>
              ))}
            </select>
          </div>
        </div>
        <div className="modal-footer">
          <button className="btn" onClick={() => setOpen(false)}>取消</button>
          <button className="btn btn-primary" onClick={handleSave}>保存</button>
        </div>
      </div>
    </div>
  );
}

