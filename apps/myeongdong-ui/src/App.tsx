import { useEffect, useMemo, useRef, useState } from 'react';
import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';
import businessData from './data/building-business-data.json';
import { createEngineAgents, renderEngineFrame, type EngineAgents } from './engine/agents';
import { arrivalsAt } from './engine/replay';
import { useEngineRun } from './engine/useEngineRun';
import { DEFAULT_RUN_OPTIONS, DEFAULT_BUILDING_OVERRIDE, type BuildingOverride, type BuildingTypeOverride, type RunOptions } from './engine/client';
import { CONTROL_BINDINGS, defaultScenarioChoices, supportedVariables } from './engine/controls';
import { sameOptions } from './engine/manualRun';
import { type BuildingMeta, type BuildingType } from './preview/scenarios';

type BusinessRecord = {
  name: string;
  addr: string;
  lotAddr: string;
  type: BuildingType;
  storeCount: number;
  totalStoreCount: number;
  stores: [string, string][];
};

const businessById = businessData as unknown as Record<string, BusinessRecord>;

const typeInfo: Record<BuildingType, { label: string; color: string }> = {
  cafe: { label: '카페', color: '#1fa84f' },
  restaurant: { label: '음식점', color: '#f0a800' },
  medical: { label: '의료 시설', color: '#e34948' },
  fashion: { label: '옷가게', color: '#2a78d6' },
  convenience: { label: '편의점', color: '#8a5cd6' },
  other: { label: '기타', color: '#b8b6ae' },
  none: { label: '1층 정보 없음', color: '#8e979c' },
};

const editableTypes: BuildingType[] = ['cafe', 'restaurant', 'medical', 'fashion', 'convenience'];
const allTypes: BuildingType[] = ['cafe', 'restaurant', 'medical', 'fashion', 'convenience', 'other', 'none'];

function TypeIcon({ type }: { type: BuildingType }) {
  if (type === 'cafe') return <><path d="M4 8h11v5a4 4 0 0 1-4 4H8a4 4 0 0 1-4-4V8Z" /><path d="M15 10h2a2.5 2.5 0 0 1 0 5h-2M5 20h13M7 3v2M11 2v3" /></>;
  if (type === 'restaurant') return <><path d="M7 3v8M4 3v5a3 3 0 0 0 6 0V3M7 11v10M16 3v18M16 3c3.5 2.5 4 7 0 10" /></>;
  if (type === 'medical') return <><rect x="4" y="6" width="16" height="15" rx="1" /><path d="M9 6V3h6v3M8 13h8M12 9v8" /></>;
  if (type === 'fashion') return <><path d="m8.5 4 3.5 2 3.5-2L21 7l-3 5-2-1v10H8V11l-2 1-3-5 5.5-3Z" /><path d="M8.5 4a3.5 3.5 0 0 0 7 0" /></>;
  if (type === 'convenience') return <><path d="M4 9h16v12H4zM3 9l2-6h14l2 6" /><path d="M8 13h4v8M15 13h3M15 16h3" /></>;
  return <><path d="M5 21V5l7-3 7 3v16M2 21h20" /><path d="M9 7h2M14 7h2M9 11h2M14 11h2M9 15h2M14 15h2" /></>;
}

function Icon({ type, className }: { type: BuildingType; className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <TypeIcon type={type} />
    </svg>
  );
}

function metaFromMesh(mesh: THREE.Mesh): BuildingMeta | null {
  const data = mesh.userData;
  if (!data?.id) return null;
  const type = (data.type in typeInfo ? data.type : 'none') as BuildingType;
  return {
    id: String(data.id),
    type,
    originalType: (data.originalType ?? type) as BuildingType,
    name: String(data.name || '이름 없는 건물'),
    addr: String(data.addr || '주소 정보 없음'),
    height: Number(data.height || 0),
    storeCount: Number(data.storeCount || 0),
  };
}

export default function App() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const sceneRef = useRef<THREE.Scene | null>(null);
  const cameraRef = useRef<THREE.PerspectiveCamera | null>(null);
  const controlsRef = useRef<OrbitControls | null>(null);
  const meshesRef = useRef<THREE.Mesh[]>([]);
  const selectedMeshRef = useRef<THREE.Mesh | null>(null);
  const helperRef = useRef<THREE.BoxHelper | null>(null);
  const hoverCardRef = useRef<HTMLDivElement>(null);
  const hoveredMeshRef = useRef<THREE.Mesh | null>(null);
  const originalMaterialsRef = useRef(new Map<string, THREE.Material | THREE.Material[]>());
  const focusMaterialsRef = useRef(new Map<THREE.Mesh, THREE.Material | THREE.Material[]>());
  const grayMaterialRef = useRef<THREE.MeshStandardMaterial | null>(null);
  const modelBoundsRef = useRef<THREE.Sphere | null>(null);
  const [selected, setSelected] = useState<BuildingMeta | null>(null);
  const [hovered, setHovered] = useState<BuildingMeta | null>(null);
  const [counts, setCounts] = useState<Record<BuildingType, number>>(() => Object.fromEntries(allTypes.map((type) => [type, 0])) as Record<BuildingType, number>);
  const [loading, setLoading] = useState(true);
  const [progress, setProgress] = useState(0);
  const [loadError, setLoadError] = useState('');
  const [autoRotate, setAutoRotate] = useState(false);
  const engine = useEngineRun();
  const replay = engine.replay;
  const [runOptions, setRunOptions] = useState<RunOptions>(() => ({ ...DEFAULT_RUN_OPTIONS, building_overrides: {} }));
  const runOptionsRef = useRef(runOptions);
  const userEditedRef = useRef(false);
  const [visualTypes, setVisualTypes] = useState<Record<string, BuildingTypeOverride>>(() => {
    try {
      const value: unknown = JSON.parse(localStorage.getItem('myeongdong.visual_types.v1') ?? '{}');
      if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
      return Object.fromEntries(Object.entries(value).filter(([id, type]) =>
        /^(way|relation|node)\/\d+$/.test(id) && editableTypes.includes(type as BuildingType))) as Record<string, BuildingTypeOverride>;
    } catch { return {}; }
  });
  const applyOptions = (next: RunOptions) => { userEditedRef.current = true; runOptionsRef.current = next; setRunOptions(next); };
  const adoptOptions = (next: RunOptions) => { runOptionsRef.current = next; setRunOptions(next); };
  const [controlsOpen, setControlsOpen] = useState(() => window.innerWidth > 700);
  const showingPreviousRun = engine.running && engine.job?.replay_base !== engine.base;
  const comparedOptions = engine.running && engine.job ? engine.job.options
    : engine.metrics?.options ?? (engine.job?.replay_base === engine.base && engine.job.state !== 'failed' ? engine.job.options : null);
  const hasUnappliedChanges = userEditedRef.current && !sameOptions(comparedOptions, runOptions);
  const agentsRef = useRef<EngineAgents | null>(null);
  const simulatedSeconds = (replay.status?.window_start_s ?? 50400) + replay.seconds;
  const hour = Math.floor(simulatedSeconds / 3600);
  const minute = Math.floor(simulatedSeconds / 60) % 60;
  const admittedAtFrame = replay.frame && replay.status
    ? arrivalsAt(replay.frame.tick, Math.round((replay.status.window_end_s - replay.status.window_start_s) / replay.status.dt_s), replay.status.planned_bodies)
    : null;

  useEffect(() => {
    if (engine.metrics && !userEditedRef.current) adoptOptions(engine.metrics.options);
  }, [engine.base, engine.metrics?.run_id]);

  useEffect(() => {
    if (engine.job?.replay_base === engine.base && !userEditedRef.current) adoptOptions(engine.job.options);
  }, [engine.base, engine.job?.run_id]);

  const totalBuildings = useMemo(() => Object.values(counts).reduce((sum, count) => sum + count, 0), [counts]);

  const updateCounts = () => {
    const next = Object.fromEntries(allTypes.map((type) => [type, 0])) as Record<BuildingType, number>;
    meshesRef.current.forEach((mesh) => {
      const type = (mesh.userData.type in typeInfo ? mesh.userData.type : 'none') as BuildingType;
      next[type] += 1;
    });
    setCounts(next);
  };

  const restoreFocusMaterials = () => {
    focusMaterialsRef.current.forEach((savedMaterial, mesh) => {
      mesh.material = savedMaterial;
    });
    focusMaterialsRef.current.clear();
  };

  const dimUnselectedMeshes = (selectedMesh: THREE.Mesh) => {
    const gray = grayMaterialRef.current ?? new THREE.MeshStandardMaterial({
      color: 0x737a77,
      roughness: .9,
      metalness: 0,
      transparent: true,
      opacity: .42,
      depthWrite: true,
    });
    grayMaterialRef.current = gray;
    meshesRef.current.forEach((mesh) => {
      if (mesh === selectedMesh) return;
      focusMaterialsRef.current.set(mesh, mesh.material);
      mesh.material = Array.isArray(mesh.material) ? mesh.material.map(() => gray) : gray;
    });
  };

  const clearSelection = () => {
    const scene = sceneRef.current;
    restoreFocusMaterials();
    if (scene && helperRef.current) scene.remove(helperRef.current);
    helperRef.current = null;
    selectedMeshRef.current = null;
    setSelected(null);
  };

  const selectMesh = (mesh: THREE.Mesh) => {
    const scene = sceneRef.current;
    if (!scene) return;
    restoreFocusMaterials();
    if (helperRef.current) scene.remove(helperRef.current);
    const helper = new THREE.BoxHelper(mesh, 0xffdf24);
    helper.material.depthTest = false;
    helper.material.transparent = true;
    helper.material.opacity = 1;
    helper.renderOrder = 1000;
    scene.add(helper);
    helperRef.current = helper;
    selectedMeshRef.current = mesh;
    setSelected(metaFromMesh(mesh));
    if (window.innerWidth <= 860) setControlsOpen(false);
    dimUnselectedMeshes(mesh);
  };

  const frameModel = () => {
    const sphere = modelBoundsRef.current;
    const camera = cameraRef.current;
    const controls = controlsRef.current;
    if (!sphere || !camera || !controls) return;
    const radius = Math.max(sphere.radius, 1);
    controls.target.copy(sphere.center);
    camera.position.set(
      sphere.center.x + radius * 1.75,
      sphere.center.y + radius * 1.35,
      sphere.center.z + radius * 1.95,
    );
    camera.near = Math.max(radius / 10000, .05);
    camera.far = radius * 100;
    camera.updateProjectionMatrix();
    controls.minDistance = radius * .015;
    controls.maxDistance = radius * 24;
    controls.update();
  };

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0xc8d2d6);
    scene.fog = new THREE.FogExp2(0xc8d2d6, .000012);
    sceneRef.current = scene;

    const camera = new THREE.PerspectiveCamera(42, 1, .05, 1000000);
    cameraRef.current = camera;

    const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, powerPreference: 'high-performance' });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.05;

    const controls = new OrbitControls(camera, canvas);
    controls.enableDamping = true;
    controls.dampingFactor = .075;
    controls.screenSpacePanning = true;
    controls.maxPolarAngle = Math.PI / 2.02;
    controlsRef.current = controls;

    const hemisphere = new THREE.HemisphereLight(0xf3f8ff, 0x66716a, 2.4);
    const sun = new THREE.DirectionalLight(0xfff4dc, 3.2);
    sun.position.set(-400, 800, 500);
    scene.add(hemisphere, sun);


    const loader = new GLTFLoader();
    loader.load(
      '/myeongdong.glb',
      (gltf) => {
        const model = gltf.scene;
        const loadedMeshes: THREE.Mesh[] = [];
        const originals = originalMaterialsRef.current;

        model.traverse((object) => {
          if (!(object instanceof THREE.Mesh)) return;
          object.frustumCulled = true;
          if (object.name === 'ground') {
            object.material = new THREE.MeshStandardMaterial({
              color: 0xbfc7c2,
              roughness: 1,
              metalness: 0,
            });
            return;
          }
          if (!object.userData?.id) return;
          const id = String(object.userData.id);
          const business = businessById[id];
          const type = business?.type ?? (object.userData.type in typeInfo ? object.userData.type : 'none') as BuildingType;
          const sourceMaterials = Array.isArray(object.material) ? object.material : [object.material];
          const color = new THREE.Color(typeInfo[type].color);
          const dataMaterials = sourceMaterials.map((sourceMaterial) => {
            const clonedMaterial = sourceMaterial.clone();
            if ('color' in clonedMaterial && clonedMaterial.color instanceof THREE.Color) clonedMaterial.color.copy(color);
            clonedMaterial.needsUpdate = true;
            return clonedMaterial;
          });
          object.material = Array.isArray(object.material) ? dataMaterials : dataMaterials[0];
          object.userData = {
            ...object.userData,
            name: business?.name || object.userData.name,
            addr: business?.addr || business?.lotAddr || object.userData.addr,
            type,
            storeCount: business?.storeCount ?? object.userData.storeCount,
            totalStoreCount: business?.totalStoreCount ?? object.userData.storeCount,
            stores: business?.stores ?? object.userData.stores ?? [],
          };
          object.userData.originalType = type;
          loadedMeshes.push(object);
          originals.set(id, object.material);
        });

        meshesRef.current = loadedMeshes;
        scene.add(model);
        const box = new THREE.Box3().setFromObject(model);
        const sphere = box.getBoundingSphere(new THREE.Sphere());
        modelBoundsRef.current = sphere;
        frameModel();
        camera.position.sub(controls.target).multiplyScalar(.6).add(controls.target);
        controls.update();
        updateCounts();
        setProgress(100);
        setLoading(false);
      },
      (event) => {
        if (event.total > 0) setProgress(Math.round((event.loaded / event.total) * 100));
      },
      () => {
        setLoading(false);
        setLoadError('myeongdong.glb를 불러오지 못했습니다.');
      },
    );

    const raycaster = new THREE.Raycaster();
    const pointer = new THREE.Vector2();
    let downX = 0;
    let downY = 0;
    canvas.addEventListener('pointerdown', (event) => {
      downX = event.clientX;
      downY = event.clientY;
    });
    const onPointerUp = (event: PointerEvent) => {
      if (Math.hypot(event.clientX - downX, event.clientY - downY) > 5) return;
      const rect = canvas.getBoundingClientRect();
      pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
      pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
      raycaster.setFromCamera(pointer, camera);
      const hit = raycaster.intersectObjects(meshesRef.current, false)[0];
      if (hit?.object instanceof THREE.Mesh) selectMesh(hit.object);
      else clearSelection();
    };
    canvas.addEventListener('pointerup', onPointerUp);

    const onPointerMove = (event: PointerEvent) => {
      const rect = canvas.getBoundingClientRect();
      pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
      pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
      raycaster.setFromCamera(pointer, camera);
      const hit = raycaster.intersectObjects(meshesRef.current, false)[0];
      const mesh = hit?.object instanceof THREE.Mesh ? hit.object : null;
      canvas.style.cursor = mesh ? 'pointer' : 'grab';

      if (mesh !== hoveredMeshRef.current) {
        hoveredMeshRef.current = mesh;
        setHovered(mesh ? metaFromMesh(mesh) : null);
      }
      if (mesh && hoverCardRef.current) {
        const cardWidth = 286;
        const cardHeight = 130;
        const x = Math.min(event.clientX + 22, window.innerWidth - cardWidth - 12);
        const y = Math.max(82, Math.min(event.clientY - 42, window.innerHeight - cardHeight - 116));
        hoverCardRef.current.style.left = `${x}px`;
        hoverCardRef.current.style.top = `${y}px`;
      }
    };
    canvas.addEventListener('pointermove', onPointerMove);
    const onPointerLeave = () => {
      hoveredMeshRef.current = null;
      setHovered(null);
    };
    canvas.addEventListener('pointerleave', onPointerLeave);

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      renderer.setSize(rect.width, rect.height, false);
      camera.aspect = rect.width / rect.height;
      camera.updateProjectionMatrix();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    resize();

    let frame = 0;
    const timer = new THREE.Timer();
    const animate = () => {
      frame = requestAnimationFrame(animate);
      timer.update();
      controls.update();
      helperRef.current?.update();
      renderer.render(scene, camera);
    };
    animate();

    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      canvas.removeEventListener('pointerup', onPointerUp);
      canvas.removeEventListener('pointermove', onPointerMove);
      canvas.removeEventListener('pointerleave', onPointerLeave);
      controls.dispose();
      timer.dispose();
      renderer.dispose();
      scene.traverse((object) => {
        if (object instanceof THREE.Mesh) {
          object.geometry.dispose();
          const materials = Array.isArray(object.material) ? object.material : [object.material];
          materials.forEach((item) => item.dispose());
        }
      });
      grayMaterialRef.current?.dispose();
      agentsRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (controlsRef.current) controlsRef.current.autoRotate = autoRotate;
  }, [autoRotate]);

  useEffect(() => {
    if (!sceneRef.current) return;
    if (!replay.status || !replay.frame) {
      if (agentsRef.current) agentsRef.current.mesh.count = 0;
      return;
    }
    if (!agentsRef.current) agentsRef.current = createEngineAgents(sceneRef.current, replay.status.planned_bodies);
    renderEngineFrame(agentsRef.current, replay.frame, replay.status.alignment);
  }, [replay.frame, replay.status]);

  useEffect(() => {
    try { localStorage.setItem('myeongdong.visual_types.v1', JSON.stringify(visualTypes)); } catch { /* display still works in this tab */ }
  }, [visualTypes]);

  // Keep all designated building colors in sync, including after loading a saved run.
  useEffect(() => {
    if (loading) return;
    const wanted = (mesh: THREE.Mesh): BuildingType => {
      const id = String(mesh.userData.id);
      const type = visualTypes[id] ?? runOptions.building_overrides[id]?.type;
      return type && type !== 'original' ? type : mesh.userData.originalType as BuildingType;
    };
    if (!meshesRef.current.some((mesh) => mesh.userData.type !== wanted(mesh))) return;
    restoreFocusMaterials();
    meshesRef.current.forEach((mesh) => {
      const next = wanted(mesh);
      if (mesh.userData.type === next) return;
      const original = originalMaterialsRef.current.get(String(mesh.userData.id));
      if (!original) return;
      const originals = Array.isArray(original) ? original : [original];
      const old = Array.isArray(mesh.material) ? mesh.material : [mesh.material];
      old.forEach((item) => { if (!originals.includes(item)) item.dispose(); });
      if (next === mesh.userData.originalType) mesh.material = original;
      else {
        const color = new THREE.Color(typeInfo[next].color);
        const copies = originals.map((item) => {
          const clone = item.clone();
          if ('color' in clone && clone.color instanceof THREE.Color) clone.color.copy(color);
          clone.needsUpdate = true;
          return clone;
        });
        mesh.material = Array.isArray(original) ? copies : copies[0];
      }
      mesh.userData.type = next;
    });
    updateCounts();
    if (selectedMeshRef.current) {
      setSelected(metaFromMesh(selectedMeshRef.current));
      dimUnselectedMeshes(selectedMeshRef.current);
    }
  }, [loading, runOptions.building_overrides, visualTypes]);

  useEffect(() => {
    if (loading) return;
    const id = new URLSearchParams(window.location.search).get('building');
    if (!id || !/^(way|relation|node)\/\d+$/.test(id)) return;
    const mesh = meshesRef.current.find((item) => String(item.userData.id) === id);
    const camera = cameraRef.current;
    const controls = controlsRef.current;
    if (!mesh || !camera || !controls) return;
    selectMesh(mesh);
    const sphere = new THREE.Box3().setFromObject(mesh).getBoundingSphere(new THREE.Sphere());
    const direction = camera.position.clone().sub(controls.target).normalize();
    controls.target.copy(sphere.center);
    camera.position.copy(sphere.center).addScaledVector(direction, Math.max(sphere.radius * 4.5, 30));
    controls.update();
  }, [loading]);

  const setBuildingOverride = (id: string, value: BuildingOverride | null) => {
    const buildings = { ...runOptionsRef.current.building_overrides };
    if (value && (value.type !== 'original' || value.discount_pct !== null || value.price_multiplier !== null || value.hours_extension !== null)) buildings[id] = value;
    else delete buildings[id];
    applyOptions({ ...runOptionsRef.current, building_overrides: buildings });
  };
  const changeBuildingType = (type: BuildingTypeOverride) => {
    if (!selected) return;
    const entry = engine.buildings?.[selected.id];
    if (!entry) return;
    if (!entry.eligible) {
      setVisualTypes((current) => {
        const next = { ...current };
        if (type === 'original') delete next[selected.id]; else next[selected.id] = type;
        return next;
      });
      return;
    }
    const previous = runOptionsRef.current.building_overrides[selected.id] ?? DEFAULT_BUILDING_OVERRIDE;
    if (type === 'original' && !entry.original_store) setBuildingOverride(selected.id, null);
    else setBuildingOverride(selected.id, { ...previous, type,
      ...(type === 'medical' ? { discount_pct: null, price_multiplier: null } : {}) });
  };
  const changeBuildingSetting = (field: 'discount_pct' | 'price_multiplier' | 'hours_extension', value: number | null) => {
    if (!selected) return;
    const previous = runOptionsRef.current.building_overrides[selected.id] ?? DEFAULT_BUILDING_OVERRIDE;
    setBuildingOverride(selected.id, { ...previous, [field]: value } as BuildingOverride);
  };

  return (
    <main className={`glb-app ${hour < 6 || hour >= 19 ? 'is-night' : ''} ${selected ? 'has-selection' : ''}`}>
      <canvas ref={canvasRef} className="glb-canvas" aria-label="명동 실제 건물 3D 모델" />

      {loading && (
        <div className="loading-screen">
          <div className="loading-mark">MD</div>
          <b>명동 3D 모델을 불러오는 중</b>
          <span>myeongdong.glb · {progress}%</span>
          <div><i style={{ width: `${progress}%` }} /></div>
        </div>
      )}

      {loadError && <div className="load-error">{loadError}</div>}

      <div className="preview-notice" role="status">
        {engine.error || replay.error || (showingPreviousRun ? '새 실행 준비 중 · 이전 실행 결과 표시' : replay.status ? `${replay.status.status === 'complete' ? '계산 완료' : '계산 중'} · ${replay.status.planned_bodies.toLocaleString('ko-KR')}명 투입 · 14:00–17:00 시뮬레이션` : '실제 엔진 실행을 불러오는 중')}
      </div>

      <div className="scene-label">
        <b className="scene-pretendard">서울특별시 중구 명동</b>
        <small className="scene-pretendard">드래그 회전 · 스크롤 확대 · 건물 클릭</small>
      </div>

      <section className="time-display" aria-label="현재 시뮬레이션 시간">
        <span>{String(hour).padStart(2, '0')}</span><small>Hr</small><i /><span>{String(minute).padStart(2, '0')}</span><small>Min</small>
      </section>

      <aside className="engine-panel" aria-label="엔진 실행 및 결과">
        <button type="button" className="engine-panel-toggle" aria-expanded={controlsOpen} onClick={() => setControlsOpen(!controlsOpen)}>
          <span>엔진 실행 · 5,044명</span><b>{controlsOpen ? '−' : '+'}</b>
        </button>
        {controlsOpen && <div className="engine-panel-content">
          <p className="engine-kicker">실제 Rust 엔진 · 14:00–17:00</p>
          <p className="engine-hint">조건을 정한 뒤 실행 버튼을 눌러 주세요. 5,044명은 매번 모두 입장합니다.</p>
          <button type="button" className="engine-start" disabled={engine.running || !engine.health?.ready} onClick={() => void engine.start(runOptionsRef.current)}>{engine.running ? '엔진 실행 중' : '시뮬레이션 실행'}</button>
          <a className="engine-demo-link" href="/demo/index.html" target="_blank" rel="noopener">검증된 가설 시연 보기 ↗</a>
          {hasUnappliedChanges && <p className="engine-progress" role="status">{engine.running ? '변경한 조건은 실행 전입니다. 계산이 끝나면 실행 버튼을 눌러 주세요.' : '변경한 조건은 아직 적용되지 않았습니다. 실행 버튼을 눌러 주세요.'}</p>}
          {engine.running && <p className="engine-progress" role="status">{engine.job?.status_available && engine.job.total_ticks ? `계산 중 · ${Math.floor((engine.job.completed_tick ?? 0) / engine.job.total_ticks * 100)}%` : engine.job?.state === 'building' ? '엔진 빌드·검증 중' : '엔진 준비·자원 대기 중'}</p>}
          {!engine.health?.ready && <p className="engine-problem" role="status">{engine.health?.error ?? '엔진 서버 확인 중'}</p>}
          {engine.error && <p className="engine-problem" role="alert">{engine.error}</p>}
          <div className="engine-variables" aria-label="원본 시나리오 변수">
            {supportedVariables.map((variable) => {
              const binding = CONTROL_BINDINGS[variable.id];
              const available = 'key' in binding;
              const index = available ? binding.values.findIndex((value) => value === runOptions[binding.key]) : (defaultScenarioChoices[variable.id] ?? 0);
              return <div className={`engine-variable ${available ? '' : 'is-disabled'}`} key={variable.id}>
                <label><span><small>{String(variable.id).padStart(2, '0')}</small> {variable.name}</span>
                  <select aria-label={variable.name} value={String(Math.max(index, 0))} disabled={!available}
                    onChange={(event) => {
                      if (!('key' in binding)) return;
                      const value = binding.values[Number(event.target.value)];
                      applyOptions({ ...runOptionsRef.current, [binding.key]: value });
                    }}>
                    {variable.options.map((option, i) => <option value={i} key={option}>{option}</option>)}
                  </select>
                </label>
                {available ? binding.hint && <p>{binding.hint}</p> : <p>{binding.reason}</p>}
              </div>;
            })}
          </div>
          <p className="engine-hint">건물을 클릭하면 업종과 개별 조건을 지정할 수 있습니다.</p>
          {engine.metrics && <div className="engine-results" aria-label="엔진 기록 요약">
            <b>{showingPreviousRun ? '이전 실행 기록' : '엔진 기록'} · 3시간 시뮬레이션</b>
            <div><span>장소 입장</span><strong>{engine.metrics.totals.enters.toLocaleString('ko-KR')}회</strong></div>
            <div><span>구매</span><strong>{engine.metrics.totals.purchases.toLocaleString('ko-KR')}회</strong></div>
            <div><span>구매액</span><strong>{Math.round(engine.metrics.totals.revenue_krw).toLocaleString('ko-KR')}원</strong></div>
            <small>적용 조건: 보행 전용 {engine.metrics.legacy_options ? (engine.metrics.options.pedestrian_hours ? '14–17시' : '미적용') : `${engine.metrics.options.pedestrian_hours}시간`} · 할인 {engine.metrics.options.discount_pct}%{engine.metrics.legacy_options ? ' · 이전 실행: 노점과 추가 변수 미적용' : ` · 건물별 지정 ${Object.keys(engine.metrics.options.building_overrides).length}곳`}</small>
            {!engine.metrics.legacy_options && <small>보행 폭 +{engine.metrics.options.width_delta_m}m · 노점 {Math.round(40 * engine.metrics.options.stall_scale)}개 · 벤치 {engine.metrics.options.bench_count}개 · 영업 +{engine.metrics.options.hours_extension}시간 · 소득 {Math.round(engine.metrics.options.income_multiplier * 100)}% · 가격 {Math.round(engine.metrics.options.price_multiplier * 100)}% · 광고 {engine.metrics.options.visibility_level === 'low' ? '낮음' : engine.metrics.options.visibility_level === 'high' ? '높음' : '기준'}</small>}
            <small>시뮬레이션 결과</small>
          </div>}
          {engine.metricsError && <p className="engine-problem">결과 조회: {engine.metricsError}</p>}
        </div>}
      </aside>

      <div className="view-controls">
        <button type="button" onClick={frameModel} aria-label="전체 모델 보기">⌂</button>
        <button type="button" className={autoRotate ? 'active' : ''} onClick={() => setAutoRotate((value) => !value)} aria-label="자동 회전">↻</button>
      </div>

      <aside className={`building-panel ${selected ? 'open' : ''}`}>
        {selected && (
          <>
            <button type="button" className="close-panel" onClick={clearSelection} aria-label="선택 해제">×</button>
            <span className="forecast-icon" style={{ background: typeInfo[selected.type].color }}><Icon type={selected.type} className="forecast-icon-svg" /></span>
            <div className="forecast-content">
              <small>{selected.name}</small>
              <p>{selected.addr}</p>
              {engine.metrics ? (engine.metrics.buildings[selected.id] ? <>
                <p>완료된 실행 · 장소 입장 <b>{engine.metrics.buildings[selected.id].enters.toLocaleString('ko-KR')}회</b></p>
                <p>구매 {engine.metrics.buildings[selected.id].purchases.toLocaleString('ko-KR')}회 · 시뮬레이션 구매액 {Math.round(engine.metrics.buildings[selected.id].revenue_krw).toLocaleString('ko-KR')}원</p>
              </> : <p>완료된 실행에 이 건물의 장소 기록이 없습니다.</p>) : <p>엔진 계산 완료 후 건물 결과를 표시합니다.</p>}
              {(() => {
                const entry = engine.buildings?.[selected.id];
                const override = runOptions.building_overrides[selected.id] ?? DEFAULT_BUILDING_OVERRIDE;
                const chosenType = entry?.eligible ? override.type : (visualTypes[selected.id] ?? 'original');
                const canSetPlace = !!entry?.eligible && (entry.original_store || override.type !== 'original');
                return <div className="building-settings">
                  <label>건물 종류 <select aria-label="건물 종류" value={chosenType} disabled={!entry} onChange={(event) => changeBuildingType(event.target.value as BuildingTypeOverride)}>
                    <option value="original">기존 건물 정보</option>
                    {editableTypes.map((type) => <option value={type} key={type}>{typeInfo[type].label}</option>)}
                  </select></label>
                  {entry?.eligible && canSetPlace && <>
                    {chosenType !== 'medical' && <>
                    <label>이 건물 할인 <select aria-label="이 건물 할인" value={override.discount_pct ?? ''} onChange={(e) => changeBuildingSetting('discount_pct', e.target.value === '' ? null : Number(e.target.value))}>
                      <option value="">전체 설정 따름</option><option value="0">0%</option><option value="10">10%</option><option value="20">20%</option>
                    </select></label>
                    <label>이 건물 가격 <select aria-label="이 건물 가격" value={override.price_multiplier ?? ''} onChange={(e) => changeBuildingSetting('price_multiplier', e.target.value === '' ? null : Number(e.target.value))}>
                      <option value="">전체 설정 따름</option><option value="0.9">기준의 90%</option><option value="1">기준의 100%</option><option value="1.1">기준의 110%</option>
                    </select></label>
                    </>}
                    <label>이 건물 영업시간 <select aria-label="이 건물 영업시간" value={override.hours_extension ?? ''} onChange={(e) => changeBuildingSetting('hours_extension', e.target.value === '' ? null : Number(e.target.value))}>
                      <option value="">전체 설정 따름</option><option value="0">기준</option><option value="1">+1시간</option><option value="2">+2시간</option>
                    </select></label>
                  </>}
                  {!entry && <p>{engine.buildingsError || '건물의 보행망 연결을 확인 중입니다.'}</p>}
                  {entry && !entry.eligible && <p>지도 표시만 변경됩니다. 보행망에서 {entry.distance_m}m 떨어져 엔진에는 반영되지 않습니다.</p>}
                  {entry?.eligible && entry.basis === 'glb_center_assumed' && <p>GLB 중심을 추정한 위치입니다. 건물 외곽과 장애물은 검증되지 않았습니다.</p>}
                  {entry?.eligible && !canSetPlace && <p>기존 엔진 상점이 없어 종류를 먼저 지정해야 합니다.</p>}
                  {chosenType === 'medical' && <p>의료 시설은 일반 방문 대리 장소입니다. 진료·의료 매출 모형은 없습니다.</p>}
                </div>;
              })()}
            </div>
          </>
        )}
      </aside>

      <div ref={hoverCardRef} className={`hover-card ${hovered ? 'visible' : ''}`} aria-hidden="true">
        {hovered && (
          <>
            <span className="hover-icon" style={{ background: typeInfo[hovered.type].color }}><Icon type={hovered.type} /></span>
            <div className="hover-content">
              <small>{typeInfo[hovered.type].label} · BUILDING</small>
              <b>{hovered.name}</b>
              <p>{hovered.addr}</p>
              <div><span>높이 {hovered.height.toFixed(1)}m</span><i /><span>1층 가게 {hovered.storeCount}개</span></div>
            </div>
          </>
        )}
      </div>

      <section className="time-slider replay-controls" aria-label="엔진 재생 조작">
        <div className="replay-topline">
          <button type="button" className="play-button" onClick={() => {
            if (!replay.playing && replay.seconds >= replay.maxSeconds) replay.setSeconds(0);
            replay.setPlaying(!replay.playing);
          }} disabled={!replay.frame} aria-label={replay.playing ? '일시정지' : '재생'}>{replay.playing ? 'Ⅱ' : '▶'}</button>
          <label className="speed-label">속도 <select value={replay.speed} onChange={(event) => replay.setSpeed(Number(event.target.value))} aria-label="재생 속도"><option value={1}>1×</option><option value={10}>10×</option><option value={60}>60×</option></select></label>
          <div className="replay-stats"><span>화면 인원 <b>{replay.frame?.records.length.toLocaleString('ko-KR') ?? '—'}명</b></span><span>현재까지 입장 <b>{admittedAtFrame?.toLocaleString('ko-KR') ?? '—'} / {replay.status?.planned_bodies.toLocaleString('ko-KR') ?? '5,044'}명</b></span></div>
        </div>
        <div className="timeline-track">
          <input type="range" min="0" max={Math.max(replay.maxSeconds, 1)} step="1" value={replay.seconds} disabled={!replay.frame} onChange={(event) => replay.setSeconds(Number(event.target.value))} aria-label="재생 시간 이동" style={{ '--time-progress': `${replay.maxSeconds ? (replay.seconds / replay.maxSeconds) * 100 : 0}%` } as React.CSSProperties} />
          <div className="timeline-labels"><span>14:00</span><span>{replay.status?.status === 'running' ? '완료된 구간까지 재생' : '16:59'}</span></div>
        </div>
        <p className="replay-note">명동 지상 도로·건물 대리값 · 사람 표시는 확대 · 물리 검증 미완료</p>
      </section>

      
    </main>
  );
}
