'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';

interface AnchorData {
  label: string;
  density: number;
  stiffness?: number;
  damping?: number;
  direction_n: number[];
}

// Project high-D direction to 3D (use first 3 components, or pad)
function project3D(dir: number[]): [number, number, number] {
  return [
    dir[0] || 0,
    dir[1] || 0,
    dir[2] || 0,
  ];
}

const LAYER_COLORS: Record<string, number> = {
  L1: 0xff7a5c,
  L2: 0xffb454,
  L3: 0x5fcdd9,
  L4: 0xa48cf2,
};

const ANCHOR_BASE_SIZE = 0.08;
const ANCHOR_SIZE_K = 0.07;
const LARGE_FIELD_THRESHOLD = 50;

interface FieldCanvasProps {
  anchors: AnchorData[];
  tension: number;
}

export default function FieldCanvas({ anchors, tension }: FieldCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const rendererRef = useRef<THREE.WebGLRenderer | null>(null);
  const sceneRef = useRef<THREE.Scene | null>(null);
  const cameraRef = useRef<THREE.PerspectiveCamera | null>(null);
  const controlsRef = useRef<OrbitControls | null>(null);
  const animRef = useRef<number>(0);
  const [hoverInfo, setHoverInfo] = useState<{ x: number; y: number; label: string; density: number } | null>(null);

  // Initialize Three.js
  useEffect(() => {
    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container) return;

    const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.05;
    rendererRef.current = renderer;

    const scene = new THREE.Scene();
    scene.fog = new THREE.FogExp2(0x050810, 0.035);
    sceneRef.current = scene;

    const camera = new THREE.PerspectiveCamera(45, 1, 0.1, 100);
    camera.position.set(0, 1.8, 9.5);
    cameraRef.current = camera;

    const controls = new OrbitControls(camera, canvas);
    controls.enableDamping = true;
    controls.dampingFactor = 0.08;
    controls.rotateSpeed = 0.8;
    controls.zoomSpeed = 0.7;
    controls.panSpeed = 0.6;
    controls.minDistance = 4;
    controls.maxDistance = 24;
    controls.target.set(0, 0, 0);
    controlsRef.current = controls;

    // Lights
    const ambient = new THREE.AmbientLight(0xb0c4e8, 0.35);
    scene.add(ambient);
    const key = new THREE.DirectionalLight(0xffffff, 0.6);
    key.position.set(5, 8, 7);
    scene.add(key);
    const fill = new THREE.PointLight(0x6a8cc4, 0.8, 30);
    fill.position.set(-8, -4, 5);
    scene.add(fill);

    // Background gradient sphere
    const bgGeo = new THREE.SphereGeometry(40, 32, 32);
    const bgMat = new THREE.MeshBasicMaterial({
      color: 0x0a0e18,
      side: THREE.BackSide,
    });
    const bgSphere = new THREE.Mesh(bgGeo, bgMat);
    scene.add(bgSphere);

    // Stars
    const starCount = 600;
    const starGeo = new THREE.BufferGeometry();
    const starPos = new Float32Array(starCount * 3);
    for (let i = 0; i < starCount * 3; i++) {
      starPos[i] = (Math.random() - 0.5) * 80;
    }
    starGeo.setAttribute('position', new THREE.BufferAttribute(starPos, 3));
    const starMat = new THREE.PointsMaterial({
      color: 0xaabbee,
      size: 0.025,
      transparent: true,
      opacity: 0.5,
    });
    const stars = new THREE.Points(starGeo, starMat);
    scene.add(stars);

    // Grid helper
    const gridHelper = new THREE.GridHelper(12, 12, 0x1a2030, 0x111822);
    gridHelper.position.y = -3;
    scene.add(gridHelper);

    // Resize handler
    const resize = () => {
      const w = container.clientWidth;
      const h = container.clientHeight;
      renderer.setSize(w, h, true);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(container);

    // Animation loop
    const animate = () => {
      animRef.current = requestAnimationFrame(animate);
      controls.update();
      renderer.render(scene, camera);
    };
    animate();

    return () => {
      cancelAnimationFrame(animRef.current);
      ro.disconnect();
      controls.dispose();
      renderer.dispose();
    };
  }, []);

  // Update anchor meshes when data changes
  useEffect(() => {
    const scene = sceneRef.current;
    if (!scene) return;

    // Remove old anchor meshes and lines
    const toRemove: THREE.Object3D[] = [];
    scene.traverse(child => {
      if (child.userData?.type === 'anchor-point' || child.userData?.type === 'anchor-line') {
        toRemove.push(child);
      }
    });
    for (const obj of toRemove) {
      scene.remove(obj);
      if (obj instanceof THREE.Mesh) obj.geometry.dispose();
      if (obj instanceof THREE.Mesh && obj.material instanceof THREE.Material) obj.material.dispose();
    }

    if (anchors.length === 0) return;

    const isLarge = anchors.length > LARGE_FIELD_THRESHOLD;

    // Convert direction to 3D positions
    const positions = anchors.map(a => {
      const [x, y, z] = project3D(a.direction_n || []);
      return {
        x: x * 3.5,
        y: y * 3.5,
        z: z * 3.5,
        a,
      };
    });

    // Anchor spheres
    for (let i = 0; i < positions.length; i++) {
      const p = positions[i];
      const d = p.a.density;
      const layer = d > 15 ? 'L1' : d > 8 ? 'L2' : d > 3 ? 'L3' : 'L4';
      const color = LAYER_COLORS[layer] || LAYER_COLORS.L4;
      const size = ANCHOR_BASE_SIZE + ANCHOR_SIZE_K * Math.min(d, 20);

      const geo = new THREE.SphereGeometry(size, 16, 16);
      const mat = new THREE.MeshStandardMaterial({
        color,
        emissive: color,
        emissiveIntensity: 0.3,
        roughness: 0.4,
        metalness: 0.2,
      });
      const mesh = new THREE.Mesh(geo, mat);
      mesh.position.set(p.x, p.y, p.z);
      mesh.userData = {
        type: 'anchor-point',
        label: p.a.label,
        density: d,
        index: i,
      };
      scene.add(mesh);
    }

    // Connection lines (only for non-large fields)
    if (!isLarge || anchors.length <= 80) {
      const maxConnections = isLarge ? 30 : positions.length;
      for (let i = 0; i < Math.min(positions.length, maxConnections); i++) {
        for (let k = i + 1; k < Math.min(positions.length, maxConnections); k++) {
          const a = positions[i].a, b = positions[k].a;
          const dn_a = a.direction_n || [];
          const dn_b = b.direction_n || [];
          let dot = 0;
          for (let d = 0; d < Math.min(dn_a.length, dn_b.length); d++) {
            dot += (dn_a[d] || 0) * (dn_b[d] || 0);
          }
          if (dot > 0.5) {
            const alpha = (dot - 0.5) * 0.4;
            const lineGeo = new THREE.BufferGeometry().setFromPoints([
              new THREE.Vector3(positions[i].x, positions[i].y, positions[i].z),
              new THREE.Vector3(positions[k].x, positions[k].y, positions[k].z),
            ]);
            const lineMat = new THREE.LineBasicMaterial({
              color: 0x4a6fa5,
              transparent: true,
              opacity: alpha,
            });
            const line = new THREE.Line(lineGeo, lineMat);
            line.userData = { type: 'anchor-line' };
            scene.add(line);
          }
        }
      }
    }

    // Dummy camera to trigger fog update
    if (cameraRef.current) {
      cameraRef.current.far = 30;
      cameraRef.current.updateProjectionMatrix();
    }
  }, [anchors]);

  // Hover detection
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !sceneRef.current || !cameraRef.current) return;

    const raycaster = new THREE.Raycaster();
    const mouse = new THREE.Vector2();

    const handleMouseMove = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect();
      mouse.x = ((e.clientX - rect.left) / rect.width) * 2 - 1;
      mouse.y = -((e.clientY - rect.top) / rect.height) * 2 + 1;

      raycaster.setFromCamera(mouse, cameraRef.current!);
      const meshes: THREE.Object3D[] = [];
      sceneRef.current!.traverse(child => {
        if (child.userData?.type === 'anchor-point') meshes.push(child);
      });
      const intersects = raycaster.intersectObjects(meshes);
      if (intersects.length > 0) {
        const obj = intersects[0].object;
        setHoverInfo({
          x: e.clientX,
          y: e.clientY,
          label: obj.userData.label || '',
          density: obj.userData.density || 0,
        });
        canvas.style.cursor = 'pointer';
      } else {
        setHoverInfo(null);
        canvas.style.cursor = 'default';
      }
    };

    canvas.addEventListener('mousemove', handleMouseMove);
    return () => canvas.removeEventListener('mousemove', handleMouseMove);
  }, [anchors]);

  return (
    <div ref={containerRef} style={{ width: '100%', height: '100%' }}>
      <canvas ref={canvasRef} />
      {hoverInfo && (
        <div
          style={{
            position: 'absolute',
            left: hoverInfo.x + 12,
            top: hoverInfo.y - 10,
            pointerEvents: 'none',
            background: 'rgba(0,0,0,0.8)',
            border: '1px solid rgba(255,255,255,0.15)',
            borderRadius: 6,
            padding: '6px 10px',
            fontSize: 11,
            color: '#e0e0e0',
            zIndex: 30,
            lineHeight: 1.4,
          }}
        >
          <div style={{ color: '#8ab', fontWeight: 500 }}>{hoverInfo.label}</div>
          <div style={{ color: '#667', fontSize: 10 }}>density: {hoverInfo.density}</div>
        </div>
      )}
    </div>
  );
}

