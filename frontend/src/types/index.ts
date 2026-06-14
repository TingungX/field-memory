// ── field-memory frontend types ──

export interface Session {
  id: string;
  title: string;
  active?: boolean;
  messages: Message[];
  created_at?: string;
  updated_at?: string;
}

export interface Message {
  role: 'user' | 'assistant' | 'system_note';
  content: string;
  time?: string;
  memoryCtx?: MemoryCtx | null;
  reasoning?: string | null;
  toolCalls?: ToolCall[] | null;
  thinkingChain?: ThinkingChainStep[] | null;
}

export interface MemoryCtx {
  tool_invoked?: boolean;
  system_prompt_line?: string;
  associations?: Association[];
  recalled_events?: RecalledEvent[];
}

export interface Association {
  label: string;
  impact: number;
}

export interface RecalledEvent {
  anchor: string;
  text: string;
}

export interface ToolCall {
  id?: string;
  type?: string;
  function: {
    name: string;
    arguments: string;
  };
}

export interface ThinkingChainStep {
  type: 'reasoning' | 'tool_call';
  content: string | ToolCall[];
}

export interface Anchor {
  label: string;
  density: number;
  direction_xy: [number, number];
  direction_n: number[];
  stiffness?: number;
  damping?: number;
}

export interface EcgBrief {
  field_tension?: number;
}

export interface MemoryStatus {
  ok: boolean;
  anchors_count: number;
  events_count: number;
  traces_count: number;
  seeds_count: number;
  anchors?: Anchor[];
  ecg?: EcgBrief;
  seeds?: string[];
  recent_activity?: Activity[];
  recent_events?: { text: string; timestamp: string }[];
}

export interface Activity {
  kind: string;
  timestamp?: string;
  detail?: string;
}

export interface LibraryInfo {
  name: string;
  anchors_count: number;
  events_count: number;
}

export interface LibraryList {
  active: string;
  libraries: LibraryInfo[];
}

export interface AppConfig {
  backendUrl: string;
  model: string;
  apiKey: string;
  reasoningEffort: string;
}

export type TabName = 'conversation' | 'system' | 'skills' | 'memory';

export interface SlashCommand {
  name: string;
  alias: string;
  desc: string;
  args: string;
}

