import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';

// ── Constants ─────────────────────────────────────
const LAYER_COLORS = {
  L1: 0xff7a5c,
  L2: 0xffb454,
  L3: 0x5fcdd9,
  L4: 0xa48cf2,
};
const SPHERE_RADIUS = 5.2;
const ANCHOR_BASE_SIZE = 0.08;
const ANCHOR_SIZE_K = 0.07;

// ── State ─────────────────────────────────────────
const state = {
  anchors: [],            // {id, label, density, stiffness, damping, direction_n}
  selectedId: null,
  hoverId: null,
  // Filters
  projMode: 'pca',
  visibleLayers: { L1: true, L2: true, L3: true, L4: true },
  simThreshold: 0.30,
  densityMin: 0,
  densityMax: 60,
  showLabels: true,
  showConnections: true,
  showGrid: false,
  showStars: false,
};

// ── Three.js setup ────────────────────────────────
const canvas = document.getElementById('canvas');
const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
// Compute canvas size; Three.js setSize(w,h,true) writes both width/height attrs and CSS px
function computeCanvasSize() {
  const leftPanel = document.getElementById('panel-left-field');
  const rightPanel = document.getElementById('panel-right-field');
  const railW = 44; // rail is always visible
  const leftW = leftPanel && !leftPanel.classList.contains('collapsed') ? 232 : 0;
  const rightW = rightPanel && !rightPanel.classList.contains('collapsed') ? 304 : 0;
  return {
    w: Math.max(100, window.innerWidth - railW - leftW - rightW),
    h: Math.max(100, window.innerHeight - 52 - 36),
  };
}
{
  const s = computeCanvasSize();
  renderer.setSize(s.w, s.h, true);
}
renderer.outputColorSpace = THREE.SRGBColorSpace;
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.toneMappingExposure = 1.05;

const scene = new THREE.Scene();
scene.fog = new THREE.FogExp2(0x050810, 0.035);

const camera = new THREE.PerspectiveCamera(45, 1, 0.1, 100);
camera.position.set(0, 1.8, 9.5);

const controls = new OrbitControls(camera, canvas);
controls.enableDamping = true;
controls.dampingFactor = 0.08;
controls.rotateSpeed = 0.8;
controls.zoomSpeed = 0.7;
controls.panSpeed = 0.6;
controls.minDistance = 4;
controls.maxDistance = 24;
controls.target.set(0, 0, 0);

// ── Lights ────────────────────────────────────────
const ambient = new THREE.AmbientLight(0xb0c4e8, 0.35);
scene.add(ambient);
const key = new THREE.DirectionalLight(0xffffff, 0.6);
key.position.set(5, 8, 7);
scene.add(key);
const fill = new THREE.PointLight(0x6a8cc4, 0.8, 30);
fill.position.set(-8, -4, 5);
scene.add(fill);

// ── Background: subtle gradient sphere + stars ────
const bgGeo = new THREE.SphereGeometry(40, 32, 16);
const bgMat = new THREE.ShaderMaterial({
  side: THREE.BackSide,
  uniforms: { topColor: { value: new THREE.Color(0x0a1428) }, bottomColor: { value: new THREE.Color(0x050810) } },
  vertexShader: `varying vec3 vPos; void main(){ vPos = position; gl_Position = projectionMatrix * modelViewMatrix * vec4(position,1.0); }`,
  fragmentShader: `varying vec3 vPos; uniform vec3 topColor; uniform vec3 bottomColor; void main(){ float t = (vPos.y + 30.0) / 60.0; t = clamp(t, 0.0, 1.0); gl_FragColor = vec4(mix(bottomColor, topColor, t), 1.0); }`,
});
const bg = new THREE.Mesh(bgGeo, bgMat);
scene.add(bg);

// Stars (background points)
function makeStars(count = 800) {
  const geom = new THREE.BufferGeometry();
  const positions = new Float32Array(count * 3);
  const sizes = new Float32Array(count);
  for (let i = 0; i < count; i++) {
    // Distribute on a large sphere
    const r = 20 + Math.random() * 15;
    const theta = Math.random() * Math.PI * 2;
    const phi = Math.acos(2 * Math.random() - 1);
    positions[i*3+0] = r * Math.sin(phi) * Math.cos(theta);
    positions[i*3+1] = r * Math.sin(phi) * Math.sin(theta);
    positions[i*3+2] = r * Math.cos(phi);
    sizes[i] = Math.random() * 0.04 + 0.01;
  }
  geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  geom.setAttribute('size', new THREE.BufferAttribute(sizes, 1));
  const mat = new THREE.ShaderMaterial({
    transparent: true,
    depthWrite: false,
    uniforms: { time: { value: 0 } },
    vertexShader: `attribute float size; varying float vSize; void main(){ vSize = size; vec4 mv = modelViewMatrix * vec4(position,1.0); gl_PointSize = size * 300.0 / -mv.z; gl_Position = projectionMatrix * mv; }`,
    fragmentShader: `varying float vSize; uniform float time; void main(){ vec2 c = gl_PointCoord - 0.5; float d = length(c); if (d > 0.5) discard; float a = smoothstep(0.5, 0.0, d) * (0.5 + 0.5 * sin(time * 0.5 + vSize * 100.0)); gl_FragColor = vec4(0.9, 0.95, 1.0, a * 0.7); }`,
  });
  return new THREE.Points(geom, mat);
}
let stars = makeStars(800);
scene.add(stars);

// ── Reference grid (toggleable) ───────────────────
const gridGroup = new THREE.Group();
{
  const size = SPHERE_RADIUS * 3.2;
  const divisions = 16;
  const colors = [0x3a4a6a, 0x3a4a6a, 0x3a4a6a].map(c => new THREE.Color(c).multiplyScalar(0.25));
  const gridXY = new THREE.GridHelper(size, divisions, colors[0], colors[0]);
  gridXY.rotation.x = 0;
  const gridXZ = new THREE.GridHelper(size, divisions, colors[1], colors[1]);
  gridXZ.rotation.x = Math.PI / 2;
  const gridYZ = new THREE.GridHelper(size, divisions, colors[2], colors[2]);
  gridYZ.rotation.z = Math.PI / 2;
  [gridXY, gridXZ, gridYZ].forEach(g => { g.material.transparent = true; g.material.opacity = 0.35; });
  gridGroup.add(gridXY, gridXZ, gridYZ);
  // Central reference sphere
  const refGeo = new THREE.SphereGeometry(SPHERE_RADIUS, 32, 16);
  const refMat = new THREE.MeshBasicMaterial({ color: 0x3a4a6a, wireframe: true, transparent: true, opacity: 0.08 });
  gridGroup.add(new THREE.Mesh(refGeo, refMat));
  gridGroup.visible = false;
}
scene.add(gridGroup);

// ── Halo sprite texture (radial gradient) ────────
function makeHaloTexture() {
  const size = 128;
  const cv = document.createElement('canvas');
  cv.width = cv.height = size;
  const ctx = cv.getContext('2d');
  const grad = ctx.createRadialGradient(size/2, size/2, 0, size/2, size/2, size/2);
  grad.addColorStop(0, 'rgba(255,255,255,1)');
  grad.addColorStop(0.2, 'rgba(255,255,255,0.6)');
  grad.addColorStop(0.5, 'rgba(255,255,255,0.15)');
  grad.addColorStop(1, 'rgba(255,255,255,0)');
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, size, size);
  const tex = new THREE.CanvasTexture(cv);
  tex.colorSpace = THREE.SRGBColorSpace;
  return tex;
}
const HALO_TEX = makeHaloTexture();

// ── Anchor pool (efficient reuse) ─────────────────
class AnchorNode {
  constructor(a) {
    this.data = a;
    const layer = layerOf(a.density);
    const color = LAYER_COLORS[layer];
    this.layer = layer;
    this.color = color;
    const size = ANCHOR_BASE_SIZE + Math.sqrt(a.density) * ANCHOR_SIZE_K;

    // Core sphere
    const geo = new THREE.SphereGeometry(size, 24, 18);
    const mat = new THREE.MeshStandardMaterial({
      color, emissive: color, emissiveIntensity: 0.7,
      roughness: 0.4, metalness: 0.1,
    });
    this.mesh = new THREE.Mesh(geo, mat);
    this.mesh.userData.anchorId = a.id;

    // Halo sprite
    const haloMat = new THREE.SpriteMaterial({
      map: HALO_TEX, color, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending,
    });
    this.halo = new THREE.Sprite(haloMat);
    this.halo.scale.set(size * 4.5, size * 4.5, 1);
    this.halo.userData.anchorId = a.id;

    // Label (CSS2D would be ideal; use a Sprite with canvas texture for portability)
    this.label = makeLabelSprite(a.label, layer);
    this.label.position.y = size + 0.18;
    this.label.visible = state.showLabels;

    // Group all together
    this.group = new THREE.Group();
    this.group.add(this.mesh, this.halo, this.label);
    this.baseScale = 1.0;
    this.targetScale = 1.0;
  }
  setScale(s) { this.targetScale = s; }
  setHighlight(on) {
    this.mesh.material.emissiveIntensity = on ? 1.6 : 0.7;
    this.halo.material.opacity = on ? 1.0 : 0.5;
  }
  setDimmed(d) {
    this.mesh.material.opacity = d ? 0.18 : 1.0;
    this.halo.material.opacity = d ? 0.08 : (this.targetScale > 1.2 ? 1.0 : 0.5);
    this.mesh.material.transparent = d;
    this.halo.material.transparent = true;
  }
  setPosition(x, y, z) { this.group.position.set(x, y, z); }
  tick() {
    // Smooth scale animation
    const cur = this.group.scale.x;
    const next = cur + (this.targetScale - cur) * 0.18;
    this.group.scale.setScalar(next);
  }
  dispose() {
    this.mesh.geometry.dispose();
    this.mesh.material.dispose();
    this.halo.material.dispose();
    if (this.label.material.map) this.label.material.map.dispose();
    this.label.material.dispose();
  }
}

function makeLabelSprite(text, layer) {
  const cv = document.createElement('canvas');
  const ctx = cv.getContext('2d');
  cv.width = 512; cv.height = 96;
  ctx.font = '500 30px -apple-system, "SF Pro Text", system-ui, sans-serif';
  ctx.textBaseline = 'middle';
  ctx.textAlign = 'center';
  ctx.fillStyle = 'rgba(232, 236, 244, 0.95)';
  // Subtle text shadow for legibility
  ctx.shadowColor = 'rgba(0,0,0,0.85)';
  ctx.shadowBlur = 8;
  ctx.fillText(text, cv.width / 2, cv.height / 2);
  const tex = new THREE.CanvasTexture(cv);
  tex.colorSpace = THREE.SRGBColorSpace;
  tex.minFilter = THREE.LinearFilter;
  const mat = new THREE.SpriteMaterial({ map: tex, transparent: true, depthTest: false });
  const sprite = new THREE.Sprite(mat);
  sprite.scale.set(1.8, 0.34, 1);
  return sprite;
}

const anchorGroup = new THREE.Group();
scene.add(anchorGroup);
const nodesById = new Map(); // id -> AnchorNode

// ── Connection lines (dynamic, threshold-controlled)
let connMesh = null;
function rebuildConnections() {
  if (connMesh) {
    anchorGroup.remove(connMesh);
    connMesh.geometry.dispose();
    connMesh.material.dispose();
    connMesh = null;
  }
  if (!state.showConnections) return;
  const visible = Array.from(nodesById.values()).filter(n =>
    state.visibleLayers[n.layer] &&
    n.data.density >= state.densityMin &&
    n.data.density <= state.densityMax
  );
  const positions = [];
  const colors = [];
  for (let i = 0; i < visible.length; i++) {
    for (let j = i + 1; j < visible.length; j++) {
      const a = visible[i].data, b = visible[j].data;
      const sim = cosSim(a.direction_n, b.direction_n);
      if (sim < state.simThreshold) continue;
      const p1 = visible[i].group.position;
      const p2 = visible[j].group.position;
      positions.push(p1.x, p1.y, p1.z, p2.x, p2.y, p2.z);
      const c1 = new THREE.Color(visible[i].color);
      const c2 = new THREE.Color(visible[j].color);
      colors.push(c1.r, c1.g, c1.b, c2.r, c2.g, c2.b);
    }
  }
  if (positions.length === 0) return;
  const geom = new THREE.BufferGeometry();
  geom.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geom.setAttribute('color', new THREE.Float32BufferAttribute(colors, 3));
  const mat = new THREE.LineBasicMaterial({
    vertexColors: true, transparent: true, opacity: 0.55, depthWrite: false, blending: THREE.AdditiveBlending,
  });
  connMesh = new THREE.LineSegments(geom, mat);
  anchorGroup.add(connMesh);
}

// ── Math helpers ──────────────────────────────────
function cosSim(a, b) {
  let dot = 0, na = 0, nb = 0;
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) { dot += a[i]*b[i]; na += a[i]*a[i]; nb += b[i]*b[i]; }
  const den = Math.sqrt(na) * Math.sqrt(nb);
  return den < 1e-9 ? 0 : dot / den;
}
function layerOf(d) { return d > 15 ? 'L1' : d > 8 ? 'L2' : d > 3 ? 'L3' : 'L4'; }

// ── PCA projection (32-dim → 3-dim) ───────────────
function pca3(vectors) {
  const n = vectors.length;
  if (n === 0) return { positions: [], basis: [[1,0,0],[0,1,0],[0,0,1]] };
  const dim = vectors[0].length;
  // Center
  const mean = new Array(dim).fill(0);
  for (const v of vectors) for (let i = 0; i < dim; i++) mean[i] += v[i] / n;
  const centered = vectors.map(v => v.map((x, i) => x - mean[i]));
  // Covariance (dim x dim) — full matrix
  const cov = Array.from({length: dim}, () => new Array(dim).fill(0));
  for (const c of centered) {
    for (let i = 0; i < dim; i++)
      for (let j = i; j < dim; j++)
        cov[i][j] += c[i] * c[j] / n;
  }
  for (let i = 0; i < dim; i++)
    for (let j = 0; j < i; j++) cov[i][j] = cov[j][i];

  // Find top-3 eigenvectors via power iteration with deflation
  function powerIter(A, numIter = 60) {
    const m = A.length;
    let v = Array.from({length: m}, () => Math.random() - 0.5);
    let norm = Math.sqrt(v.reduce((s, x) => s + x*x, 0));
    v = v.map(x => x / norm);
    for (let it = 0; it < numIter; it++) {
      const Av = A.map(row => row.reduce((s, Aij, j) => s + Aij * v[j], 0));
      const newNorm = Math.sqrt(Av.reduce((s, x) => s + x*x, 0));
      if (newNorm < 1e-9) break;
      v = Av.map(x => x / newNorm);
    }
    return v;
  }
  const eigvals = [];
  const eigvecs = [];
  let A = cov.map(r => r.slice());
  for (let k = 0; k < 3; k++) {
    const v = powerIter(A, 80);
    // Rayleigh quotient for eigenvalue
    const Av = A.map(row => row.reduce((s, Aij, j) => s + Aij * v[j], 0));
    let ev = 0; for (let i = 0; i < v.length; i++) ev += v[i] * Av[i];
    eigvals.push(ev);
    eigvecs.push(v);
    // Deflate
    for (let i = 0; i < A.length; i++)
      for (let j = 0; j < A.length; j++)
        A[i][j] -= ev * v[i] * v[j];
  }

  // Project: positions = centered · eigvecs.T (3 dims per vector)
  const positions = centered.map(c => {
    const p = [];
    for (const ev of eigvecs) {
      let s = 0; for (let i = 0; i < dim; i++) s += c[i] * ev[i];
      p.push(s);
    }
    return p;
  });

  // Normalize to fit in a unit sphere, then scale to SPHERE_RADIUS
  let maxAbs = 0;
  for (const p of positions) for (const v of p) maxAbs = Math.max(maxAbs, Math.abs(v));
  if (maxAbs > 0) for (const p of positions) for (let i = 0; i < 3; i++) p[i] = p[i] / maxAbs * SPHERE_RADIUS;

  return { positions, eigvals, eigvecs };
}

function first3Project(vectors) {
  // Use first 3 components, normalized to unit sphere
  const positions = vectors.map(v => {
    const x = v[0] || 0, y = v[1] || 0, z = v[2] || 0;
    const len = Math.sqrt(x*x + y*y + z*z) || 1;
    return [x/len * SPHERE_RADIUS, y/len * SPHERE_RADIUS, z/len * SPHERE_RADIUS];
  });
  return { positions };
}

// ── Layout anchors into 3D ────────────────────────
function layoutAnchors(anchors) {
  if (anchors.length === 0) {
    // Clear existing
    for (const n of nodesById.values()) { anchorGroup.remove(n.group); n.dispose(); }
    nodesById.clear();
    rebuildConnections();
    return;
  }
  const vectors = anchors.map(a => a.direction_n);
  const { positions } = state.projMode === 'pca' ? pca3(vectors) : first3Project(vectors);

  // Add/update nodes
  const newIds = new Set();
  for (let i = 0; i < anchors.length; i++) {
    const a = anchors[i];
    newIds.add(a.id);
    const [x, y, z] = positions[i];
    if (nodesById.has(a.id)) {
      const n = nodesById.get(a.id);
      // Update data
      n.data = a;
      const size = ANCHOR_BASE_SIZE + Math.sqrt(a.density) * ANCHOR_SIZE_K;
      n.mesh.geometry.dispose();
      n.mesh.geometry = new THREE.SphereGeometry(size, 24, 18);
      n.mesh.material.color.setHex(n.color);
      n.mesh.material.emissive.setHex(n.color);
      n.halo.scale.set(size * 4.5, size * 4.5, 1);
      n.setPosition(x, y, z);
    } else {
      const n = new AnchorNode(a);
      n.setPosition(x, y, z);
      nodesById.set(a.id, n);
      anchorGroup.add(n.group);
    }
  }
  // Remove stale
  for (const [id, n] of nodesById) {
    if (!newIds.has(id)) {
      anchorGroup.remove(n.group);
      n.dispose();
      nodesById.delete(id);
    }
  }
  rebuildConnections();
  updateLayerCounts();
  applyFilters(); // re-apply visible layers
}

function applyFilters() {
  for (const n of nodesById.values()) {
    const ok = state.visibleLayers[n.layer] &&
               n.data.density >= state.densityMin &&
               n.data.density <= state.densityMax;
    n.group.visible = ok;
    n.label.visible = ok && state.showLabels;
  }
  rebuildConnections();
  applyFocus();
  updateHud();
}

// When something is hovered/selected, dim the rest and show the focus label.
function applyFocus() {
  const focusId = state.selectedId || state.hoverId;
  const showFocusLabel = !!focusId; // always show the focused anchor's label
  for (const [id, n] of nodesById) {
    if (!n.group.visible) continue;
    if (focusId == null) {
      n.setDimmed(false);
      n.label.visible = n.group.visible && state.showLabels;
    } else {
      const isFocus = id === focusId;
      n.setDimmed(!isFocus);
      // Focused anchor always shows its label; non-focused obey the master toggle
      n.label.visible = isFocus ? true : (n.group.visible && state.showLabels);
    }
  }
}

function updateLayerCounts() {
  const counts = { L1: 0, L2: 0, L3: 0, L4: 0 };
  for (const a of state.anchors) counts[layerOf(a.density)]++;
  document.querySelectorAll('.layer-row').forEach(r => {
    const L = r.dataset.layer;
    r.querySelector('.layer-count').textContent = counts[L] || 0;
  });
}

function updateHud() {
  const visible = Array.from(nodesById.values()).filter(n => n.group.visible);
  let lines = 0;
  if (connMesh) lines = connMesh.geometry.attributes.position.count / 2;
  document.getElementById('hud-vis').textContent = `${visible.length} 锚点 / ${lines} 连线`;
  document.getElementById('hud-cam').textContent =
    `${camera.position.x.toFixed(1)}, ${camera.position.y.toFixed(1)}, ${camera.position.z.toFixed(1)}`;
}

// ── Data fetch ────────────────────────────────────
async function fetchStatus() {
  try {
    const [statusResp, libResp] = await Promise.all([
      fetch('/api/memory/status'),
      fetch('/api/memory/libraries'),
    ]);
    if (!statusResp.ok) throw new Error('status');
    const data = await statusResp.json();
    const libs = libResp.ok ? await libResp.json() : { libraries: [], active: 'default' };
    state.anchors = (data.anchors || []).map(a => ({
      id: a.id,
      label: a.label,
      density: a.density,
      stiffness: a.stiffness,
      damping: a.damping,
      direction_n: a.direction_n,
    }));
    layoutAnchors(state.anchors);
    // Stats
    document.getElementById('stat-anchors').textContent = state.anchors.length;
    document.getElementById('stat-events').textContent = data.events_count || 0;
    document.getElementById('stat-tension').textContent =
      data.ecg ? data.ecg.field_tension.toFixed(2) : '—';
    document.getElementById('stat-lib').textContent = libs.active || 'default';
  } catch (e) {
    console.warn('status fetch failed', e);
  }
}

// ── Picking (raycaster) ───────────────────────────
const raycaster = new THREE.Raycaster();
const mouse = new THREE.Vector2();
const pickableMeshes = () => {
  const arr = [];
  for (const n of nodesById.values()) if (n.group.visible) arr.push(n.mesh);
  return arr;
};

canvas.addEventListener('pointermove', (e) => {
  const rect = canvas.getBoundingClientRect();
  mouse.x = ((e.clientX - rect.left) / rect.width) * 2 - 1;
  mouse.y = -((e.clientY - rect.top) / rect.height) * 2 + 1;
  raycaster.setFromCamera(mouse, camera);
  const hits = raycaster.intersectObjects(pickableMeshes(), false);
  const newHover = hits.length > 0 ? hits[0].object.userData.anchorId : null;
  if (newHover !== state.hoverId) {
    if (state.hoverId) {
      const prev = nodesById.get(state.hoverId);
      if (prev) prev.setHighlight(false);
    }
    state.hoverId = newHover;
    if (newHover) {
      const cur = nodesById.get(newHover);
      if (cur) cur.setHighlight(true);
      canvas.style.cursor = 'pointer';
    } else {
      canvas.style.cursor = 'grab';
    }
    applyFocus();
  }
});
canvas.addEventListener('mousemove', (e) => {
  const rect = canvas.getBoundingClientRect();
  mouse.x = ((e.clientX - rect.left) / rect.width) * 2 - 1;
  mouse.y = -((e.clientY - rect.top) / rect.height) * 2 + 1;
  raycaster.setFromCamera(mouse, camera);
  const hits = raycaster.intersectObjects(pickableMeshes(), false);
  const newHover = hits.length > 0 ? hits[0].object.userData.anchorId : null;
  if (newHover !== state.hoverId) {
    if (state.hoverId) {
      const prev = nodesById.get(state.hoverId);
      if (prev) prev.setHighlight(false);
    }
    state.hoverId = newHover;
    if (newHover) {
      const cur = nodesById.get(newHover);
      if (cur) cur.setHighlight(true);
      canvas.style.cursor = 'pointer';
    } else {
      canvas.style.cursor = 'grab';
    }
  }
  applyFocus();
});

canvas.addEventListener('click', () => {
  if (state.hoverId) {
    selectAnchor(state.hoverId);
  } else if (state.selectedId) {
    clearSelection();
  }
});

function selectAnchor(id) {
  if (state.selectedId === id) return clearSelection();
  if (state.selectedId) {
    const prev = nodesById.get(state.selectedId);
    if (prev) { prev.setHighlight(false); prev.setScale(1.0); }
  }
  state.selectedId = id;
  const n = nodesById.get(id);
  if (n) { n.setHighlight(true); n.setScale(1.4); }
  applyFocus();
  renderDetail();
  if (n) {
    // Smoothly fly to the anchor
    const target = n.group.position.clone();
    const offset = new THREE.Vector3().subVectors(camera.position, controls.target).normalize();
    const newCam = target.clone().add(offset.multiplyScalar(8));
    animateCamera(newCam, target, 0.55);
  }
}

function clearSelection() {
  if (state.selectedId) {
    const prev = nodesById.get(state.selectedId);
    if (prev) { prev.setHighlight(false); prev.setScale(1.0); }
  }
  state.selectedId = null;
  applyFocus();
  renderDetail();
}

function animateCamera(toPos, toTarget, duration = 0.5) {
  const fromPos = camera.position.clone();
  const fromTarget = controls.target.clone();
  const t0 = performance.now();
  function step() {
    const t = Math.min((performance.now() - t0) / 1000 / duration, 1);
    const e = t < 0.5 ? 2*t*t : 1 - Math.pow(-2*t+2, 2)/2; // easeInOut
    camera.position.lerpVectors(fromPos, toPos, e);
    controls.target.lerpVectors(fromTarget, toTarget, e);
    controls.update();
    if (t < 1) requestAnimationFrame(step);
  }
  step();
}

// ── Detail panel ──────────────────────────────────
function renderDetail() {
  const body = document.getElementById('detail-body');
  if (!state.selectedId) {
    body.innerHTML = `<div class="detail-empty">
      <div class="ico"></div>
      <div class="msg">点击画布中的锚点<br>查看其参数与关联</div>
    </div>`;
    return;
  }
  const a = state.anchors.find(x => x.id === state.selectedId);
  if (!a) return;
  const layer = layerOf(a.density);
  const layerColor = '#' + LAYER_COLORS[layer].toString(16).padStart(6, '0');
  // Related anchors by similarity
  const related = state.anchors
    .filter(x => x.id !== a.id)
    .map(x => ({ a: x, sim: cosSim(a.direction_n, x.direction_n) }))
    .sort((x, y) => y.sim - x.sim)
    .slice(0, 8);
  const relHtml = related.length ? related.map(r => {
    const c = '#' + LAYER_COLORS[layerOf(r.a.density)].toString(16).padStart(6, '0');
    return `<div class="related-item" data-id="${r.a.id}">
      <span class="dot" style="background:${c}; box-shadow:0 0 6px ${c}"></span>
      <span class="lbl">${escapeHtml(r.a.label)}</span>
      <span class="bar"><span class="fill" style="width:${Math.max(0, r.sim) * 100}%"></span></span>
      <span class="sim">${r.sim.toFixed(2)}</span>
    </div>`;
  }).join('') : '<div style="color: var(--text-faint); font-size: 11px; padding: 4px 0">无关联</div>';
  body.innerHTML = `<div class="detail-card">
    <div>
      <div class="detail-name">${escapeHtml(a.label)}</div>
      <div style="margin-top:6px">
        <span class="detail-tag" style="color:${layerColor}">${layer}</span>
        <span class="detail-tag" style="margin-left:4px">d = ${a.density}</span>
      </div>
    </div>
    <div class="detail-meta">
      <div class="row"><span class="k">id</span><span class="v" style="font-size:10px">${a.id}</span></div>
      <div class="row"><span class="k">density</span><span class="v">${a.density}</span></div>
      <div class="row"><span class="k">stiffness</span><span class="v">${a.stiffness.toFixed(3)}</span></div>
      <div class="row"><span class="k">damping</span><span class="v">${a.damping.toFixed(3)}</span></div>
      <div class="row"><span class="k">‖d‖</span><span class="v">${Math.sqrt(a.direction_n.reduce((s,x)=>s+x*x,0)).toFixed(3)}</span></div>
      <div class="row"><span class="k">d[0..3]</span><span class="v" style="font-size:10px">${a.direction_n.slice(0,3).map(x => x.toFixed(2)).join(', ')}</span></div>
    </div>
    <div>
      <h3 style="font-size:10px; font-weight:600; letter-spacing:0.8px; text-transform:uppercase; color: var(--text-faint); margin-bottom:8px">相关锚点 · 相似度</h3>
      <div class="related">${relHtml}</div>
    </div>
  </div>`;
  // Bind related-item clicks
  body.querySelectorAll('.related-item').forEach(el => {
    el.addEventListener('click', () => selectAnchor(el.dataset.id));
  });
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
}

// ── UI bindings ───────────────────────────────────
function bindToggle(id, key, onChange) {
  const el = document.getElementById(id);
  el.addEventListener('click', () => {
    el.classList.toggle('on');
    state[key] = el.classList.contains('on');
    onChange();
  });
}

document.getElementById('btn-reset-cam').addEventListener('click', () => {
  animateCamera(new THREE.Vector3(0, 2.5, 12), new THREE.Vector3(0, 0, 0), 0.6);
});

// Layer toggles
document.querySelectorAll('.layer-row').forEach(row => {
  const input = row.querySelector('input');
  row.addEventListener('click', (e) => {
    if (e.target.tagName === 'INPUT') return;
    e.preventDefault();
    input.checked = !input.checked;
    row.classList.toggle('off', !input.checked);
    state.visibleLayers[row.dataset.layer] = input.checked;
    applyFilters();
  });
  input.addEventListener('change', () => {
    row.classList.toggle('off', !input.checked);
    state.visibleLayers[row.dataset.layer] = input.checked;
    applyFilters();
  });
});

// Projection mode
document.querySelectorAll('#proj-mode .mode-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    document.querySelectorAll('#proj-mode .mode-btn').forEach(b => b.classList.remove('active'));
    btn.classList.add('active');
    state.projMode = btn.dataset.mode;
    layoutAnchors(state.anchors);
    showToast(state.projMode === 'pca' ? 'PCA 投影 · 32→3 主成分' : '首 3 维 · 归一化到球面');
  });
});

// Sliders
const simSlider = document.getElementById('sim-slider');
const simVal = document.getElementById('sim-val');
simSlider.addEventListener('input', () => {
  state.simThreshold = parseFloat(simSlider.value);
  simVal.textContent = state.simThreshold.toFixed(2);
  rebuildConnections();
  updateHud();
});
const dminSlider = document.getElementById('dmin-slider');
const dminVal = document.getElementById('dmin-val');
const dmaxSlider = document.getElementById('dmax-slider');
const dmaxVal = document.getElementById('dmax-val');
function syncDensity() {
  let a = parseInt(dminSlider.value), b = parseInt(dmaxSlider.value);
  if (a > b) { a = b; dminSlider.value = a; }
  state.densityMin = a; state.densityMax = b;
  dminVal.textContent = a; dmaxVal.textContent = b;
  applyFilters();
}
dminSlider.addEventListener('input', syncDensity);
dmaxSlider.addEventListener('input', syncDensity);

bindToggle('t-labels', 'showLabels', () => {
  for (const n of nodesById.values()) n.label.visible = n.group.visible && state.showLabels;
});
bindToggle('t-connections', 'showConnections', () => { rebuildConnections(); updateHud(); });
bindToggle('t-grid', 'showGrid', () => { gridGroup.visible = state.showGrid; });
bindToggle('t-stars', 'showStars', () => { stars.visible = state.showStars; });

// Toast
function showToast(text) {
  const wrap = document.querySelector('.canvas-wrap');
  const t = document.createElement('div');
  t.className = 'toast';
  t.textContent = text;
  wrap.appendChild(t);
  setTimeout(() => t.remove(), 2400);
}

// ── Resize ────────────────────────────────────────
function resize() {
  const { w, h } = computeCanvasSize();
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
  renderer.setSize(w, h, true);
}
window.addEventListener('resize', () => { resize(); });
resize();

// ── Animation loop ────────────────────────────────
let lastT = performance.now();
let frames = 0, fpsAcc = 0, fpsLast = lastT;
const hudFps = document.getElementById('hud-fps');

function animate() {
  requestAnimationFrame(animate);
  const t = performance.now();
  const dt = (t - lastT) / 1000;
  lastT = t;
  // FPS
  frames++;
  fpsAcc += dt;
  if (t - fpsLast > 500) {
    const fps = (frames / fpsAcc) || 0;
    hudFps.textContent = fps.toFixed(0);
    frames = 0; fpsAcc = 0; fpsLast = t;
  }
  controls.update();
  // (auto-rotation disabled — camera is controlled by user via OrbitControls)
  // Smooth scale tween for all anchor nodes
  for (const n of nodesById.values()) n.tick();
  // Stars twinkle
  stars.material.uniforms.time.value = t * 0.001;
  renderer.render(scene, camera);
  updateHud();
}
animate();

// ── Initial fetch + polling ───────────────────────
fetchStatus();
setInterval(fetchStatus, 2000);

// Debug hook (exposes state for browser console / future tests)
window.__fieldViz = { state, nodesById, selectAnchor, clearSelection, applyFocus, camera, controls };

// Surface any uncaught errors visibly (helpful when something in the CDN fails)
window.addEventListener('error', (e) => {
  console.error('runtime error:', e.message);
  const body = document.getElementById('detail-body');
  if (body) body.innerHTML = `<div class="detail-empty"><div class="ico"></div><div class="msg" style="color:#ff7a5c">运行时错误<br><code style="font-size:10px">${escapeHtml(e.message)}</code></div></div>`;
});

// ── Rail panel toggle ──────────────────────────────
function initPanelToggles() {
  const panelConfigs = [
    { id: 'panel-left-field', storageKey: 'field-panelLeft', dir: 'left' },
    { id: 'panel-right-field', storageKey: 'field-panelRight', dir: 'right' },
  ];

  // Apply initial collapsed states
  panelConfigs.forEach(cfg => {
    const panel = document.getElementById(cfg.id);
    if (!panel) return;
    const isOpen = sessionStorage.getItem(cfg.storageKey) !== 'collapsed';
    if (!isOpen) panel.classList.add('collapsed');
  });

  // Bind rail buttons
  document.querySelectorAll('#rail .rail-btn').forEach(btn => {
    const panelId = btn.getAttribute('data-panel');
    const panel = document.getElementById(panelId);
    if (!panel) return;

    const storageKey = panelConfigs.find(c => c.id === panelId)?.storageKey;
    const isOpen = !panel.classList.contains('collapsed');
    if (isOpen) btn.classList.add('active');

    btn.addEventListener('click', () => {
      const nowCollapsed = panel.classList.toggle('collapsed');
      if (storageKey) sessionStorage.setItem(storageKey, nowCollapsed ? 'collapsed' : 'expanded');
      btn.classList.toggle('active', !nowCollapsed);
      // Recompute canvas size after panel toggle
      setTimeout(resize, 280);
    });
  });
}

// Init panel toggles on load
initPanelToggles();
