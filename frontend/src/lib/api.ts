// ── API helper ──
// Migrated from app.js apiCall(). Unified discriminated result for all HTTP calls.

import type { AppConfig } from '@/types';

export type ApiResult<T = unknown> =
  | { ok: true; data: T }
  | { ok: false; error: string; kind: 'network' | 'http' | 'parse' | 'server'; status?: number };

export async function apiCall<T = unknown>(
  url: string,
  opts: RequestInit,
  label = '请求',
): Promise<ApiResult<T>> {
  const ctx = `${url} [${label}]`;
  try {
    const resp = await fetch(url, opts);
    if (!resp.ok) {
      let raw = '';
      try { raw = await resp.text(); } catch { /* ignore */ }
      let serverErr: string | null = null;
      try {
        const j = JSON.parse(raw);
        if (j && typeof j.error === 'string') serverErr = j.error;
      } catch { /* not JSON */ }
      const msg = `${label}: HTTP ${resp.status}${serverErr ? ' — ' + serverErr : resp.statusText ? ' ' + resp.statusText : ''}`;
      console.error('apiCall http error:', ctx, resp.status, raw);
      return { ok: false, error: msg, kind: 'http', status: resp.status };
    }
    let data: T;
    try {
      data = await resp.json() as T;
    } catch (e: unknown) {
      const pmsg = `${label}: 响应解析失败 — ${(e as Error).message}`;
      console.error('apiCall parse error:', ctx, e);
      return { ok: false, error: pmsg, kind: 'parse', status: resp.status };
    }
    // Some endpoints return { ok: false, error: "..." } with HTTP 200
    if (data && typeof data === 'object' && 'ok' in data && (data as { ok?: boolean }).ok === false) {
      const smsg = `${label}: ${(data as { error?: string }).error || '服务器拒绝'}`;
      console.error('apiCall server error:', ctx, data);
      return { ok: false, error: smsg, kind: 'server', status: resp.status };
    }
    return { ok: true, data };
  } catch (e: unknown) {
    const nmsg = `${label}: 网络错误 — ${(e as Error).message}`;
    console.error('apiCall network error:', ctx, e);
    return { ok: false, error: nmsg, kind: 'network' };
  }
}

// ── Config helpers ──

const CONFIG_KEY = 'fm-config';

const DEFAULT_CONFIG: AppConfig = {
  backendUrl: '',
  model: 'test-model-1',
  apiKey: 'sk-fm',
  reasoningEffort: 'medium',
};

export function loadConfig(): AppConfig {
  try {
    const raw = localStorage.getItem(CONFIG_KEY);
    if (!raw) return { ...DEFAULT_CONFIG };
    const data = JSON.parse(raw);
    const cfg = { ...DEFAULT_CONFIG, ...data };
    // Migration: old hardcoded localhost breaks mobile/LAN
    if (cfg.backendUrl === 'http://127.0.0.1:5100' || cfg.backendUrl === 'http://localhost:5100') {
      cfg.backendUrl = '';
    }
    return cfg;
  } catch {
    return { ...DEFAULT_CONFIG };
  }
}

export function saveConfig(cfg: AppConfig): void {
  try {
    localStorage.setItem(CONFIG_KEY, JSON.stringify(cfg));
  } catch { /* ignore */ }
}

export function resolveBackendUrl(cfg: AppConfig): string {
  const v = (cfg?.backendUrl || '').trim();
  if (v) return v.replace(/\/$/, '');
  // In production (static export served by Rust), same origin
  return typeof window !== 'undefined' ? window.location.origin : '';
}

