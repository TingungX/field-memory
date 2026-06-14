// ── State ──
// `sessions` is a client-side cache of the server's source of truth.
// Every mutation goes through a server endpoint immediately, then the local
// cache is updated optimistically. The server's data is authoritative — a
// refresh (e.g. on visibility change) pulls the latest from the server.
var sessions = [];
// active_session_id is per-device, persisted in localStorage so it survives
// reload. The server also tracks it, but each device decides its own.
var activeSessionId = localStorage.getItem('fm-active-id') || null;
var isStreaming = false;
var pollTimer = null;
var currentLibrary = 'default';
var knownLibraries = []; // cached from /api/memory/libraries
var fieldParticles = [];
var lastAnchorCount = 0;
var fieldTension = 0;
var anchors = [];

// ── Unified API helper ──
//
// Wraps fetch + response handling into a single discriminated result so callers
// never need to duplicate try/catch/HTTP/parse logic.
//
// Returns:
//   { ok: true,  data }                  on success
//   { ok: false, error, kind, status? }  on failure
//
//   kind ∈ 'network' | 'http' | 'parse' | 'server'
//   - 'network': fetch itself rejected (server down, DNS, abort, …)
//   - 'http':    server returned 4xx/5xx (status + body included)
//   - 'parse':   response body wasn't valid JSON
//   - 'server':  HTTP 200 but data.ok === false (backend logical error)
//
// `label` is the user-facing operation name (e.g. '切换记忆库'); it's prepended
// to `error` so the message is self-describing. Also logs full detail to
// console.error with a `apiCall` prefix for debugging.
async function apiCall(url, opts, label) {
  label = label || '请求';
  var ctx = url + ' [' + label + ']';
  try {
    var resp = await fetch(url, opts);
    if (!resp.ok) {
      var raw = '';
      try { raw = await resp.text(); } catch(e2) { /* ignore */ }
      var serverErr = null;
      try { var j = JSON.parse(raw); if (j && typeof j.error === 'string') serverErr = j.error; } catch(e2) { /* not JSON */ }
      var msg = label + ': HTTP ' + resp.status +
        (serverErr ? ' — ' + serverErr : (resp.statusText ? ' ' + resp.statusText : ''));
      console.error('apiCall http error:', ctx, resp.status, raw);
      return { ok: false, error: msg, kind: 'http', status: resp.status, rawBody: raw };
    }
    var data;
    try { data = await resp.json(); }
    catch(e) {
      var pmsg = label + ': 响应解析失败 — ' + e.message;
      console.error('apiCall parse error:', ctx, e);
      return { ok: false, error: pmsg, kind: 'parse', status: resp.status };
    }
    if (data && data.ok === false) {
      var smsg = label + ': ' + (data.error || '服务器拒绝');
      console.error('apiCall server error:', ctx, data);
      return { ok: false, error: smsg, kind: 'server', status: resp.status };
    }
    return { ok: true, data: data };
  } catch(e) {
var nmsg = label + ': 网络错误 — ' + e.message;
    console.error('apiCall network error:', ctx, e);
    return { ok: false, error: nmsg, kind: 'network' };
  }
}

// ── Inline SVG icon helpers ──
//
// All chrome that would otherwise be text or a unicode glyph goes through
// here so we can stay consistent (sizing, stroke weight, currentColor).
// Each returns an HTML string for direct .innerHTML insertion.
var ICON_CHEVRON = '<svg viewBox="0 0 8 8" fill="currentColor"><path d="M2 1 L6 4 L2 7 Z"/></svg>';
var ICON_PENCIL = '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M11 2 L14 5 L5 14 L2 14 L2 11 Z"/><path d="M10 3 L13 6"/></svg>';
var ICON_INFO = '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="6.5"/><line x1="8" y1="7" x2="8" y2="11.5"/><circle cx="8" cy="4.7" r="0.6" fill="currentColor" stroke="none"/></svg>';

// ── Session persistence (server is source of truth) ──
//
// The server holds the canonical sessions list. The client keeps a cache in
// `sessions` (initialized from the server on load) and pushes every mutation
// through a dedicated endpoint immediately. After mutation the local cache is
// updated optimistically, so the UI stays snappy.
//
// Why no more polling: every visible-state read pulls from the server, and
// every write lands before returning. The only thing the client can't predict
// is what *other devices* are doing — and we accept that "another device
// changed something while you weren't looking" is fine to ignore for a
// personal LAN tool. If you want to see other-device edits, hit the ↻ button.

// Refresh the client cache from the server. Used at startup and on demand.
async function refreshSessions() {
  var r = await apiCall('/api/sessions', { method: 'GET' }, '刷新会话');
  if (!r.ok) {
    console.warn('refreshSessions:', r.error);
    return;
  }
  var data = r.data;
  if (!data || !data.sessions) return;
  sessions = data.sessions;
  // active_session_id is per-device (localStorage). If the remembered id is
  // gone (deleted on another device), fall back to the first session so the
  // user lands somewhere instead of on an empty state.
  if (activeSessionId && !sessions.find(function(s) { return s.id === activeSessionId; })) {
    activeSessionId = sessions.length ? sessions[0].id : null;
    if (activeSessionId) localStorage.setItem('fm-active-id', activeSessionId);
  }
  if (!activeSessionId && sessions.length) {
    activeSessionId = sessions[0].id;
    localStorage.setItem('fm-active-id', activeSessionId);
  }
  renderSessionList();
  if (typeof getActiveSession === 'function' && getActiveSession()) renderMessages();
}


// Manual trigger for the ↻ button — pull the latest from the server.
async function manualSessionSync() {
  var btn = document.querySelector('.sb-sync');
  if (btn) btn.classList.add('syncing');
  await refreshSessions();
  if (btn) setTimeout(function() { btn.classList.remove('syncing'); }, 400);
}

// On returning to the foreground, immediately re-pull — covers the case
// where another device or a long background freeze left the cache stale.
document.addEventListener('visibilitychange', function() {
  if (!document.hidden) refreshSessions();
});

// ── Panel collapse state (persisted in sessionStorage) ──
// Desktop: only session-sidebar open by default; mobile: all collapsed.
// sessionStorage overrides take priority (user explicitly toggled).
var _isMobile = window.innerWidth <= 768;
function _panelDefault(id) {
  var stored = sessionStorage.getItem('panel-' + id);
  if (stored !== null) return stored !== 'collapsed';
  if (_isMobile) return false;
  return id === 'session-sidebar';
}
var panelStates = {
  'session-sidebar': _panelDefault('session-sidebar'),
  'panel-core': _panelDefault('panel-core'),
  'panel-mem': _panelDefault('panel-mem'),
};

// ── Session management ──
function getActiveSession() {
  return sessions.find(function(s) { return s.id === activeSessionId; });
}

async function createSession() {
  var r = await apiCall('/api/sessions', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ title: '新会话' }),
  }, '创建会话');
  if (!r.ok) {
    if (typeof addMessageToSession === 'function') addSystemNote(r.error);
    return;
  }
  // Server returns the new session; use it as authoritative.
  sessions.push(r.data.session);
  activeSessionId = r.data.session.id;
  localStorage.setItem('fm-active-id', activeSessionId);
  renderSessionList();
  renderMessages();
}

async function switchSession(id) {
  activeSessionId = id;
  localStorage.setItem('fm-active-id', id);
  renderSessionList();
  renderMessages();
  // Fire-and-forget the active marker; the local view is already correct.
  apiCall('/api/sessions/' + encodeURIComponent(id), {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ active: true }),
  }, '切换会话').then(function(r) {
    if (!r.ok) console.warn('switchSession:', r.error);
  });
}

async function deleteSession(id) {
  // Optimistic local update first so the UI feels instant.
  sessions = sessions.filter(function(s) { return s.id !== id; });
  if (activeSessionId === id) {
    activeSessionId = sessions.length ? sessions[0].id : null;
    if (activeSessionId) localStorage.setItem('fm-active-id', activeSessionId);
    else localStorage.removeItem('fm-active-id');
  }
  renderSessionList();
  renderMessages();
  // Then tell the server.
  apiCall('/api/sessions/' + encodeURIComponent(id), {
    method: 'DELETE',
  }, '删除会话').then(function(r) {
    if (!r.ok) console.warn('deleteSession:', r.error);
  });
}

function renderSessionList() {
  var el = document.getElementById('session-list');
  if (!el) return;
  var html = '';
  for (var i = 0; i < sessions.length; i++) {
    var s = sessions[i];
    var cls = s.id === activeSessionId ? ' active' : '';
    html += '<div class="sb-item' + cls + '" data-id="' + s.id + '">';
    html += '<span class="sb-label" onclick="switchSession(\'' + s.id + '\')">' + esc(s.title) + '</span>';
    html += '<button class="sb-del" onclick="event.stopPropagation();deleteSession(\'' + s.id + '\')">&times;</button>';
    html += '</div>';
  }
  el.innerHTML = html;
}

function renderMessages() {
  var el = document.getElementById('messages');
  var s = getActiveSession();
  if (!el) return;
  if (!s || s.messages.length === 0) {
    el.innerHTML = '<div class="welcome"><strong>field-memory</strong><br>输入消息开始对话<br>输入 / 查看命令</div>';
    return;
  }
  el.innerHTML = '';
  for (var i = 0; i < s.messages.length; i++) {
    var m = s.messages[i];
    var div = document.createElement('div');
    div.className = 'message ' + m.role;
    // system_note reuses the same DOM shape as appendChatBubble produces
    // (info icon + note-text span), so it stays visually consistent.
    if (m.role === 'system_note') {
      div.innerHTML = '<div class="content">' +
        '<svg class="note-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">' +
          '<circle cx="8" cy="8" r="6.5"/>' +
          '<line x1="8" y1="7" x2="8" y2="11.5"/>' +
          '<circle cx="8" cy="4.7" r="0.6" fill="currentColor" stroke="none"/>' +
        '</svg>' +
        '<span class="note-text">' + esc(m.content) + '</span>' +
      '</div>';
      el.appendChild(div);
      continue;
    }
    var actionsHtml = '';
    if (m.role === 'user') {
      actionsHtml = '<div class="message-actions">' +
        '<button class="message-action-btn" title="编辑并重新发送" onclick="beginEditMessage(this, ' + i + ')">' +
          ICON_PENCIL +
        '</button>' +
      '</div>';
    }
    div.innerHTML = '<div class="content">' + esc(m.content) + '</div>' +
      actionsHtml +
      '<div class="meta">' + (m.role === 'user' ? '你' : 'FM') + ' &middot; ' + (m.time || '') + '</div>';
    if (m.role === 'assistant' && m.memoryCtx) {
      var toggle = document.createElement('div');
      toggle.className = 'mem-ctx-toggle';
      toggle.textContent = m.memoryCtx.tool_invoked ? '已调用 tool' : '记忆注入详情';
      var detail = document.createElement('div');
      detail.className = 'mem-ctx-detail';
      var h = '';
      if (m.memoryCtx.tool_invoked) h += '<div style="color:var(--accent);font-weight:500">tool 已被模型调用</div>';
      if (m.memoryCtx.system_prompt_line) h += '<div style="margin-top:4px"><span class="label">注入：</span>' + esc(m.memoryCtx.system_prompt_line) + '</div>';
      if (m.memoryCtx.associations && m.memoryCtx.associations.length) {
        h += '<div style="margin-top:4px"><span class="label">关联：</span></div>';
        for (var ai = 0; ai < m.memoryCtx.associations.length; ai++) {
          h += '<div class="item">  ' + esc(m.memoryCtx.associations[ai].label) + ' (' + m.memoryCtx.associations[ai].impact + ')</div>';
        }
      }
      if (m.memoryCtx.recalled_events && m.memoryCtx.recalled_events.length) {
        h += '<div style="margin-top:4px"><span class="label">召回事件：</span></div>';
        for (var ei = 0; ei < m.memoryCtx.recalled_events.length; ei++) {
          h += '<div class="item">  [' + esc(m.memoryCtx.recalled_events[ei].anchor) + '] ' + esc(m.memoryCtx.recalled_events[ei].text) + '</div>';
        }
      }
      detail.innerHTML = h;
      (function(t, d) {
        t.addEventListener('click', function() { t.classList.toggle('open'); d.classList.toggle('open'); });
      })(toggle, detail);
div.appendChild(toggle);
      div.appendChild(detail);
    }
    if (m.role === 'assistant') {
      var chain = m.thinkingChain;
      // Fallback: if thinkingChain is missing but legacy reasoning/toolCalls exist,
      // reconstruct from them (covers messages saved before this feature was added).
      if ((!chain || chain.length === 0) && (m.reasoning || m.toolCalls)) {
        chain = legacyToThinkingChain(m.reasoning, m.toolCalls);
      }
      renderThinkingChain(div, chain);
    }
    el.appendChild(div);
  }
el.scrollTop = el.scrollHeight;
}

// ── Thinking chain (vertical timeline) ──
//
// Renders the reasoning / tool-call / output sequence as a compact vertical
// timeline above the main content.  Each node is a small dot-and-label row;
// click to expand the detail.  The chain is derived from the SSE stream order:
// reasoning deltas accumulate into one "思考" node until a tool_call arrives,
// which closes the current reasoning node and creates a "recall 调用" node.
// After all intermediate steps, the main content is the "输出" node.
//
// The thinkingChain is stored as an array of {type, content} on the message
// object.  type ∈ 'reasoning' | 'tool_call'.  The final "输出" node is implicit
// (it's the main .content div) and is not stored in the chain.
function renderThinkingChain(div, thinkingChain) {
  if (!div) return;
  // Remove any pre-existing chain we own (tagged via data-fm-chain).
  var prev = div.querySelectorAll('[data-fm-chain]');
  for (var p = 0; p < prev.length; p++) prev[p].remove();

  if (!thinkingChain || !Array.isArray(thinkingChain) || thinkingChain.length === 0) return;

  var chainWrap = document.createElement('div');
  chainWrap.setAttribute('data-fm-chain', '1');
  chainWrap.className = 'fm-thinking-chain';

  for (var i = 0; i < thinkingChain.length; i++) {
    var step = thinkingChain[i];
    var isLast = (i === thinkingChain.length - 1);
    var nodeEl = document.createElement('div');
    nodeEl.className = 'chain-node';

    var dotEl = document.createElement('div');
    dotEl.className = 'chain-dot';
    if (step.type === 'tool_call') dotEl.classList.add('dot-tool');
    else dotEl.classList.add('dot-reasoning');

    var labelEl = document.createElement('button');
    labelEl.type = 'button';
    labelEl.className = 'chain-label';
    if (step.type === 'tool_call') {
      var tcList = step.content;
      var fnNames = [];
      for (var k = 0; k < tcList.length; k++) {
        fnNames.push((tcList[k].function && tcList[k].function.name) || '(unnamed)');
      }
      labelEl.innerHTML = ICON_CHEVRON + '<span>工具调用 · ' + fnNames.join(', ') + '</span>';
    } else {
      var rLen = (typeof step.content === 'string') ? step.content.length : 0;
      labelEl.innerHTML = ICON_CHEVRON + '<span>思考 · ' + rLen + ' 字</span>';
    }

    var detailEl = document.createElement('div');
    detailEl.className = 'chain-detail';
    if (step.type === 'tool_call') {
      detailEl.appendChild(renderToolCallsBody(step.content));
    } else {
      detailEl.textContent = step.content;
    }

    // Wire toggle
    (function(lbl, det) {
      lbl.addEventListener('click', function() {
        lbl.classList.toggle('open');
        det.classList.toggle('open');
      });
    })(labelEl, detailEl);

    nodeEl.appendChild(dotEl);
    nodeEl.appendChild(labelEl);
    nodeEl.appendChild(detailEl);

    // Connector line (not on last node)
    if (!isLast) {
      var connEl = document.createElement('div');
      connEl.className = 'chain-connector';
      nodeEl.appendChild(connEl);
    }

    chainWrap.appendChild(nodeEl);
  }

  // Insert chain before .content div
  var contentEl = div.querySelector('.content');
  if (contentEl) {
    div.insertBefore(chainWrap, contentEl);
  } else {
    div.appendChild(chainWrap);
  }
}

// Build a thinkingChain array from the SSE stream events.
// Called incrementally during streaming and at stream end.
// The chain captures the alternation: reasoning segments → tool calls → reasoning → …
function buildThinkingChain(chain, reasoningDelta, newToolCalls) {
  // If we got a reasoning delta, append to the last reasoning node
  // or create a new one if the last node was a tool_call.
  if (reasoningDelta) {
    if (chain.length > 0 && chain[chain.length - 1].type === 'reasoning') {
      chain[chain.length - 1].content += reasoningDelta;
    } else {
      chain.push({ type: 'reasoning', content: reasoningDelta });
    }
  }
  // If we got new tool_calls, push a tool_call node.
  if (newToolCalls && Array.isArray(newToolCalls) && newToolCalls.length) {
    chain.push({ type: 'tool_call', content: newToolCalls });
  }
  return chain;
}

// Backwards compatibility: convert legacy (reasoning string, toolCalls array)
// into a thinkingChain array.  If a tool_call appears between reasoning text,
// we can't reconstruct the alternation precisely — the server interleaves them
// in real-time, but the persisted form loses the interleaving order.  The best
// we can do is: reasoning (one node) → tool_call (one node).
function legacyToThinkingChain(reasoning, toolCalls) {
  var chain = [];
  if (reasoning && typeof reasoning === 'string' && reasoning.trim()) {
    chain.push({ type: 'reasoning', content: reasoning });
  }
  if (toolCalls && Array.isArray(toolCalls) && toolCalls.length) {
    chain.push({ type: 'tool_call', content: toolCalls });
  }
  return chain;
}

function makeCollapsePanel(label, bodyContent, extraClass) {
  // bodyContent may be a string or an HTMLElement; the helper hides this.
  var wrap = document.createElement('div');
  wrap.setAttribute('data-fm-collapse', '1');
  if (extraClass) wrap.className = extraClass;
  var toggle = document.createElement('button');
  toggle.type = 'button';
  toggle.className = 'collapse-toggle';
  toggle.innerHTML = ICON_CHEVRON + '<span>' + esc(label) + '</span>';
  var content = document.createElement('div');
  content.className = 'collapse-content';
  if (typeof bodyContent === 'string') {
    content.textContent = bodyContent;
  } else if (bodyContent) {
    content.appendChild(bodyContent);
  }
  toggle.addEventListener('click', function() {
    toggle.classList.toggle('open');
    content.classList.toggle('open');
  });
  wrap.appendChild(toggle);
  wrap.appendChild(content);
  return wrap;
}

function renderToolCallsBody(toolCalls) {
  var box = document.createElement('div');
  for (var i = 0; i < toolCalls.length; i++) {
    var tc = toolCalls[i] || {};
    var item = document.createElement('div');
    item.className = 'tool-call-item';
    var name = document.createElement('div');
    name.className = 'tool-call-name';
    name.textContent = (tc.function && tc.function.name) || '(unnamed)';
    item.appendChild(name);
    if (tc.function && tc.function.arguments) {
      var args = document.createElement('div');
      args.className = 'tool-call-args';
      // Try to pretty-print if arguments looks like JSON; otherwise show raw.
      var raw = tc.function.arguments;
      try { args.textContent = JSON.stringify(JSON.parse(raw), null, 2); }
      catch (e2) { args.textContent = raw; }
      item.appendChild(args);
    }
    box.appendChild(item);
  }
  return box;
}

// ── Edit-and-resend ──
//
// Inline editor for the most recent user message. Replaces the bubble with a
// textarea + save/cancel buttons. On save: PATCH the message in place, DELETE
// every message after it (so we don't carry stale assistant half into the
// next stream), then re-run the streaming pipeline from that user turn.
function beginEditMessage(btn, idx) {
  var s = getActiveSession();
  if (!s || !s.messages[idx] || s.messages[idx].role !== 'user') return;
  if (isStreaming) {
    addSystemNote('正在流式响应中，请等待结束后再编辑。');
    return;
  }
  var div = btn.closest('.message');
  if (!div) return;
  var original = s.messages[idx].content;
  // Build the edit form (textarea + two buttons), swap it in for the bubble.
  var form = document.createElement('div');
  form.className = 'edit-form';
  var ta = document.createElement('textarea');
  ta.value = original;
  ta.rows = 3;
  var actions = document.createElement('div');
  actions.className = 'edit-form-actions';
  var cancelBtn = document.createElement('button');
  cancelBtn.type = 'button';
  cancelBtn.textContent = '取消';
  cancelBtn.addEventListener('click', function() { cancelEdit(div); });
  var saveBtn = document.createElement('button');
  saveBtn.type = 'button';
  saveBtn.className = 'primary';
  saveBtn.textContent = '保存并重新发送';
  saveBtn.addEventListener('click', function() {
    var newText = ta.value.trim();
    if (!newText) return;
    commitEdit(s, idx, newText);
  });
  // Enter to save, Esc to cancel — standard form ergonomics.
  ta.addEventListener('keydown', function(e) {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); saveBtn.click(); }
    else if (e.key === 'Escape') { e.preventDefault(); cancelBtn.click(); }
  });
  actions.appendChild(cancelBtn);
  actions.appendChild(saveBtn);
  form.appendChild(ta);
  form.appendChild(actions);
  div.innerHTML = '';
  div.appendChild(form);
  ta.focus();
  // Auto-size to content
  ta.style.height = 'auto';
  ta.style.height = Math.min(ta.scrollHeight, 240) + 'px';
}

function cancelEdit(div) {
  // Re-render the active session to put the bubble back as it was.
  renderMessages();
}

async function commitEdit(s, idx, newText) {
  // 1. PATCH the message content in place.
  var r1 = await apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages/' + idx, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ content: newText }),
  }, '编辑用户消息');
  if (!r1.ok) {
    addSystemNote('编辑失败: ' + r1.error);
    return;
  }
  s.messages[idx].content = newText;
  // 2. Truncate everything after the edited message — the assistant
  //    half of the conversation is now stale relative to the new text.
  //    Loop one DELETE at a time so each lands atomically.
  while (s.messages.length > idx + 1) {
    var r2 = await apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages/' + (idx + 1), {
      method: 'DELETE',
    }, '截断后续消息');
    if (!r2.ok) {
      addSystemNote('截断失败: ' + r2.error);
      return;
    }
    s.messages.splice(idx + 1, 1);
  }
  // 3. Re-render so the bubble reflects the new text and the stale
  //    assistant messages are gone, then run the streaming pipeline from
  //    the edited turn. We do NOT re-POST the user message — it's already
  //    in the server's session, we just want a fresh assistant reply.
  renderMessages();
  await resendFromMessage(s, idx);
}

// Re-runs the streaming pipeline from a user message that already exists
// in the session. Same shape as the post-placeholder tail of send(), but
// without the POST-user-msg / create-placeholder steps.
async function resendFromMessage(s, userIdx) {
  isStreaming = true; sendBtn.disabled = true;
  // Create a fresh assistant placeholder and capture its index.
  var time2 = new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'});
  var placeholderResp = await apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ role: 'assistant', content: '', time: time2, memoryCtx: null }),
  }, 'resend: 创建助手占位');
  if (!placeholderResp.ok) {
    addSystemNote('占位失败: ' + placeholderResp.error);
    isStreaming = false; sendBtn.disabled = false; input.focus();
    return;
  }
  var assistantIdx = placeholderResp.data.idx;
  var assistantDiv = appendChatBubble('assistant', '...', time2);
  assistantDiv.classList.add('typing');
  var contentEl = assistantDiv.querySelector('.content');
  contentEl.textContent = '...';
  s.messages.push({ role: 'assistant', content: '', time: time2, memoryCtx: null });

  var baseUrl = resolveBackendUrl();
  var apiMessages = s.messages.filter(function(m) { return m.role === 'user' || m.role === 'assistant'; }).map(function(m) {
    return { role: m.role, content: m.content };
  });
  var fullText = '', memoryCtx = null;
  var thinkingChain = [];
  var receivedContent = false;
  try {
    var resp = await fetch(baseUrl + '/v1/chat/completions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Authorization': 'Bearer ' + apiKeyInput.value },
      body: JSON.stringify({
        messages: apiMessages,
        model: modelSelect.value,
        stream: true,
        reasoning_effort: (effortSelect.value || '').trim() || undefined,
      }),
    });
    if (!resp.ok) {
      var errBody = ''; try { errBody = await resp.text(); } catch(e2) {}
      var errMsg = 'HTTP ' + resp.status;
      try { var ej = JSON.parse(errBody); if (ej && (ej.error || ej.message)) errMsg += ' — ' + (ej.error || ej.message); }
      catch(e2) { if (errBody && errBody.length < 200) errMsg += ' — ' + errBody; else if (resp.statusText) errMsg += ' ' + resp.statusText; }
      throw new Error(errMsg);
    }
    var reader = resp.body.getReader(), decoder = new TextDecoder(), buffer = '';
    while (true) {
      var result = await reader.read();
      if (result.done) break;
      buffer += decoder.decode(result.value, { stream: true });
      var lines = buffer.split('\n');
      buffer = lines.pop() || '';
      for (var j = 0; j < lines.length; j++) {
        var line = lines[j].trim();
        if (line.indexOf('data: ') !== 0) continue;
        var data = line.slice(6);
        if (data === '[DONE]') break;
        if (data.indexOf('__MEMORY__') === 0) {
          try { memoryCtx = JSON.parse(data.slice(10)); } catch(e) {}
          continue;
        }
        if (data.indexOf('__REASONING__') === 0) {
          try {
            var rp = JSON.parse(data.slice('__REASONING__'.length));
            if (rp && typeof rp.delta === 'string') buildThinkingChain(thinkingChain, rp.delta, null);
          } catch(e) {}
          renderThinkingChain(assistantDiv, thinkingChain);
          continue;
        }
        if (data.indexOf('__TOOL_CALLS__') === 0) {
          try {
            var tc = JSON.parse(data.slice('__TOOL_CALLS__'.length));
            buildThinkingChain(thinkingChain, null, tc);
          } catch(e) {}
          renderThinkingChain(assistantDiv, thinkingChain);
          continue;
        }
        if (data.indexOf('__REASONING__') === 0) {
          try {
            var rp = JSON.parse(data.slice('__REASONING__'.length));
            if (rp && typeof rp.delta === 'string') buildThinkingChain(thinkingChain, rp.delta, null);
          } catch(e) {}
          renderThinkingChain(assistantDiv, thinkingChain);
          continue;
        }
        if (data.indexOf('__TOOL_CALLS__') === 0) {
          try {
            var tc = JSON.parse(data.slice('__TOOL_CALLS__'.length));
            buildThinkingChain(thinkingChain, null, tc);
          } catch(e) {}
          renderThinkingChain(assistantDiv, thinkingChain);
          continue;
        }
        if (!receivedContent) {
          receivedContent = true;
          assistantDiv.classList.remove('typing');
        }
        fullText += data;
        contentEl.innerHTML = mdToHtml(fullText);
        msgEl.scrollTop = msgEl.scrollHeight;
      }
    }
    var lastAssistant = s.messages[s.messages.length - 1];
    lastAssistant.content = fullText;
    lastAssistant.memoryCtx = memoryCtx;
    lastAssistant.thinkingChain = thinkingChain.length ? thinkingChain : null;
    lastAssistant.thinkingChain = thinkingChain.length ? thinkingChain : null;
    // Derive legacy reasoning/toolCalls from chain for server persistence.
    var _lr = '', _ltc = null;
    for (var _ci = 0; _ci < thinkingChain.length; _ci++) {
      if (thinkingChain[_ci].type === 'reasoning') _lr += thinkingChain[_ci].content;
      else if (thinkingChain[_ci].type === 'tool_call') _ltc = thinkingChain[_ci].content;
    }
    lastAssistant.reasoning = _lr || null;
    lastAssistant.toolCalls = _ltc || null;
    if (currentTab === 'memory' || currentTab === 'system') switchTab(currentTab);
    contentEl.innerHTML = mdToHtml(fullText);
    if (memoryCtx) renderMemoryCtx(assistantDiv, memoryCtx);
    if (memoryCtx && memoryCtx.tool_invoked) fetchStatus();
    renderThinkingChain(assistantDiv, thinkingChain);
    renderThinkingChain(assistantDiv, thinkingChain);
  } catch (e) {
    assistantDiv.classList.remove('typing');
    s.messages.pop();
    assistantDiv.remove();
    apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages/' + assistantIdx, {
      method: 'DELETE',
    }, 'resend: 回滚助手占位');
    addSystemNote('错误: ' + e.message);
    isStreaming = false; sendBtn.disabled = false; input.focus();
    return;
  }
  apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages/' + assistantIdx, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      content: fullText,
      memoryCtx: memoryCtx,
      reasoning: _lr || null,
      toolCalls: _ltc || null,
      thinkingChain: thinkingChain.length ? thinkingChain : null,
    }),
  }, 'resend: 更新助手消息');
  isStreaming = false; sendBtn.disabled = false; input.focus();
}

function updateSessionTitle(s) {
  if (s.title !== '新会话') return;
  var first = s.messages.find(function(m) { return m.role === 'user'; });
  if (first) {
    s.title = first.content.length > 16 ? first.content.slice(0, 16) + '...' : first.content;
    renderSessionList();
  }
}

// ── Rail panel toggle ──

// Mobile overlay helpers — injected once.
var _mobileOverlayInited = false;
function ensureMobileOverlay() {
  if (_mobileOverlayInited) return;
  _mobileOverlayInited = true;
  // Inject a close button at the top of each collapsible panel
  document.querySelectorAll('.collapsible').forEach(function(panel) {
    if (panel.querySelector('.panel-close')) return;
    var btn = document.createElement('button');
    btn.className = 'panel-close';
    btn.type = 'button';
    btn.setAttribute('aria-label', '关闭面板');
    btn.textContent = '×';
    panel.insertBefore(btn, panel.firstChild);
    btn.addEventListener('click', function() {
      panel.classList.add('collapsed');
      syncOverlayBodyClass();
      // Sync rail button active state
      var railBtn = document.querySelector('#rail .rail-btn[data-panel="' + panel.id + '"]');
      if (railBtn) railBtn.classList.remove('active');
      panelStates[panel.id] = false;
      sessionStorage.setItem('panel-' + panel.id, 'collapsed');
    });
  });
  // Inject backdrop element
  if (!document.querySelector('.panel-backdrop')) {
    var bd = document.createElement('div');
    bd.className = 'panel-backdrop';
    document.body.appendChild(bd);
    bd.addEventListener('click', function(ev) {
      // On mobile, native <select> dropdowns are rendered by the OS and
      // extend beyond the panel.  A click on a dropdown option hits the
      // backdrop first, which closes the panel and destroys the select.
      // Guard: if a <select> inside a collapsible panel is currently
      // focused (i.e. its dropdown is open), ignore this backdrop click.
      var activeEl = document.activeElement;
      if (activeEl && activeEl.tagName === 'SELECT' &&
          activeEl.closest('.collapsible')) {
        // Let the select handle the click; don't close panels.
        return;
      }
      // Close all open panels
      document.querySelectorAll('.collapsible').forEach(function(p) {
        p.classList.add('collapsed');
      });
      document.querySelectorAll('#rail .rail-btn').forEach(function(b) { b.classList.remove('active'); });
      Object.keys(panelStates).forEach(function(id) {
        panelStates[id] = false;
        sessionStorage.setItem('panel-' + id, 'collapsed');
      });
      syncOverlayBodyClass();
    });
  }
}

function syncOverlayBodyClass() {
  var anyOpen = false;
  document.querySelectorAll('.collapsible').forEach(function(p) {
    if (!p.classList.contains('collapsed')) anyOpen = true;
  });
  document.body.classList.toggle('panel-open', anyOpen);
}

function initRailToggles() {
  // Mobile overlay injection (no-op on desktop; CSS hides the chrome)
  ensureMobileOverlay();

  // Apply initial collapsed states
  Object.keys(panelStates).forEach(function(id) {
    var el = document.getElementById(id);
    if (!el) return;
    if (!panelStates[id]) {
      el.classList.add('collapsed');
    }
  });
  syncOverlayBodyClass();

  // Bind rail buttons
  document.querySelectorAll('#rail .rail-btn').forEach(function(btn) {
    var panelId = btn.getAttribute('data-panel');
    var panel = document.getElementById(panelId);
    if (!panel) return;

    // Set initial active state
    if (panelStates[panelId]) {
      btn.classList.add('active');
    }

    btn.addEventListener('click', function() {
      var isCollapsed = panel.classList.toggle('collapsed');
      panelStates[panelId] = !isCollapsed;
      sessionStorage.setItem('panel-' + panelId, isCollapsed ? 'collapsed' : 'expanded');
      btn.classList.toggle('active', !isCollapsed);
      // On mobile: close other panels when opening this one
      if (!isCollapsed && window.innerWidth <= 768) {
        document.querySelectorAll('#rail .rail-btn').forEach(function(otherBtn) {
          var otherPanelId = otherBtn.getAttribute('data-panel');
          if (otherPanelId !== panelId) {
            var otherPanel = document.getElementById(otherPanelId);
            if (otherPanel && !otherPanel.classList.contains('collapsed')) {
              otherPanel.classList.add('collapsed');
              panelStates[otherPanelId] = false;
              sessionStorage.setItem('panel-' + otherPanelId, 'collapsed');
              otherBtn.classList.remove('active');
            }
          }
        });
      }
      syncOverlayBodyClass();
    });
  });

  // Re-sync overlay state when viewport crosses breakpoint
  window.addEventListener('resize', function() {
    syncOverlayBodyClass();
  });
}

// ── Chat input & send ──
var msgEl, input, sendBtn, modelSelect, apiKeyInput, urlInput, reasoningSelect, effortSelect;
var chatTabsEl, chatDetailEl, chatViewEl;
var currentTab = 'conversation';

function handleKey(e) {
  if (cmdPopoverOpen) {
    if (e.key === 'ArrowDown') { e.preventDefault(); selectCmdItem(1); return; }
    if (e.key === 'ArrowUp') { e.preventDefault(); selectCmdItem(-1); return; }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      confirmCmdSelection();
      return;
    }
    if (e.key === 'Escape') { e.preventDefault(); closeCmdPopover(); return; }
  }
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); }
  input.style.height = 'auto';
  input.style.height = Math.min(input.scrollHeight, 120) + 'px';
}

// ── Slash command popover ──
var SLASH_COMMANDS = [
  { name: '/seed',    alias: '/s',  desc: '进入记忆构建模式',   args: '[描述]' },
  { name: '/recall',  alias: '/r',  desc: '召回事件',          args: '<文本>' },
  { name: '/associate', alias: '/a', desc: '概念关联',         args: '<文本>' },
  { name: '/status',  alias: '/st', desc: '当前库状态',        args: '' },
  { name: '/save',    alias: '',    desc: '持久化到磁盘',      args: '' },
  { name: '/load',    alias: '',    desc: '从磁盘读取',        args: '' },
  { name: '/help',    alias: '/h',  desc: '显示帮助',          args: '' },
];

var cmdPopoverOpen = false;
var cmdSelectedIndex = -1;
var cmdFiltered = [];

function onInputChange() {
  var val = input.value;
  if (val.indexOf('/') === 0 && val.indexOf(' ') === -1) {
    var query = val.toLowerCase();
    cmdFiltered = SLASH_COMMANDS.filter(function(c) {
      return c.name.indexOf(query) === 0 || (c.alias && c.alias.indexOf(query) === 0);
    });
    if (cmdFiltered.length > 0) {
      showCmdPopover(cmdFiltered);
      return;
    }
  }
  closeCmdPopover();
}

function showCmdPopover(items) {
  var popover = document.getElementById('cmd-popover');
  var html = '';
  for (var i = 0; i < items.length; i++) {
    var c = items[i];
    var sel = i === 0 ? ' selected' : '';
    html += '<div class="cmd-item' + sel + '" data-index="' + i + '" onmousedown="event.preventDefault();cmdSelectedIndex=' + i + ';confirmCmdSelection()">';
    html += '<span class="cmd-name">' + c.name + '</span>';
    if (c.alias) html += '<span class="cmd-alias">' + c.alias + '</span>';
    html += '<span class="cmd-desc">' + c.desc + '</span>';
    html += '</div>';
  }
  popover.innerHTML = html;
  popover.classList.add('open');
  cmdPopoverOpen = true;
  cmdSelectedIndex = 0;
}

function closeCmdPopover() {
  var popover = document.getElementById('cmd-popover');
  popover.classList.remove('open');
  cmdPopoverOpen = false;
  cmdSelectedIndex = -1;
}

function selectCmdItem(delta) {
  if (!cmdFiltered.length) return;
  cmdSelectedIndex = (cmdSelectedIndex + delta + cmdFiltered.length) % cmdFiltered.length;
  var items = document.querySelectorAll('#cmd-popover .cmd-item');
  for (var i = 0; i < items.length; i++) {
    items[i].classList.toggle('selected', i === cmdSelectedIndex);
  }
}

function confirmCmdSelection() {
  if (cmdSelectedIndex < 0 || cmdSelectedIndex >= cmdFiltered.length) return;
  var c = cmdFiltered[cmdSelectedIndex];
  input.value = c.name + ' ';
  closeCmdPopover();
  input.focus();
}

// ── Polling ──
function startPolling() {
  if (pollTimer) return;
  pollTimer = setInterval(fetchStatus, 1500);
  fetchStatus();
}

// ── Library management ──

// Fill a <select> element with library options.
// selId: DOM id of the <select>
// data: response from /api/memory/libraries
function fillLibrarySelect(selId, data) {
  var sel = document.getElementById(selId);
  if (!sel) return;
  sel.innerHTML = '';
  var opt = document.createElement('option');
  opt.value = currentLibrary;
  opt.textContent = currentLibrary + ' (active)';
  sel.appendChild(opt);
  for (var i = 0; i < data.libraries.length; i++) {
    var l = data.libraries[i];
    if (l.name === currentLibrary) continue;
    var o2 = document.createElement('option');
    o2.value = l.name;
    o2.textContent = l.name + ' (' + l.anchors_count + '锚点)';
    sel.appendChild(o2);
  }
}

async function loadLibraryList() {
  var r = await apiCall('/api/memory/libraries', { method: 'GET' }, '加载记忆库列表');
  if (!r.ok) {
    if (typeof addMessageToSession === 'function') addSystemNote(r.error);
    return;
  }
  var data = r.data;
  currentLibrary = data.active || 'default';
  knownLibraries = data.libraries || [];
  // Fill both selects: header and right panel
  fillLibrarySelect('lib-select', data);
  fillLibrarySelect('mem-lib-sel', data);
}

async function onLibraryChange(name) {
  if (!name || name === currentLibrary) return;
  if (name.indexOf(' (active)') > 0) name = name.split(' (')[0];
  var r = await apiCall('/api/memory/library/load', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name: name }),
  }, '切换记忆库');
  if (!r.ok) {
    addSystemNote(r.error);
    return;
  }
  var data = r.data;
  if (data.ok) {
    addSystemNote('已切换到记忆库: ' + data.name + ' (' + data.anchors_count + ' 锚点, ' + data.events_count + ' 事件)');
    currentLibrary = data.name;
    await loadLibraryList();
    fetchStatus();
  } else {
    // apiCall should have caught ok:false; fallback for unexpected shape
    addSystemNote('切换失败: ' + (data.error || '未知错误'));
    loadLibraryList();
  }
}

function showLibraryDialog() {
  var existing = document.querySelector('.lib-dialog');
  if (existing) existing.remove();
  var d = document.createElement('div');
  d.className = 'lib-dialog';
  d.style.cssText = 'position:fixed;inset:0;background:rgba(0,0,0,0.2);z-index:500;display:flex;align-items:center;justify-content:center';
  d.innerHTML =
    '<div style="background:#fff;border-radius:10px;padding:20px 24px;min-width:320px;max-width:420px;box-shadow:0 4px 20px rgba(0,0,0,0.12)">' +
    '<h3 style="font-size:14px;font-weight:600;margin-bottom:12px">记忆库管理</h3>' +
    '<div style="font-size:11px;color:#666;margin-bottom:8px">当前库: <strong>' + currentLibrary + '</strong></div>' +
    '<div style="font-size:11px;margin-bottom:4px">新建记忆库（空库并切换）:</div>' +
    '<div style="display:flex;gap:4px;margin-bottom:12px">' +
    '<input id="lib-create-name" placeholder="my-library" style="flex:1;padding:6px;border:1px solid var(--border);border-radius:4px;font-size:12px">' +
    '<button id="lib-create-btn" style="padding:6px 12px;background:var(--accent);color:#fff;border:none;border-radius:4px;font-size:11px;cursor:pointer">新建</button>' +
    '</div>' +
    '<div style="font-size:11px;margin-bottom:4px">保存当前库为新名称:</div>' +
    '<div style="display:flex;gap:4px;margin-bottom:12px">' +
    '<input id="lib-save-name" placeholder="my-library" style="flex:1;padding:6px;border:1px solid var(--border);border-radius:4px;font-size:12px">' +
    '<button id="lib-save-btn" style="padding:6px 12px;background:var(--surface);color:var(--text);border:1px solid var(--border);border-radius:4px;font-size:11px;cursor:pointer">另存为</button>' +
    '</div>' +
    '<div style="font-size:11px;margin-bottom:4px">已保存的库:</div>' +
    '<div id="lib-list" style="max-height:200px;overflow-y:auto;font-size:11px"></div>' +
    '<div style="margin-top:12px;text-align:right">' +
    '<button id="lib-close-btn" style="padding:6px 12px;border:1px solid var(--border);border-radius:4px;background:#fff;cursor:pointer;font-size:11px">关闭</button>' +
    '</div></div>';
  document.body.appendChild(d);
  document.getElementById('lib-close-btn').onclick = function() { d.remove(); };
  document.getElementById('lib-create-btn').onclick = function() {
    var n = document.getElementById('lib-create-name').value.trim();
    if (n) createLibrary(n);
  };
  document.getElementById('lib-save-btn').onclick = function() {
    var n = document.getElementById('lib-save-name').value.trim();
    if (n) saveAsLibrary(n);
  };
  renderLibListInDialog();
}

function promptCreateLibrary() {
  var name = prompt('新建记忆库名称:');
  if (name && name.trim()) createLibrary(name.trim());
}

async function createLibrary(name) {
  var r = await apiCall('/api/memory/library/create', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name: name }),
  }, '新建记忆库');
  if (!r.ok) {
    addSystemNote(r.error);
    return;
  }
  var data = r.data;
  if (data.ok) {
    currentLibrary = data.name;
    addSystemNote('已新建并切换到记忆库: ' + data.name);
    await loadLibraryList();
    fetchStatus();
    var d = document.querySelector('.lib-dialog');
    if (d) d.remove();
  } else {
    addSystemNote('新建失败: ' + (data.error || '未知错误'));
  }
}

async function saveAsLibrary(name) {
  var r = await apiCall('/api/memory/library/save', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name: name }),
  }, '保存记忆库');
  if (!r.ok) {
    addSystemNote(r.error);
    return;
  }
  var data = r.data;
  if (data.ok) {
    addSystemNote('已保存为记忆库: ' + data.name);
    await loadLibraryList();
    var d = document.querySelector('.lib-dialog');
    if (d) d.remove();
    showLibraryDialog();
  } else {
    addSystemNote('保存失败: ' + (data.error || '未知错误'));
  }
}

async function renderLibListInDialog() {
  var r = await apiCall('/api/memory/libraries', { method: 'GET' }, '加载库列表（对话框）');
  if (!r.ok) {
    if (typeof addMessageToSession === 'function') addSystemNote(r.error);
    return;
  }
  var data = r.data;
  var html = '';
  for (var i = 0; i < data.libraries.length; i++) {
    var l = data.libraries[i];
    var isActive = l.name === currentLibrary;
    html += '<div style="display:flex;justify-content:space-between;align-items:center;padding:4px 6px;border-bottom:1px solid #eee">' +
      '<span style="flex:1">' + (isActive ? '<strong>' : '') + l.name + ' (' + l.anchors_count + ' 锚点)' + (isActive ? ' ⭐</strong>' : '') + '</span>' +
      '<button data-name="' + l.name + '" class="lib-load-btn" style="padding:2px 8px;border:1px solid var(--border);border-radius:3px;background:#fff;cursor:pointer;font-size:10px;margin-right:4px">切换</button>' +
      (!isActive ? '<button data-name="' + l.name + '" class="lib-del-btn" style="padding:2px 8px;border:1px solid #fcc;border-radius:3px;background:#fff;color:#c33;cursor:pointer;font-size:10px">删除</button>' : '') +
      '</div>';
  }
  if (!data.libraries.length) html = '<div style="color:#999;padding:8px">（暂无已保存的库）</div>';
  var list = document.getElementById('lib-list');
  if (list) {
    list.innerHTML = html;
    var loadBtns = list.querySelectorAll('.lib-load-btn');
    for (var j = 0; j < loadBtns.length; j++) {
      loadBtns[j].onclick = function() { onLibraryChange(this.getAttribute('data-name')); var d = document.querySelector('.lib-dialog'); if (d) d.remove(); };
    }
    var delBtns = list.querySelectorAll('.lib-del-btn');
    for (var k = 0; k < delBtns.length; k++) {
      delBtns[k].onclick = function() { deleteLibrary(this.getAttribute('data-name')); };
    }
  }
}

async function deleteLibrary(name) {
  if (!confirm('删除记忆库 "' + name + '" ?')) return;
  var r = await apiCall('/api/memory/library/delete', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name: name }),
  }, '删除记忆库');
  if (!r.ok) {
    addSystemNote(r.error);
    return;
  }
  var data = r.data;
  if (data.ok) {
    addSystemNote('已删除记忆库: ' + name);
    await loadLibraryList();
    showLibraryDialog();
  } else {
    addSystemNote('删除失败: ' + (data.error || '未知错误'));
  }
}

// ── Field visualization ──
function renderFieldViz(anchors, tension) {
  var canvas = document.getElementById('field-canvas');
  if (!canvas) return;
  var w = canvas.width = canvas.clientWidth;
  var h = canvas.height = canvas.clientHeight;
  var ctx = canvas.getContext('2d');

  if (anchors.length > lastAnchorCount) {
    for (var i = 0; i < Math.min(8, anchors.length - lastAnchorCount); i++) {
      fieldParticles.push({
        x: w / 2 + (Math.random() - 0.5) * 20,
        y: h / 2 + (Math.random() - 0.5) * 20,
        vx: (Math.random() - 0.5) * 0.4,
        vy: (Math.random() - 0.5) * 0.4,
        life: 1.0,
      });
    }
  }
  lastAnchorCount = anchors.length;

  ctx.fillStyle = '#0e1116';
  ctx.fillRect(0, 0, w, h);
  var tnorm = Math.min(tension / 30, 1);
  var grad = ctx.createRadialGradient(w/2, h/2, 0, w/2, h/2, Math.min(w, h) * 0.55);
  grad.addColorStop(0, 'rgba(74, 111, 165, ' + (0.1 + tnorm * 0.4) + ')');
  grad.addColorStop(1, 'rgba(14, 17, 22, 0)');
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, w, h);

  ctx.strokeStyle = 'rgba(255,255,255,0.04)';
  ctx.lineWidth = 1;
  for (var i = 0; i < w; i += 30) {
    ctx.beginPath(); ctx.moveTo(i, 0); ctx.lineTo(i, h); ctx.stroke();
  }
  for (var j = 0; j < h; j += 30) {
    ctx.beginPath(); ctx.moveTo(0, j); ctx.lineTo(w, j); ctx.stroke();
  }

  var cx = w / 2, cy = h / 2, scale = Math.min(w, h) * 0.36;
  var positions = anchors.map(function(a) {
    return { x: cx + a.direction_xy[0] * scale, y: cy - a.direction_xy[1] * scale, a: a };
  });

  for (var i = 0; i < positions.length; i++) {
    for (var k = i + 1; k < positions.length; k++) {
      var a = positions[i].a, b = positions[k].a;
      var dot = a.direction_xy[0] * b.direction_xy[0] + a.direction_xy[1] * b.direction_xy[1];
      if (dot > 0.5) {
        var alpha = (dot - 0.5) * 0.5;
        ctx.strokeStyle = 'rgba(74, 111, 165, ' + alpha + ')';
        ctx.lineWidth = dot * 1.5;
        ctx.beginPath();
        ctx.moveTo(positions[i].x, positions[i].y);
        ctx.lineTo(positions[k].x, positions[k].y);
        ctx.stroke();
      }
    }
  }

  for (var p of positions) {
    var d = p.a.density;
    var layer = d > 15 ? 'L1' : d > 8 ? 'L2' : d > 3 ? 'L3' : 'L4';
    var colors = { L1: '#ff6b6b', L2: '#ffa94d', L3: '#ffd43b', L4: '#74c0fc' };
    var color = colors[layer];
    var size = Math.sqrt(d) * 3 + 5;

    var g2 = ctx.createRadialGradient(p.x, p.y, 0, p.x, p.y, size * 3);
    g2.addColorStop(0, color);
    g2.addColorStop(0.3, color.replace(')', ', 0.3)').replace('rgb', 'rgba'));
    g2.addColorStop(1, 'transparent');
    ctx.fillStyle = g2;
    ctx.beginPath();
    ctx.arc(p.x, p.y, size * 3, 0, Math.PI * 2);
    ctx.fill();

    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(p.x, p.y, size, 0, Math.PI * 2);
    ctx.fill();

    ctx.fillStyle = 'rgba(255,255,255,0.6)';
    ctx.beginPath();
    ctx.arc(p.x, p.y, size * 0.4, 0, Math.PI * 2);
    ctx.fill();

    ctx.fillStyle = 'rgba(255,255,255,0.7)';
    ctx.font = '10px -apple-system, sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText(p.a.label, p.x, p.y + size + 12);
  }

  fieldParticles = fieldParticles.filter(function(p) { return p.life > 0; });
  for (var particle of fieldParticles) {
    particle.x += particle.vx;
    particle.y += particle.vy;
    particle.life -= 0.02;
    ctx.fillStyle = 'rgba(255, 200, 100, ' + particle.life + ')';
    ctx.beginPath();
    ctx.arc(particle.x, particle.y, 2, 0, Math.PI * 2);
    ctx.fill();
  }
}

// ── Status / panels ──
// fetchStatus is polled every 1.5s — must not spam the chat on transient errors.
// `_statusOk` is the latch: when it flips from true→false or false→true, surface a message.
var _statusOk = true;
async function fetchStatus() {
  var r = await apiCall('/api/memory/status', { method: 'GET' }, '拉取状态');
  if (!r.ok) {
    if (_statusOk && typeof addMessageToSession === 'function') {
      addSystemNote(r.error + ' (kind=' + r.kind + ')');
    }
    _statusOk = false;
    return;
  }
  if (!_statusOk) {
    if (typeof addMessageToSession === 'function') addSystemNote('状态拉取已恢复。');
    _statusOk = true;
  }
  var data = r.data;
  renderCorePanel(data);
  renderMemPanel(data);
  // Defensive: backend may emit null/0/missing for field_tension; coerce to number.
  var t = data.ecg && data.ecg.field_tension;
  fieldTension = (typeof t === 'number' && !isNaN(t)) ? t : 0;
  renderFieldViz(data.anchors || [], fieldTension);
}

function renderCorePanel(d) {
  var el = document.getElementById('panel-core'), html = '';
  html += '<h3>引擎参数</h3>';
  html += '<div class="pc-row"><span class="l">向量维度</span><span class="v">'+d.vector_dim+'</span></div>';
  html += '<div class="pc-row"><span class="l">事件窗口（秒）</span><span class="v">'+d.event_window_secs+'</span></div>';
  html += '<div class="pc-row"><span class="l">阻尼基数</span><span class="v">'+d.damping_base.toFixed(2)+'</span></div>';
  html += '<div class="pc-row"><span class="l">刚度基数</span><span class="v">'+d.stiffness_base.toFixed(2)+'</span></div>';
  html += '<div class="pc-row"><span class="l">收敛阈值</span><span class="v">'+d.convergence_threshold.toFixed(4)+'</span></div>';
  html += '<h3>松弛周期</h3>';
  html += '<div class="pc-row"><span class="l">时间窗口（秒）</span><span class="v">'+d.cycle_window_secs+'</span></div>';
  html += '<div class="pc-row"><span class="l">impact 阈值</span><span class="v">'+d.impact_trace_threshold.toFixed(3)+'</span></div>';
  html += '<h3>计数</h3>';
  html += '<div class="pc-row"><span class="l">锚点</span><span class="v">'+d.anchors_count+'</span></div>';
  html += '<div class="pc-row"><span class="l">事件</span><span class="v">'+d.events_count+'</span></div>';
  html += '<div class="pc-row"><span class="l">痕迹</span><span class="v">'+d.traces_count+'</span></div>';
  html += '<div class="pc-row"><span class="l">种子</span><span class="v">'+d.seeds_count+'</span></div>';
  if (d.ecg) {
    html += '<h3>ECG</h3>';
    // field_tension is the only numeric field reliably present in the backend
    // response. Defensive: only call toFixed if it's actually a number.
    if (typeof d.ecg.field_tension === 'number') {
      html += '<div class="pc-row"><span class="l">场张力</span><span class="v">'+d.ecg.field_tension.toFixed(4)+'</span></div>';
    }
    // NOTE: the per-cycle bar chart was removed — the backend never exposed
    // a `cycle_counts` field on the ecg payload (see EcgBrief in memory_routes.rs).
    // Re-add a bar here once the backend emits a per-cycle series.
  }
  if (d.seeds && d.seeds.length) {
    html += '<h3>种子</h3>';
    for (var i = 0; i < d.seeds.length; i++) {
      html += '<div class="pc-seed">'+esc(d.seeds[i])+'</div>';
    }
  }
  var acts = d.recent_activity || [];
  if (acts.length) {
    html += '<h3>field-mem-core 引擎活动</h3>';
    for (var i = 0; i < acts.length; i++) {
      var a = acts[i];
      var kindLabel = '';
      switch(a.kind) {
        case 'Init': kindLabel = '初始化'; break;
        case 'EventInput': kindLabel = '事件输入'; break;
        case 'Relax': kindLabel = '松弛'; break;
        case 'ParadigmShift': kindLabel = '范式转移'; break;
        case 'Recall': kindLabel = '召回'; break;
        case 'Associate': kindLabel = '关联'; break;
        case 'Consolidate': kindLabel = '固化'; break;
        case 'Save': kindLabel = '保存'; break;
        case 'Load': kindLabel = '加载'; break;
        default: kindLabel = a.kind || '';
      }
      var ts = a.timestamp ? a.timestamp.slice(11, 19) : '';
      html += '<div class="pc-activity">';
      html += '<span class="pc-act-kind">'+esc(kindLabel)+'</span>';
      html += '<span class="pc-act-ts">'+esc(ts)+'</span>';
      html += '<div class="pc-act-detail">'+esc(a.detail || '')+'</div>';
      html += '</div>';
    }
  }
  el.innerHTML = html;
}

function renderMemPanel(d) {
  var el = document.getElementById('panel-mem');
  // ── Select area: only rebuild if missing (initial render or loadLibraryList) ──
  // The 1.5s polling cycle must NOT destroy the <select>, or the user's dropdown
  // snaps shut on every tick.  We use a marker div and only touch the select
  // through fillLibrarySelect / loadLibraryList.
  var libArea = el.querySelector('.mem-lib-area');
  if (!libArea) {
    // First render — inject the select area as a stable DOM island
    var libDiv = document.createElement('div');
    libDiv.className = 'mem-lib-area';
    libDiv.innerHTML =
      '<h3>记忆库</h3>' +
      '<div class="lib-row">' +
      '<select id="mem-lib-sel" onchange="onLibraryChange(this.value)"></select>' +
      '<button onclick="showLibraryDialog()">管理</button>' +
      '<button onclick="promptCreateLibrary()" style="margin-left:2px">+ 新建</button>' +
      '</div>';
    el.appendChild(libDiv);
    // Populate the select from cached data
    if (knownLibraries.length) {
      fillLibrarySelect('mem-lib-sel', { libraries: knownLibraries, active: currentLibrary });
    }
  }
  // ── Data area: rebuild innerHTML for everything below the select ──
  var dataArea = el.querySelector('.mem-data-area');
  if (!dataArea) {
    dataArea = document.createElement('div');
    dataArea.className = 'mem-data-area';
    el.appendChild(dataArea);
  }
  var html = '';
  html += '<div class="pm-row"><span class="l">锚点</span><span class="v">'+d.anchors_count+'</span></div>';
  html += '<div class="pm-row"><span class="l">事件</span><span class="v">'+d.events_count+'</span></div>';
  html += '<div class="pm-row"><span class="l">痕迹</span><span class="v">'+d.traces_count+'</span></div>';
  html += '<div class="pm-row"><span class="l">种子</span><span class="v">'+d.seeds_count+'</span></div>';
  if (d.anchors && d.anchors.length > 0) {
    html += '<h3>锚点密度</h3>';
    var sorted = d.anchors.slice().sort(function(a,b){return b.density - a.density;});
    var maxD = sorted[0].density || 1;
    for (var i = 0; i < Math.min(sorted.length, 12); i++) {
      var a = sorted[i];
      var pct = (a.density / maxD * 100).toFixed(0);
      html += '<div class="pm-anchor"><span class="lbl" title="'+esc(a.label)+'">'+esc(a.label)+'</span><div class="bar"><div class="fill" style="width:'+pct+'%;background:var(--accent)"></div></div><span class="val">'+a.density+'</span></div>';
    }
  }
  html += '<h3>场可视化</h3>';
  html += '<div class="field-viz-wrap"><canvas id="field-canvas" class="field-viz-canvas"></canvas><div class="field-viz-overlay" id="field-overlay"></div></div>';
  html += '<div class="pm-actions">';
  html += '<button onclick="window.open(\'/field\',\'_blank\')">3D 可视化</button>';
  html += '<button onclick="saveMem()">保存</button>';
  html += '<button onclick="loadMem()">读取</button>';
  html += '</div>';
  if (d.recent_events && d.recent_events.length > 0) {
    html += '<h3>最近事件</h3>';
    var limit = Math.min(d.recent_events.length, 5);
    for (var i = 0; i < limit; i++) {
      var ev = d.recent_events[i], ts = ev.timestamp ? ev.timestamp.slice(11, 19) : '';
      html += '<div class="pm-event"><div class="e-txt" title="'+esc(ev.text)+'">'+esc(ev.text)+'</div><div class="e-ts">'+ts+'</div></div>';
    }
  }
  dataArea.innerHTML = html;
  // Trigger field viz render
  renderFieldViz(d.anchors || [], d.ecg ? d.ecg.field_tension : 0);
}

function esc(s) { return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;'); }

function renderMemoryCtx(assistantDiv, ctx) {
  var toggle = document.createElement('div');
  toggle.className = 'mem-ctx-toggle';
  toggle.textContent = ctx.tool_invoked ? '已调用 tool' : '记忆注入详情';
  var detail = document.createElement('div');
  detail.className = 'mem-ctx-detail';
  var h = '';
  if (ctx.tool_invoked) h += '<div style="color:var(--accent);font-weight:500">tool 已被模型调用</div>';
  if (ctx.system_prompt_line) h += '<div style="margin-top:4px"><span class="label">注入：</span>' + esc(ctx.system_prompt_line) + '</div>';
  if (ctx.associations && ctx.associations.length) {
    h += '<div style="margin-top:4px"><span class="label">关联：</span></div>';
    for (var ai = 0; ai < ctx.associations.length; ai++) {
      h += '<div class="item">  ' + esc(ctx.associations[ai].label) + ' (' + ctx.associations[ai].impact + ')</div>';
    }
  }
  if (ctx.recalled_events && ctx.recalled_events.length) {
    h += '<div style="margin-top:4px"><span class="label">召回事件：</span></div>';
    for (var ei = 0; ei < ctx.recalled_events.length; ei++) {
      h += '<div class="item">  [' + esc(ctx.recalled_events[ei].anchor) + '] ' + esc(ctx.recalled_events[ei].text) + '</div>';
    }
  }
  detail.innerHTML = h;
  toggle.addEventListener('click', function() { toggle.classList.toggle('open'); detail.classList.toggle('open'); });
  assistantDiv.appendChild(toggle);
  assistantDiv.appendChild(detail);
}

// ── Markdown renderer ──
function mdToHtml(text) {
  var s = esc(text);
  // Extract and protect code blocks
  var blocks = [];
  s = s.replace(/```(\w*)\n([\s\S]*?)```/g, function(m, lang, code) {
    var idx = blocks.length;
    blocks.push('<pre><code>' + code + '</code></pre>');
    return '%%%CODEBLOCK' + idx + '%%%';
  });
  // Inline code
  s = s.replace(/`([^`]+)`/g, '<code>$1</code>');
  // Bold
  s = s.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
  // Italic
  s = s.replace(/\*(.+?)\*/g, '<em>$1</em>');
  // Blockquotes
  s = s.replace(/^&gt;\s?(.*)$/gm, '<blockquote>$1</blockquote>');
  // Unordered list
  s = s.replace(/^- (.+)$/gm, '<li>$1</li>');
  s = s.replace(/((?:<li>.*<\/li>\n?)+)/g, '<ul>$1</ul>');
  // Paragraphs: double newline -> <p>
  s = '<p>' + s.replace(/\n\n+/g, '</p><p>') + '</p>';
  // Single newline -> <br> inside paragraphs
  s = s.replace(/\n/g, '<br>');
  // Restore code blocks
  for (var i = 0; i < blocks.length; i++) {
    s = s.replace('%%%CODEBLOCK' + i + '%%%', blocks[i]);
  }
  // Cleanup empty paragraphs
  s = s.replace(/<p>\s*<\/p>/g, '');
  return s;
}

// ── Chat ──
async function send() {
  var text = input.value.trim();
  if (!text || isStreaming) return;
  input.value = ''; input.style.height = 'auto';
  closeCmdPopover();
  isStreaming = true; sendBtn.disabled = true;

  var s = getActiveSession();
  if (!s) return;

  var welcome = msgEl.querySelector('.welcome');
  if (welcome) welcome.remove();

  if (text.indexOf('/') === 0) {
    var parts = text.split(/\s+/);
    var cmdName = parts[0].slice(1).toLowerCase();
    if (cmdName === 'seed' || cmdName === 's') { await handleSeedCommand(parts.slice(1)); isStreaming = false; sendBtn.disabled = false; input.focus(); return; }
    if (cmdName === 'recall' || cmdName === 'r') { await handleQueryCommand(parts.slice(1).join(' '), 'recall'); isStreaming = false; sendBtn.disabled = false; input.focus(); return; }
    if (cmdName === 'associate' || cmdName === 'a') { await handleQueryCommand(parts.slice(1).join(' '), 'associate'); isStreaming = false; sendBtn.disabled = false; input.focus(); return; }
    if (cmdName === 'save') { await saveMem(); isStreaming = false; sendBtn.disabled = false; input.focus(); return; }
    if (cmdName === 'load') { await loadMem(); isStreaming = false; sendBtn.disabled = false; input.focus(); return; }
    if (cmdName === 'status' || cmdName === 'st') { await showStatusInline(); isStreaming = false; sendBtn.disabled = false; input.focus(); return; }
    if (cmdName === 'help' || cmdName === 'h' || cmdName === '?') { showHelp(); isStreaming = false; sendBtn.disabled = false; input.focus(); return; }
addSystemNote('未知命令: ' + parts[0] + '。输入 /help 查看命令。');
    isStreaming = false; sendBtn.disabled = false; input.focus();
return;
}

  var time = new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'});

  // Step 1: persist the user message immediately. We don't await — the user's
  // typing flow shouldn't block on the round-trip, and a network hiccup will
  // surface as a console warning rather than a stuck spinner.
  apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ role: 'user', content: text, time: time }),
  }, '记录用户消息');
s.messages.push({ role: 'user', content: text, time: time });
  updateSessionTitle(s);
  // Pass msgIdx so the bubble renders an edit button on hover.
  appendChatBubble('user', text, time, s.messages.length - 1);

  // Step 2: create the assistant placeholder server-side and capture its index.
  // We MUST await here — without the idx the streaming-finish PATCH has nowhere
  // to land. If this fails, we abort the send and tell the user.
  var time2 = new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'});
  var placeholderResp = await apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ role: 'assistant', content: '', time: time2, memoryCtx: null }),
  }, '创建助手占位');
  if (!placeholderResp.ok) {
if (typeof addSystemNote === 'function') addSystemNote('占位失败: ' + placeholderResp.error);
    isStreaming = false; sendBtn.disabled = false; input.focus();
    return;
  }
  var assistantIdx = placeholderResp.data.idx;
  var assistantDiv = appendChatBubble('assistant', '...', time2);
  assistantDiv.classList.add('typing');
  var contentEl = assistantDiv.querySelector('.content');
  contentEl.textContent = '...';
  s.messages.push({ role: 'assistant', content: '', time: time2, memoryCtx: null });

  var baseUrl = resolveBackendUrl();
  var apiMessages = s.messages.filter(function(m) { return m.role === 'user' || m.role === 'assistant'; }).map(function(m) {
    return { role: m.role, content: m.content };
  });
  try {
    var resp = await fetch(baseUrl + '/v1/chat/completions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Authorization': 'Bearer ' + apiKeyInput.value },
body: JSON.stringify({
        messages: apiMessages,
        model: modelSelect.value,
        stream: true,
        reasoning_effort: (effortSelect.value || '').trim() || undefined,
      }),
    });
    if (!resp.ok) {
      // Try to read error body for richer message; fall back to status text
      var errBody = '';
      try { errBody = await resp.text(); } catch(e2) {}
      var errMsg = 'HTTP ' + resp.status;
      try { var ej = JSON.parse(errBody); if (ej && (ej.error || ej.message)) errMsg += ' — ' + (ej.error || ej.message); }
      catch(e2) { if (errBody && errBody.length < 200) errMsg += ' — ' + errBody; else if (resp.statusText) errMsg += ' ' + resp.statusText; }
      throw new Error(errMsg);
    }
var fullText = '', memoryCtx = null, reader = resp.body.getReader(), decoder = new TextDecoder(), buffer = '', receivedContent = false;
    // thinkingChain captures the alternation of reasoning ↔ tool_call steps
    // in their original order.  We accumulate it incrementally from SSE deltas.
    var thinkingChain = [];
    while (true) {
      var result = await reader.read();
      if (result.done) break;
      buffer += decoder.decode(result.value, { stream: true });
      var lines = buffer.split('\n');
      buffer = lines.pop() || '';
      for (var j = 0; j < lines.length; j++) {
        var line = lines[j].trim();
        if (line.indexOf('data: ') !== 0) continue;
        var data = line.slice(6);
        if (data === '[DONE]') break;
        if (data.indexOf('__MEMORY__') === 0) {
          try { memoryCtx = JSON.parse(data.slice(10)); } catch(e) {}
          continue;
        }
        if (data.indexOf('__REASONING__') === 0) {
          try {
            var rp = JSON.parse(data.slice('__REASONING__'.length));
            if (rp && typeof rp.delta === 'string') buildThinkingChain(thinkingChain, rp.delta, null);
          } catch(e) {}
          renderThinkingChain(assistantDiv, thinkingChain);
          continue;
        }
        if (data.indexOf('__TOOL_CALLS__') === 0) {
          try {
            var tc = JSON.parse(data.slice('__TOOL_CALLS__'.length));
            buildThinkingChain(thinkingChain, null, tc);
          } catch(e) {}
          renderThinkingChain(assistantDiv, thinkingChain);
          continue;
        }
        if (!receivedContent) {
          receivedContent = true;
          assistantDiv.classList.remove('typing');
        }
        fullText += data;
        contentEl.innerHTML = mdToHtml(fullText);
        msgEl.scrollTop = msgEl.scrollHeight;
      }
    }
var lastAssistant = s.messages[s.messages.length - 1];
    lastAssistant.content = fullText;
    lastAssistant.memoryCtx = memoryCtx;
    // Persist thinkingChain so they survive session switches and refresh.
    // Also derive legacy reasoning/toolCalls for backwards compatibility
    // with the server's persisted message format.
    lastAssistant.thinkingChain = thinkingChain.length ? thinkingChain : null;
    var lastReasoning = '';
    var lastToolCalls = null;
    for (var ci = 0; ci < thinkingChain.length; ci++) {
      if (thinkingChain[ci].type === 'reasoning') lastReasoning += thinkingChain[ci].content;
      else if (thinkingChain[ci].type === 'tool_call') lastToolCalls = thinkingChain[ci].content;
    }
    lastAssistant.reasoning = lastReasoning || null;
    lastAssistant.toolCalls = lastToolCalls || null;
    if (currentTab === 'memory' || currentTab === 'system') switchTab(currentTab);
    contentEl.innerHTML = mdToHtml(fullText);
    if (memoryCtx) renderMemoryCtx(assistantDiv, memoryCtx);
    if (memoryCtx && memoryCtx.tool_invoked) fetchStatus();
    // Final chain render so the timeline reflects the complete steps.
    renderThinkingChain(assistantDiv, thinkingChain);
} catch (e) {
    // Roll back the placeholder we created at the start of send() so the
    // assistant message never carries an "错误: ..." line that would leak
    // into subsequent LLM history. Surface the failure as a system note.
    assistantDiv.classList.remove('typing');
    s.messages.pop();
    assistantDiv.remove();
    apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages/' + assistantIdx, {
      method: 'DELETE',
    }, '回滚助手占位');
    addSystemNote('错误: ' + e.message);
    isStreaming = false; sendBtn.disabled = false; input.focus();
    return;
  }
  // Step 4: persist the final assistant content + memoryCtx via PATCH on the
  // placeholder. Fire-and-forget — the local view is already correct, so a
  // failure here only loses the cross-device view of THIS message, not the
  // conversation history.
  apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages/' + assistantIdx, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
body: JSON.stringify({
      content: fullText,
      memoryCtx: memoryCtx,
      reasoning: lastReasoning || null,
      toolCalls: lastToolCalls || null,
      thinkingChain: thinkingChain.length ? thinkingChain : null,
    }),
  }, '更新助手消息');
  isStreaming = false; sendBtn.disabled = false; input.focus();
}

function appendChatBubble(role, content, time, msgIdx) {
  var div = document.createElement('div');
  div.className = 'message ' + role;
  // system_note: a one-shot UI hint (command response / error). Rendered as
  // an inline note with an info icon — distinct from real assistant replies
  // so the user can tell at a glance what's LLM-generated and what's the
  // console talking back. Also filtered out of LLM history in send().
  if (role === 'system_note') {
    div.innerHTML = '<div class="content">' +
      '<svg class="note-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">' +
        '<circle cx="8" cy="8" r="6.5"/>' +
        '<line x1="8" y1="7" x2="8" y2="11.5"/>' +
        '<circle cx="8" cy="4.7" r="0.6" fill="currentColor" stroke="none"/>' +
      '</svg>' +
      '<span class="note-text">' + esc(content) + '</span>' +
    '</div>';
    msgEl.appendChild(div);
    msgEl.scrollTop = msgEl.scrollHeight;
    return div;
  }
  var label = role === 'user' ? '你' : 'FM';
  var actionsHtml = '';
  // Edit button only for user messages. idx is the message index in the
  // session — used by editAndResend() to PATCH + truncate + re-stream.
  if (role === 'user' && typeof msgIdx === 'number') {
    actionsHtml = '<div class="message-actions">' +
      '<button class="message-action-btn" title="编辑并重新发送" onclick="beginEditMessage(this, ' + msgIdx + ')">' +
        ICON_PENCIL +
      '</button>' +
    '</div>';
  }
  div.innerHTML = '<div class="content">' + esc(content) + '</div>' +
    actionsHtml +
    '<div class="meta">' + label + ' &middot; ' + (time || new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'})) + '</div>';
  msgEl.appendChild(div);
  msgEl.scrollTop = msgEl.scrollHeight;
  return div;
}

function addMessageToSession(role, content) {
  var s = getActiveSession();
  if (!s) return;
  var time = new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'});
  // Optimistic local update.
  s.messages.push({ role: role, content: content, time: time });
  appendChatBubble(role, content, time);
  updateSessionTitle(s);
  // Persist immediately — every mutation lands server-side before this returns.
apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ role: role, content: content, time: time }),
  }, '追加消息').then(function(r) {
    if (!r.ok) console.warn('addMessageToSession:', r.error);
  });
}

// One-shot UI note — kept in the messages stream (so refresh / re-render
// preserves it) but tagged 'system_note' so send() filters it out of the
// LLM history. Use this for command responses, validation hints, errors —
// anything the model shouldn't see as prior context.
function addSystemNote(content) {
  var s = getActiveSession();
  if (!s) return;
  var time = new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'});
  s.messages.push({ role: 'system_note', content: content, time: time });
  appendChatBubble('system_note', content, time);
  // No updateSessionTitle() call — system notes shouldn't influence the title.
  apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ role: 'system_note', content: content, time: time }),
  }, '追加系统提示').then(function(r) {
    if (!r.ok) console.warn('addSystemNote:', r.error);
  });
}

// ── Settings modal ──
var CONFIG_KEY = 'fm-config';
var DEFAULT_CONFIG = {
  // Empty string = "use the same origin as the page". Critical for mobile / LAN access
  // where hardcoding 127.0.0.1 would point the phone at itself, not the server.
  // Users can still set this to a different LLM provider URL if needed.
  backendUrl: '',
  model: 'test-model-1',
  apiKey: 'sk-fm',
  // Reasoning effort: low | medium | high | xhigh. Passed through to the
  // upstream LLM verbatim; if a model doesn't understand it, the field is
  // omitted server-side and nothing breaks.
  reasoningEffort: 'medium',
};

// Resolve the LLM backend URL.
//   - User-set URL (settings.modal) → use it
//   - Otherwise → window.location.origin (same as the page itself)
//
// This is what makes LAN/mobile access work: open http://192.168.1.12:5100
// on the phone, LLM calls go to that same origin. A hardcoded 127.0.0.1
// would route the phone's request to itself → "Load failed".
function resolveBackendUrl() {
  try {
    var v = (urlInput && urlInput.value || '').trim();
    if (v) return v.replace(/\/$/, '');
  } catch(e) {}
  return window.location.origin;
}

function loadConfig() {
  try {
    var raw = localStorage.getItem(CONFIG_KEY);
    if (!raw) return Object.assign({}, DEFAULT_CONFIG);
    var data = JSON.parse(raw);
    var cfg = Object.assign({}, DEFAULT_CONFIG, data);
    // Migration: the old hardcoded default was 'http://127.0.0.1:5100', which
    // breaks mobile/LAN access (phone would talk to itself). Treat that legacy
    // value as "empty" so the same-origin fallback kicks in.
    if (cfg.backendUrl === 'http://127.0.0.1:5100' || cfg.backendUrl === 'http://localhost:5100') {
      cfg.backendUrl = '';
    }
    return cfg;
  } catch(e) { return Object.assign({}, DEFAULT_CONFIG); }
}

function saveConfigToDisk() {
  try {
    var c = {
      backendUrl: urlInput.value.trim(),
      model: modelSelect.value.trim(),
      apiKey: apiKeyInput.value,
      reasoningEffort: effortSelect.value,
    };
    localStorage.setItem(CONFIG_KEY, JSON.stringify(c));
  } catch(e) {}
}

function openSettings() {
  document.getElementById('settings-modal').hidden = false;
}

function closeSettings() {
  document.getElementById('settings-modal').hidden = true;
}

function saveSettings() {
  saveConfigToDisk();
  closeSettings();
}

// ── Chat detail tabs ──
var SKILL_DEFS = [
  { name: 'seed_memory', desc: '根据一组种子概念创建全新的记忆知识库。' },
  { name: 'recall_memory', desc: '从记忆库中召回与查询文本相关的事件和锚点。' },
  { name: 'associate_memory', desc: '从记忆库中查找与查询文本语义相关的概念锚点（轻量版）。' },
];

function findLastMemoryCtx() {
  var s = getActiveSession();
  if (!s) return null;
  for (var i = s.messages.length - 1; i >= 0; i--) {
    var m = s.messages[i];
    if (m.role === 'assistant' && m.memoryCtx) return m.memoryCtx;
  }
  return null;
}

function renderSystemTab() {
  var ctx = findLastMemoryCtx();
  var sys = ctx && ctx.system_prompt_line ? ctx.system_prompt_line : '（暂无系统提示词）';
  return '<h3>当前系统提示词</h3>' +
    '<pre class="prompt-block">' + esc(sys) + '</pre>' +
    '<div class="prompt-note">注意：以上 [Memory Recall] 内容是辅助上下文，不是需要回写到记忆库的知识。</div>';
}

function renderSkillsTab() {
  var html = '<h3>可用技能（tool）</h3>';
  for (var i = 0; i < SKILL_DEFS.length; i++) {
    var s = SKILL_DEFS[i];
    html += '<div class="skill-card">' +
      '<div class="skill-name">' + esc(s.name) + '</div>' +
      '<div class="skill-desc">' + esc(s.desc) + '</div>' +
    '</div>';
  }
  return html;
}

function renderMemoryTab() {
  var ctx = findLastMemoryCtx();
  if (!ctx) return '<div class="empty">（暂无记忆注入）</div>';
  var html = '<h3>最近一次记忆注入</h3>';
  if (ctx.tool_invoked) html += '<div class="ctx-flag">tool 已被模型调用</div>';
  if (ctx.associations && ctx.associations.length) {
    html += '<div class="ctx-section-title">关联概念</div>';
    for (var i = 0; i < ctx.associations.length; i++) {
      var a = ctx.associations[i];
      html += '<div class="ctx-item">  ' + esc(a.label) + ' <span class="ctx-impact">(' + a.impact + ')</span></div>';
    }
  } else {
    html += '<div class="ctx-empty">（无关联概念）</div>';
  }
  if (ctx.recalled_events && ctx.recalled_events.length) {
    html += '<div class="ctx-section-title">召回事件</div>';
    for (var j = 0; j < ctx.recalled_events.length; j++) {
      var ev = ctx.recalled_events[j];
      html += '<div class="ctx-item">  [' + esc(ev.anchor) + '] ' + esc(ev.text) + '</div>';
    }
  } else {
    html += '<div class="ctx-empty">（无召回事件）</div>';
  }
  return html;
}

function switchTab(name) {
  currentTab = name;
  var tabs = chatTabsEl.querySelectorAll('.chat-tab');
  for (var i = 0; i < tabs.length; i++) {
    tabs[i].classList.toggle('active', tabs[i].getAttribute('data-tab') === name);
  }
  if (name === 'conversation') {
    chatViewEl.hidden = false;
    chatDetailEl.hidden = true;
  } else {
    chatViewEl.hidden = true;
    chatDetailEl.hidden = false;
    var html = '';
    if (name === 'system') html = renderSystemTab();
    else if (name === 'skills') html = renderSkillsTab();
    else if (name === 'memory') html = renderMemoryTab();
    chatDetailEl.innerHTML = html;
    chatDetailEl.scrollTop = 0;
  }
}

async function handleSeedCommand(args) {
  var userDesc = args.length > 0 ? args.join(' ') : '';
  var seedPrompt = '【记忆构建模式】你现在是记忆构建助手。请通过对话了解我的知识结构、经验偏好和核心原则，然后逐步调用 seed_memory tool 来构建记忆库。\n' +
    '建议流程：\n' +
    '1. 先通过提问了解我的背景\n' +
    '2. 每次了解一批概念后调用一次 seed_memory\n' +
    '3. 继续提问、继续注入，直到记忆库完整\n';
  if (userDesc) seedPrompt += '\n初始信息：' + userDesc + '\n请在此基础上开始提问。';

  var s = getActiveSession();
  if (!s) return;
  var time = new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'});
  var userText = '/' + (args.length > 0 ? 'seed ' + userDesc : 'seed');
  // Persist user message + create assistant placeholder server-side before
  // opening the stream — same atomic pattern as send().
  apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ role: 'user', content: userText, time: time }),
  }, 'seed: 记录用户消息');
  s.messages.push({ role: 'user', content: userText, time: time });
  appendChatBubble('user', userText, time);
  updateSessionTitle(s);

  var time2 = new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'});
  var placeholderResp = await apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ role: 'assistant', content: '', time: time2, memoryCtx: null }),
  }, 'seed: 创建助手占位');
  if (!placeholderResp.ok) {
    if (typeof addMessageToSession === 'function') addSystemNote('占位失败: ' + placeholderResp.error);
    return;
  }
  var assistantIdx = placeholderResp.data.idx;
  var assistantDiv = appendChatBubble('assistant', '...', time2);
  assistantDiv.classList.add('typing');
  var contentEl = assistantDiv.querySelector('.content');
  contentEl.textContent = '...';
  s.messages.push({ role: 'assistant', content: '', time: time2, memoryCtx: null });

  var baseUrl = resolveBackendUrl();
  var apiMessages = [{ role: 'user', content: seedPrompt }];
  try {
    var resp = await fetch(baseUrl + '/v1/chat/completions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Authorization': 'Bearer ' + apiKeyInput.value },
      body: JSON.stringify({ messages: apiMessages, model: modelSelect.value, stream: true }),
    });
    if (!resp.ok) throw new Error('HTTP ' + resp.status);
    var fullText = '', memoryCtx = null, reader = resp.body.getReader(), decoder = new TextDecoder(), buffer = '', receivedContent = false;
    var thinkingChain = [];
    while (true) {
      var result = await reader.read();
      if (result.done) break;
      buffer += decoder.decode(result.value, { stream: true });
      var lines = buffer.split('\n');
      buffer = lines.pop() || '';
      for (var j = 0; j < lines.length; j++) {
        var line = lines[j].trim();
        if (line.indexOf('data: ') !== 0) continue;
        var data = line.slice(6);
        if (data === '[DONE]') break;
        if (data.indexOf('__MEMORY__') === 0) {
          try { memoryCtx = JSON.parse(data.slice(10)); } catch(e) {}
          continue;
        }
        if (!receivedContent) {
          receivedContent = true;
          assistantDiv.classList.remove('typing');
        }
        fullText += data;
        contentEl.innerHTML = mdToHtml(fullText);
        msgEl.scrollTop = msgEl.scrollHeight;
      }
    }
    var lastAssistant = s.messages[s.messages.length - 1];
    lastAssistant.content = fullText;
    lastAssistant.memoryCtx = memoryCtx;
    if (currentTab === 'memory' || currentTab === 'system') switchTab(currentTab);
    contentEl.innerHTML = mdToHtml(fullText);
    if (memoryCtx) renderMemoryCtx(assistantDiv, memoryCtx);
    if (memoryCtx && memoryCtx.tool_invoked) fetchStatus();
  } catch (e) {
    assistantDiv.classList.remove('typing');
    contentEl.innerHTML = '<span style="color:#c33">错误: ' + esc(e.message) + '</span>';
fullText = '错误: ' + e.message;
    var lastAssistant2 = s.messages[s.messages.length - 1];
    lastAssistant2.content = fullText;
    lastAssistant2.memoryCtx = null;
  }
  // PATCH the placeholder with the final streamed content (fire-and-forget).
  apiCall('/api/sessions/' + encodeURIComponent(s.id) + '/messages/' + assistantIdx, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ content: fullText, memoryCtx: memoryCtx, thinkingChain: thinkingChain.length ? thinkingChain : null }),
  }, 'seed: 更新助手消息');
  isStreaming = false; sendBtn.disabled = false; input.focus();
}

async function handleQueryCommand(query, mode) {
  if (!query) {
    addSystemNote('用法: /' + mode + ' <查询文本>');
    return;
  }
  var msg = appendChatBubble('assistant', '正在查询...', new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'}));
  msg.querySelector('.content').textContent = mode + ': ' + query;
  var r = await apiCall('/api/memory/query', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query: query, mode: mode, top_k: 5 }),
  }, '查询');
  if (!r.ok) {
    msg.querySelector('.content').textContent = r.error;
    var sErr = getActiveSession();
    if (sErr) sErr.messages.push({ role: 'assistant', content: r.error, time: new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'}) });
    return;
  }
  var data = r.data;
  var result = '';
    if (data.associated_anchors && data.associated_anchors.length > 0) {
      result += '关联锚点:\n';
      for (var i = 0; i < data.associated_anchors.length; i++) {
        var a = data.associated_anchors[i];
        result += '  [' + a.label + '] density=' + a.density + ' impact=' + a.impact + '\n';
      }
    }
    if (data.recalled_events && data.recalled_events.length > 0) {
      result += '召回事件:\n';
      for (var i = 0; i < data.recalled_events.length; i++) {
        var e = data.recalled_events[i];
        result += '  [' + e.anchor + '] ' + e.text + '\n';
      }
    }
    if (!result) result = '(无结果)';
    msg.querySelector('.content').textContent = result;
    var s = getActiveSession();
    if (s) s.messages.push({ role: 'assistant', content: result, time: new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'}) });
}

async function showStatusInline() {
  var r = await apiCall('/api/memory/status', { method: 'GET' }, '获取状态');
  if (!r.ok) { addSystemNote(r.error); return; }
  var data = r.data;
    var anchors = data.anchors || [];
    var lines = [];
    lines.push('库: ' + currentLibrary);
    lines.push('锚点: ' + data.anchors_count + ' | 事件: ' + data.events_count + ' | 痕迹: ' + data.traces_count + ' | 种子: ' + data.seeds_count);
    if (data.ecg && typeof data.ecg.field_tension === 'number') {
      lines.push('张力: ' + data.ecg.field_tension.toFixed(4));
    }
    for (var i = 0; i < Math.min(anchors.length, 8); i++) {
      lines.push('  ' + anchors[i].label + ' (d=' + anchors[i].density + ')');
    }
    addSystemNote(lines.join('\n'));
}

function showHelp() {
  var h = '可用命令:\n' +
    '  /seed [描述]              进入记忆构建模式（唤起 LLM）\n' +
    '  /recall <文本>             召回事件（别名: /r）\n' +
    '  /associate <文本>          概念关联（别名: /a）\n' +
    '  /status                   当前库状态（别名: /st）\n' +
    '  /save / /load             持久化到磁盘\n' +
    '  /help                     显示帮助\n\n' +
    '记忆库管理: 点 header 的"管理"按钮';
  addSystemNote(h);
}

async function saveMem() {
  var r = await apiCall('/api/memory/save', { method: 'POST' }, '保存记忆');
  if (!r.ok) { addSystemNote(r.error); fetchStatus(); return; }
  var data = r.data;
  if (data.ok) {
    addSystemNote('记忆已保存到磁盘。');
  } else {
    addSystemNote('保存失败: ' + (data.error || '未知错误'));
  }
  fetchStatus();
}

async function loadMem() {
  var r = await apiCall('/api/memory/load', { method: 'POST' }, '读取记忆');
  if (!r.ok) { addSystemNote(r.error); fetchStatus(); return; }
  var data = r.data;
  if (data.ok) {
    addSystemNote('已读取 ' + data.anchors_count + ' 锚点、' + data.events_count + ' 事件。');
  } else {
    addSystemNote('读取失败: ' + (data.error || '未知错误'));
  }
  fetchStatus();
}

// ── Global error handler ──
window.addEventListener('error', function(e) {
  console.error('Global error:', e.message, e.filename, e.lineno);
  if (msgEl) {
    addSystemNote('运行时错误: ' + e.message + ' (line ' + e.lineno + ')');
  }
});
window.addEventListener('unhandledrejection', function(e) {
  console.error('Unhandled promise rejection:', e.reason);
  if (msgEl) {
    addSystemNote('异步错误: ' + (e.reason && e.reason.message || e.reason || '未知错误'));
  }
});

// ── Init ──
document.addEventListener('DOMContentLoaded', function() {
  msgEl = document.getElementById('messages');
  input = document.getElementById('user-input');
  sendBtn = document.getElementById('send-btn');
  modelSelect = document.getElementById('settings-model');
  apiKeyInput = document.getElementById('settings-api-key');
urlInput = document.getElementById('settings-backend-url');
  reasoningSelect = document.getElementById('settings-reasoning-effort');
  effortSelect = document.getElementById('effort-select');
  chatTabsEl = document.getElementById('chat-tabs');
  chatDetailEl = document.getElementById('chat-detail');
  chatViewEl = document.getElementById('chat-view');

  // Load saved config
  var cfg = loadConfig();
urlInput.value = cfg.backendUrl;
  modelSelect.value = cfg.model;
  apiKeyInput.value = cfg.apiKey;
  reasoningSelect.value = cfg.reasoningEffort || 'medium';
  effortSelect.value = cfg.reasoningEffort || 'medium';
  // Sync the two reasoning effort selects — changing one updates the other.
  reasoningSelect.addEventListener('change', function() {
    effortSelect.value = reasoningSelect.value;
    saveConfigToDisk();
  });
  effortSelect.addEventListener('change', function() {
    reasoningSelect.value = effortSelect.value;
    saveConfigToDisk();
  });

  input.addEventListener('keydown', handleKey);
  input.addEventListener('input', onInputChange);

  // Tab switching
  chatTabsEl.addEventListener('click', function(e) {
    var btn = e.target.closest('.chat-tab');
    if (!btn) return;
    switchTab(btn.getAttribute('data-tab'));
  });

initRailToggles();
  // Refresh from server on load. If no sessions exist yet (fresh install),
  // create one so the UI isn't empty.
  refreshSessions().then(function() {
    if (sessions.length === 0) createSession();
  });
  loadLibraryList();
  startPolling();
  // No more session polling — visibilitychange triggers refreshSessions() on
  // foreground return, which is enough for a single-user LAN tool.
});
