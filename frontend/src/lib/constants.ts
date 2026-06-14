// ── Constants ──

import type { SlashCommand } from '@/types';

export const SLASH_COMMANDS: SlashCommand[] = [
  { name: '/init', alias: '/i', desc: '构建认知地形', args: '[意图描述]' },
  { name: '/recall', alias: '/r', desc: '召回事件', args: '<文本>' },
  { name: '/status', alias: '/st', desc: '当前库状态', args: '' },
  { name: '/save', alias: '', desc: '持久化到磁盘', args: '' },
  { name: '/load', alias: '', desc: '从磁盘读取', args: '' },
  { name: '/help', alias: '/h', desc: '显示帮助', args: '' },
];

export const SKILL_DEFS = [
  { name: 'init_field', desc: '根据用户意图描述构建认知地形。' },
  { name: 'recall_memory', desc: '从记忆库中召回与查询文本相关的事件和锚点。' },
] as const;

export const EFFORT_OPTIONS = [
  { value: 'low', label: '低' },
  { value: 'medium', label: '中' },
  { value: 'high', label: '高' },
  { value: 'xhigh', label: '极高' },
] as const;

