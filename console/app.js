// ORBISTRA console — 3D catalog view + conjunction watch.
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";

const R_EARTH = 6378.137; // km
const SCALE = 1 / R_EARTH; // render in Earth radii
const API = "";

// --- Scene setup ---------------------------------------------------------
const container = document.getElementById("globe-container");
const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(
  45, container.clientWidth / container.clientHeight, 0.01, 100
);
camera.position.set(0, 1.4, 3.2);

const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
renderer.setSize(container.clientWidth, container.clientHeight);
container.appendChild(renderer.domElement);

const controls = new OrbitControls(camera, renderer.domElement);
controls.enableDamping = true;
controls.minDistance = 1.3;
controls.maxDistance = 20;

// Earth: textured sphere with graceful fallback to a shaded globe.
const earthGroup = new THREE.Group();
scene.add(earthGroup);

const sphereGeo = new THREE.SphereGeometry(1, 64, 64);
const fallbackMat = new THREE.MeshPhongMaterial({
  color: 0x0d2c50, emissive: 0x030b18, specular: 0x0a1a2e, shininess: 8,
});
const earth = new THREE.Mesh(sphereGeo, fallbackMat);
earthGroup.add(earth);

new THREE.TextureLoader().load(
  "https://cdn.jsdelivr.net/npm/three-globe@2.31.0/example/img/earth-blue-marble.jpg",
  (tex) => {
    tex.colorSpace = THREE.SRGBColorSpace;
    earth.material = new THREE.MeshPhongMaterial({ map: tex, shininess: 6 });
  },
  undefined,
  () => {} // keep fallback material on error
);

// Subtle atmosphere.
const atmosphere = new THREE.Mesh(
  new THREE.SphereGeometry(1.015, 64, 64),
  new THREE.MeshBasicMaterial({
    color: 0x4ea1ff, transparent: true, opacity: 0.06, side: THREE.BackSide,
  })
);
earthGroup.add(atmosphere);

scene.add(new THREE.AmbientLight(0xffffff, 0.85));
const sun = new THREE.DirectionalLight(0xffffff, 1.6);
sun.position.set(5, 2, 4);
scene.add(sun);

// Starfield.
{
  const n = 1500;
  const pos = new Float32Array(n * 3);
  for (let i = 0; i < n; i++) {
    const v = new THREE.Vector3().randomDirection().multiplyScalar(40 + Math.random() * 40);
    pos.set([v.x, v.y, v.z], i * 3);
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.BufferAttribute(pos, 3));
  scene.add(new THREE.Points(g, new THREE.PointsMaterial({ color: 0x8899bb, size: 0.05 })));
}

// Satellite point cloud.
let satPoints = null;
let satIndex = new Map(); // norad_id -> index into positions array

// Conjunction highlight markers.
const highlightGroup = new THREE.Group();
scene.add(highlightGroup);

// ECEF (km) -> scene coordinates. Three.js is y-up; ECEF is z-up.
function ecefToScene([x, y, z]) {
  return new THREE.Vector3(x * SCALE, z * SCALE, -y * SCALE);
}

function altitudeColor(altKm) {
  if (altKm < 2000) return new THREE.Color(0x4ea1ff);   // LEO — blue
  if (altKm < 30000) return new THREE.Color(0xffc857);  // MEO — amber
  return new THREE.Color(0xff7ab8);                     // GEO+ — pink
}

// --- Data ----------------------------------------------------------------
async function fetchJSON(path) {
  const r = await fetch(API + path);
  if (!r.ok) throw new Error(`${path}: ${r.status}`);
  return r.json();
}

async function refreshStates() {
  try {
    const data = await fetchJSON("/api/states");
    const states = data.states;
    const pos = new Float32Array(states.length * 3);
    const col = new Float32Array(states.length * 3);
    satIndex = new Map();
    states.forEach((s, i) => {
      const v = ecefToScene(s.ecef_km);
      pos.set([v.x, v.y, v.z], i * 3);
      const c = altitudeColor(s.alt_km);
      col.set([c.r, c.g, c.b], i * 3);
      satIndex.set(s.id, i);
    });

    if (satPoints) {
      satPoints.geometry.dispose();
      scene.remove(satPoints);
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.BufferAttribute(pos, 3));
    g.setAttribute("color", new THREE.BufferAttribute(col, 3));
    satPoints = new THREE.Points(
      g,
      new THREE.PointsMaterial({ size: 0.014, vertexColors: true, sizeAttenuation: true })
    );
    scene.add(satPoints);

    document.getElementById("stat-objects").innerHTML = `<b>${data.count}</b> objects`;
    document.getElementById("stat-time").textContent =
      new Date(data.t).toISOString().replace("T", " ").slice(0, 19) + "Z";
  } catch (e) {
    console.error("states refresh failed", e);
  }
}

async function refreshSummary() {
  try {
    const s = await fetchJSON("/api/summary");
    document.getElementById("stat-source").textContent = s.source;
    document.getElementById("screen-meta").textContent =
      ` · next ${s.screen_hours} h · < ${s.threshold_km} km`;
    return s;
  } catch { return null; }
}

async function refreshConjunctions() {
  try {
    const data = await fetchJSON("/api/conjunctions");
    const list = document.getElementById("conjunction-list");
    const status = document.getElementById("screening-status");
    if (data.status !== "ready") {
      status.textContent = "screening in progress…";
      setTimeout(refreshConjunctions, 4000);
      return;
    }
    status.textContent =
      `${data.conjunctions.length} events · computed in ${data.elapsed_s.toFixed(1)} s`;
    list.innerHTML = "";
    data.conjunctions.slice(0, 50).forEach((c) => {
      const li = document.createElement("li");
      const missClass = c.miss_distance_km < 1 ? "miss" : "miss far";
      li.innerHTML = `
        <div class="pair">${esc(c.name_a)} ⇄ ${esc(c.name_b)}</div>
        <div class="metrics">
          <span class="${missClass}">${c.miss_distance_km.toFixed(3)} km</span>
          <span>${c.relative_speed_km_s.toFixed(1)} km/s</span>
          <span>Pc ${c.probability.toExponential(1)}</span>
          <span>${c.tca.slice(5, 16).replace("T", " ")}Z</span>
        </div>`;
      li.addEventListener("click", () => highlightPair(c, li));
      list.appendChild(li);
    });
  } catch (e) {
    console.error("conjunctions refresh failed", e);
    setTimeout(refreshConjunctions, 6000);
  }
}

function esc(s) {
  const d = document.createElement("div");
  d.textContent = s;
  return d.innerHTML;
}

function highlightPair(c, li) {
  document.querySelectorAll("#conjunction-list li").forEach((n) => n.classList.remove("active"));
  li.classList.add("active");
  highlightGroup.clear();
  const mk = () =>
    new THREE.Mesh(
      new THREE.SphereGeometry(0.02, 12, 12),
      new THREE.MeshBasicMaterial({ color: 0xff5d5d, transparent: true, opacity: 0.9 })
    );
  for (const id of [c.norad_id_a, c.norad_id_b]) {
    const idx = satIndex.get(id);
    if (idx === undefined || !satPoints) continue;
    const a = satPoints.geometry.attributes.position;
    const m = mk();
    m.position.set(a.getX(idx), a.getY(idx), a.getZ(idx));
    highlightGroup.add(m);
    controls.target.copy(m.position).multiplyScalar(0.5);
  }
}

// --- Loop ----------------------------------------------------------------
function onResize() {
  camera.aspect = container.clientWidth / container.clientHeight;
  camera.updateProjectionMatrix();
  renderer.setSize(container.clientWidth, container.clientHeight);
}
window.addEventListener("resize", onResize);

function animate() {
  requestAnimationFrame(animate);
  controls.update();
  renderer.render(scene, camera);
}

refreshSummary();
refreshStates();
refreshConjunctions();
setInterval(refreshStates, 15000);
setInterval(refreshSummary, 30000);
animate();
