<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch } from "vue";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import type { ReconstructedSurface } from "@/services/tauri";

const props = defineProps<{ surface: ReconstructedSurface | null }>();
const canvasRef = ref<HTMLCanvasElement | null>(null);

let scene: THREE.Scene | null = null;
let camera: THREE.PerspectiveCamera | null = null;
let renderer: THREE.WebGLRenderer | null = null;
let controls: OrbitControls | null = null;
let mesh: THREE.Mesh | null = null;
let raf = 0;
let resizeCleanup: (() => void) | null = null;

function buildGeometry(surface: ReconstructedSurface): THREE.BufferGeometry {
  const g = new THREE.BufferGeometry();
  const positions = new Float32Array(surface.vertices.flatMap((v) => [v[0], v[1], v[2]]));
  const uvs = new Float32Array(surface.uv_coords.flatMap((uv) => [uv[0], uv[1]]));
  g.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  g.setAttribute("uv", new THREE.BufferAttribute(uvs, 2));
  const cols = surface.topology.cols;
  const rows = surface.topology.rows;
  const idx: number[] = [];
  const idxAt = (c: number, r: number) => r * (cols + 1) + c;
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const a = idxAt(c, r);
      const b = idxAt(c + 1, r);
      const cc = idxAt(c + 1, r + 1);
      const d = idxAt(c, r + 1);
      idx.push(a, b, cc, a, cc, d);
    }
  }
  g.setIndex(idx);
  g.computeVertexNormals();
  return g;
}

function ensureScene() {
  if (!canvasRef.value) return;
  scene = new THREE.Scene();
  scene.background = new THREE.Color(0x111827);
  camera = new THREE.PerspectiveCamera(60, 1, 0.01, 1000);
  camera.position.set(8, 6, 8);
  renderer = new THREE.WebGLRenderer({ canvas: canvasRef.value, antialias: true });
  renderer.setPixelRatio(window.devicePixelRatio);
  controls = new OrbitControls(camera, canvasRef.value);
  controls.enableDamping = true;

  const grid = new THREE.GridHelper(10, 10, 0x444444, 0x222222);
  scene.add(grid);
  const dir = new THREE.DirectionalLight(0xffffff, 0.9);
  dir.position.set(5, 10, 5);
  scene.add(dir);
  scene.add(new THREE.AmbientLight(0xffffff, 0.4));

  const resize = () => {
    if (!canvasRef.value || !renderer || !camera) return;
    const w = canvasRef.value.clientWidth;
    const h = canvasRef.value.clientHeight;
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  };
  resize();
  window.addEventListener("resize", resize);
  resizeCleanup = () => window.removeEventListener("resize", resize);

  const tick = () => {
    raf = requestAnimationFrame(tick);
    controls?.update();
    if (renderer && scene && camera) renderer.render(scene, camera);
  };
  tick();
}

function setMeshFromSurface(surface: ReconstructedSurface) {
  if (!scene) return;
  if (mesh) {
    scene.remove(mesh);
    mesh.geometry.dispose();
    (mesh.material as THREE.Material).dispose();
    mesh = null;
  }
  const g = buildGeometry(surface);
  const m = new THREE.MeshStandardMaterial({ color: 0x0ea5e9, side: THREE.DoubleSide, wireframe: false });
  mesh = new THREE.Mesh(g, m);
  scene.add(mesh);
  const box = new THREE.Box3().setFromObject(mesh);
  const center = new THREE.Vector3();
  box.getCenter(center);
  controls?.target.copy(center);
}

onMounted(() => {
  ensureScene();
  if (props.surface) setMeshFromSurface(props.surface);
});

watch(
  () => props.surface,
  (v) => {
    if (v) setMeshFromSurface(v);
  },
);

onBeforeUnmount(() => {
  cancelAnimationFrame(raf);
  resizeCleanup?.();
  if (mesh) {
    mesh.geometry.dispose();
    (mesh.material as THREE.Material).dispose();
  }
  controls?.dispose();
  renderer?.dispose();
  scene = null;
  camera = null;
  renderer = null;
  controls = null;
  mesh = null;
});
</script>

<template>
  <canvas ref="canvasRef" class="h-full w-full" />
</template>
