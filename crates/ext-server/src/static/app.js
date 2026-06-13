// ── State ──
var sessions = [];
var activeSessionId = null;
var isStreaming = false;
var pollTimer = null;
var currentLibrary = 'default';
var fieldParticles = [];
var lastAnchorCount = 0;
var fieldTension = 0;
var anchors = [];

// ── Panel collapse state (persisted in sessionStorage) ──
var panelStates = {
  'session-sidebar': sessionStorage.getItem('panel-session-sidebar') !== 'collapsed',
  'panel-core': sessionStorage.getItem('panel-panel-core') !== 'collapsed',
  'panel-mem': sessionStorage.getItem('panel-panel-mem') !== 'collapsed',
};

// ── Session management ──
function getActiveSession() {
  return sessions.find(function(s) { return s.id === activeSessionId; });
}

function createSession() {
  var s = { id: Date.now().toString(), title: '新会话', messages: [] };
  sessions.push(s);
  activeSessionId = s.id;
  renderSessionList();
  renderMessages();
}

function switchSession(id) {
  activeSessionId = id;
  renderSessionList();
  renderMessages();
}

function deleteSession(id) {
  sessions = sessions.filter(function(s) { return s.id !== id; });
  if (activeSessionId === id) {
    activeSessionId = sessions.length ? sessions[0].id : null;
  }
  renderSessionList();
  renderMessages();
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
    div.innerHTML = '<div class="content">' + esc(m.content) + '</div>' +
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
    el.appendChild(div);
  }
  el.scrollTop = el.scrollHeight;
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
function initRailToggles() {
  // Apply initial collapsed states
  Object.keys(panelStates).forEach(function(id) {
    var el = document.getElementById(id);
    if (!el) return;
    if (!panelStates[id]) {
      el.classList.add('collapsed');
    }
  });

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
    });
  });
}

// ── Chat input & send ──
var msgEl, input, sendBtn, modelSelect, apiKeyInput, urlInput;

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
async function loadLibraryList() {
  try {
    var resp = await fetch('/api/memory/libraries');
    var data = await resp.json();
    currentLibrary = data.active || 'default';
    var sel = document.getElementById('lib-select');
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
  } catch(e) {}
}

async function onLibraryChange(name) {
  if (!name || name === currentLibrary) return;
  if (name.indexOf(' (active)') > 0) name = name.split(' (')[0];
  try {
    var resp = await fetch('/api/memory/library/load', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name }),
    });
    var data = await resp.json();
    if (data.ok) {
      addMessageToSession('assistant', '已切换到记忆库: ' + data.name + ' (' + data.anchors_count + ' 锚点, ' + data.events_count + ' 事件)');
      currentLibrary = data.name;
      await loadLibraryList();
      fetchStatus();
    } else {
      addMessageToSession('assistant', '切换失败: ' + (data.error || '未知错误'));
      loadLibraryList();
    }
  } catch(e) {
    addMessageToSession('assistant', '切换请求失败: ' + e.message);
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
    '<div style="font-size:11px;margin-bottom:4px">保存当前库为新名称:</div>' +
    '<div style="display:flex;gap:4px;margin-bottom:12px">' +
    '<input id="lib-save-name" placeholder="my-library" style="flex:1;padding:6px;border:1px solid var(--border);border-radius:4px;font-size:12px">' +
    '<button id="lib-save-btn" style="padding:6px 12px;background:var(--accent);color:#fff;border:none;border-radius:4px;font-size:11px;cursor:pointer">保存</button>' +
    '</div>' +
    '<div style="font-size:11px;margin-bottom:4px">已保存的库（点击删除）:</div>' +
    '<div id="lib-list" style="max-height:200px;overflow-y:auto;font-size:11px"></div>' +
    '<div style="margin-top:12px;text-align:right">' +
    '<button id="lib-close-btn" style="padding:6px 12px;border:1px solid var(--border);border-radius:4px;background:#fff;cursor:pointer;font-size:11px">关闭</button>' +
    '</div></div>';
  document.body.appendChild(d);
  document.getElementById('lib-close-btn').onclick = function() { d.remove(); };
  document.getElementById('lib-save-btn').onclick = function() {
    var n = document.getElementById('lib-save-name').value.trim();
    if (n) saveAsLibrary(n);
  };
  renderLibListInDialog();
}

async function saveAsLibrary(name) {
  try {
    var resp = await fetch('/api/memory/library/save', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name }),
    });
    var data = await resp.json();
    if (data.ok) {
      addMessageToSession('assistant', '已保存为记忆库: ' + data.name);
      await loadLibraryList();
      var d = document.querySelector('.lib-dialog');
      if (d) d.remove();
      showLibraryDialog();
    } else {
      addMessageToSession('assistant', '保存失败: ' + (data.error || '未知错误'));
    }
  } catch(e) { addMessageToSession('assistant', '保存失败: ' + e.message); }
}

async function renderLibListInDialog() {
  try {
    var resp = await fetch('/api/memory/libraries');
    var data = await resp.json();
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
  } catch(e) {}
}

async function deleteLibrary(name) {
  if (!confirm('删除记忆库 "' + name + '" ?')) return;
  try {
    var resp = await fetch('/api/memory/library/delete', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name }),
    });
    var data = await resp.json();
    if (data.ok) {
      addMessageToSession('assistant', '已删除记忆库: ' + name);
      await loadLibraryList();
      showLibraryDialog();
    } else {
      addMessageToSession('assistant', '删除失败: ' + (data.error || ''));
    }
  } catch(e) {}
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
async function fetchStatus() {
  try {
    var resp = await fetch('/api/memory/status');
    if (!resp.ok) throw Error();
    var data = await resp.json();
    renderCorePanel(data);
    renderMemPanel(data);
    fieldTension = data.ecg ? data.ecg.field_tension : 0;
    renderFieldViz(data.anchors || [], fieldTension);
  } catch(e) {}
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
    html += '<div class="pc-row"><span class="l">场张力</span><span class="v">'+d.ecg.field_tension.toFixed(4)+'</span></div>';
    html += '<div class="ecg-bar">';
    var maxC = Math.max.apply(null, d.ecg.cycle_counts.concat([1]));
    for (var i = 0; i < d.ecg.cycle_counts.length; i++) {
      var pct = (d.ecg.cycle_counts[i] / maxC * 100).toFixed(0);
      html += '<div class="col" style="height:'+pct+'%"></div>';
    }
    html += '</div>';
  }
  if (d.seeds && d.seeds.length) {
    html += '<h3>种子</h3>';
    for (var i = 0; i < d.seeds.length; i++) {
      html += '<div class="pc-seed">'+esc(d.seeds[i])+'</div>';
    }
  }
  if (d.activities && d.activities.length) {
    html += '<h3>活动</h3>';
    for (var i = 0; i < d.activities.length; i++) {
      html += '<div class="pc-activity">'+esc(d.activities[i])+'</div>';
    }
  }
  el.innerHTML = html;
}

function renderMemPanel(d) {
  var el = document.getElementById('panel-mem'), html = '';
  html += '<h3>记忆库</h3>';
  html += '<div class="lib-row">';
  html += '<select id="mem-lib-sel" onchange="onLibraryChange(this.value)"></select>';
  html += '<button onclick="showLibraryDialog()">管理</button>';
  html += '</div>';
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
  el.innerHTML = html;
  // Trigger field viz render
  renderFieldViz(anchors, d.ecg ? d.ecg.field_tension : 0);
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
    addMessageToSession('assistant', '未知命令: ' + parts[0] + '。输入 /help 查看命令。');
    isStreaming = false; sendBtn.disabled = false; input.focus();
    return;
  }

  var time = new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'});
  s.messages.push({ role: 'user', content: text, time: time });
  updateSessionTitle(s);
  appendChatBubble('user', text, time);

  var time2 = new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'});
  s.messages.push({ role: 'assistant', content: '', time: time2, memoryCtx: null });
  var assistantDiv = appendChatBubble('assistant', '...', time2);
  assistantDiv.classList.add('typing');
  var contentEl = assistantDiv.querySelector('.content');
  contentEl.textContent = '...';

  var baseUrl = urlInput.value.replace(/\/$/, '');
  var apiMessages = s.messages.filter(function(m) { return m.role === 'user' || m.role === 'assistant'; }).map(function(m) {
    return { role: m.role, content: m.content };
  });
  try {
    var resp = await fetch(baseUrl + '/v1/chat/completions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Authorization': 'Bearer ' + apiKeyInput.value },
      body: JSON.stringify({ messages: apiMessages, model: modelSelect.value, stream: true }),
    });
    if (!resp.ok) throw new Error('HTTP ' + resp.status);
    assistantDiv.classList.remove('typing');
    var fullText = '', memoryCtx = null, reader = resp.body.getReader(), decoder = new TextDecoder(), buffer = '';
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
        fullText += data;
        contentEl.textContent = fullText;
        msgEl.scrollTop = msgEl.scrollHeight;
      }
    }
    var lastAssistant = s.messages[s.messages.length - 1];
    lastAssistant.content = fullText;
    lastAssistant.memoryCtx = memoryCtx;
    contentEl.textContent = fullText;
    if (memoryCtx) renderMemoryCtx(assistantDiv, memoryCtx);
    if (memoryCtx && memoryCtx.tool_invoked) fetchStatus();
  } catch (e) {
    assistantDiv.classList.remove('typing');
    contentEl.textContent = '错误: ' + e.message;
    var lastAssistant2 = s.messages[s.messages.length - 1];
    lastAssistant2.content = '错误: ' + e.message;
  }
  isStreaming = false; sendBtn.disabled = false; input.focus();
}

function appendChatBubble(role, content, time) {
  var div = document.createElement('div');
  div.className = 'message ' + role;
  var label = role === 'user' ? '你' : 'FM';
  div.innerHTML = '<div class="content">' + esc(content) + '</div>' +
    '<div class="meta">' + label + ' &middot; ' + (time || new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'})) + '</div>';
  msgEl.appendChild(div);
  msgEl.scrollTop = msgEl.scrollHeight;
  return div;
}

function addMessageToSession(role, content) {
  var s = getActiveSession();
  if (!s) return;
  var time = new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'});
  s.messages.push({ role: role, content: content, time: time });
  appendChatBubble(role, content, time);
  updateSessionTitle(s);
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
  s.messages.push({ role: 'user', content: '/' + (args.length > 0 ? 'seed ' + userDesc : 'seed'), time: time });
  appendChatBubble('user', '/' + (args.length > 0 ? 'seed ' + userDesc : 'seed'), time);
  updateSessionTitle(s);

  var time2 = new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'});
  s.messages.push({ role: 'assistant', content: '', time: time2, memoryCtx: null });
  var assistantDiv = appendChatBubble('assistant', '...', time2);
  assistantDiv.classList.add('typing');
  var contentEl = assistantDiv.querySelector('.content');
  contentEl.textContent = '...';

  var baseUrl = urlInput.value.replace(/\/$/, '');
  var apiMessages = [{ role: 'user', content: seedPrompt }];
  try {
    var resp = await fetch(baseUrl + '/v1/chat/completions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Authorization': 'Bearer ' + apiKeyInput.value },
      body: JSON.stringify({ messages: apiMessages, model: modelSelect.value, stream: true }),
    });
    if (!resp.ok) throw new Error('HTTP ' + resp.status);
    assistantDiv.classList.remove('typing');
    var fullText = '', memoryCtx = null, reader = resp.body.getReader(), decoder = new TextDecoder(), buffer = '';
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
        fullText += data;
        contentEl.textContent = fullText;
        msgEl.scrollTop = msgEl.scrollHeight;
      }
    }
    var lastAssistant = s.messages[s.messages.length - 1];
    lastAssistant.content = fullText;
    lastAssistant.memoryCtx = memoryCtx;
    contentEl.textContent = fullText;
    if (memoryCtx) renderMemoryCtx(assistantDiv, memoryCtx);
    if (memoryCtx && memoryCtx.tool_invoked) fetchStatus();
  } catch (e) {
    assistantDiv.classList.remove('typing');
    contentEl.textContent = '错误: ' + e.message;
    var lastAssistant2 = s.messages[s.messages.length - 1];
    lastAssistant2.content = '错误: ' + e.message;
  }
  isStreaming = false; sendBtn.disabled = false; input.focus();
}

async function handleQueryCommand(query, mode) {
  if (!query) {
    addMessageToSession('assistant', '用法: /' + mode + ' <查询文本>');
    return;
  }
  var msg = appendChatBubble('assistant', '正在查询...', new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'}));
  msg.querySelector('.content').textContent = mode + ': ' + query;
  try {
    var resp = await fetch('/api/memory/query', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ query: query, mode: mode, top_k: 5 }),
    });
    var data = await resp.json();
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
  } catch (e) {
    msg.querySelector('.content').textContent = '查询失败: ' + e.message;
    var s2 = getActiveSession();
    if (s2) s2.messages.push({ role: 'assistant', content: '查询失败: ' + e.message, time: new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'}) });
  }
}

async function showStatusInline() {
  try {
    var resp = await fetch('/api/memory/status');
    var data = await resp.json();
    var anchors = data.anchors || [];
    var lines = [];
    lines.push('库: ' + currentLibrary);
    lines.push('锚点: ' + data.anchors_count + ' | 事件: ' + data.events_count + ' | 痕迹: ' + data.traces_count + ' | 种子: ' + data.seeds_count);
    if (data.ecg) lines.push('张力: ' + data.ecg.field_tension.toFixed(4));
    for (var i = 0; i < Math.min(anchors.length, 8); i++) {
      lines.push('  ' + anchors[i].label + ' (d=' + anchors[i].density + ')');
    }
    addMessageToSession('assistant', lines.join('\n'));
  } catch (e) { addMessageToSession('assistant', '获取状态失败: ' + e.message); }
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
  addMessageToSession('assistant', h);
}

async function saveMem() {
  try {
    var resp = await fetch('/api/memory/save', { method: 'POST' });
    var data = await resp.json();
    addMessageToSession('assistant', data.ok ? '记忆已保存到磁盘。' : '保存失败：' + (data.error || '未知错误'));
    fetchStatus();
  } catch(e) { addMessageToSession('assistant', '保存失败：' + e.message); }
}

async function loadMem() {
  try {
    var resp = await fetch('/api/memory/load', { method: 'POST' });
    var data = await resp.json();
    if (data.ok) addMessageToSession('assistant', '已读取 ' + data.anchors_count + ' 锚点、' + data.events_count + ' 事件。');
    else addMessageToSession('assistant', '读取失败：' + (data.error || ''));
    fetchStatus();
  } catch(e) {}
}

// ── Init ──
document.addEventListener('DOMContentLoaded', function() {
  msgEl = document.getElementById('messages');
  input = document.getElementById('user-input');
  sendBtn = document.getElementById('send-btn');
  modelSelect = document.getElementById('model-select');
  apiKeyInput = document.getElementById('api-key');
  urlInput = document.getElementById('backend-url');

  input.addEventListener('keydown', handleKey);
  input.addEventListener('input', onInputChange);

  initRailToggles();
  createSession();
  loadLibraryList();
  startPolling();
});
