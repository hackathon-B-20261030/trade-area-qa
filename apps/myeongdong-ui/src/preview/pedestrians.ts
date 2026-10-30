import * as THREE from 'three';

type PedestrianAgent = {
  position: THREE.Vector3;
  target: THREE.Vector3;
  speed: number;
  phase: number;
  heading: number;
};

type ObstacleGrid = {
  cellSize: number;
  cells: Map<string, THREE.Box3[]>;
};

type PedestrianSystem = {
  agents: PedestrianAgent[];
  bodies: THREE.InstancedMesh;
  heads: THREE.InstancedMesh;
  shadows: THREE.InstancedMesh;
  dummy: THREE.Object3D;
};

function pointInBoundary(x: number, z: number, boundary: [number, number][]) {
  let inside = false;
  for (let index = 0, previous = boundary.length - 1; index < boundary.length; previous = index, index += 1) {
    const [xi, zi] = boundary[index];
    const [xj, zj] = boundary[previous];
    if (((zi > z) !== (zj > z)) && x < ((xj - xi) * (z - zi)) / (zj - zi) + xi) inside = !inside;
  }
  return inside;
}

function createObstacleGrid(obstacles: THREE.Box3[], cellSize = 32): ObstacleGrid {
  const cells = new Map<string, THREE.Box3[]>();
  obstacles.forEach((box) => {
    const minX = Math.floor(box.min.x / cellSize);
    const maxX = Math.floor(box.max.x / cellSize);
    const minZ = Math.floor(box.min.z / cellSize);
    const maxZ = Math.floor(box.max.z / cellSize);
    for (let x = minX; x <= maxX; x += 1) {
      for (let z = minZ; z <= maxZ; z += 1) {
        const key = `${x}:${z}`;
        const cell = cells.get(key) ?? [];
        cell.push(box);
        cells.set(key, cell);
      }
    }
  });
  return { cellSize, cells };
}

function isBlocked(x: number, z: number, grid: ObstacleGrid) {
  const key = `${Math.floor(x / grid.cellSize)}:${Math.floor(z / grid.cellSize)}`;
  return (grid.cells.get(key) ?? []).some((box) => x >= box.min.x && x <= box.max.x && z >= box.min.z && z <= box.max.z);
}

function randomWalkablePoint(bounds: THREE.Box3, obstacles: ObstacleGrid, boundary: [number, number][]) {
  const width = bounds.max.x - bounds.min.x;
  const depth = bounds.max.z - bounds.min.z;
  for (let attempt = 0; attempt < 500; attempt += 1) {
    const x = bounds.min.x + width * Math.random();
    const z = bounds.min.z + depth * Math.random();
    if (pointInBoundary(x, z, boundary) && !isBlocked(x, z, obstacles)) return new THREE.Vector3(x, 0, z);
  }
  const center = bounds.getCenter(new THREE.Vector3());
  return new THREE.Vector3(center.x, 0, center.z);
}

function createMockPedestrians(scene: THREE.Scene, bounds: THREE.Box3, obstacles: ObstacleGrid, boundary: [number, number][], count = 700): PedestrianSystem {
  const agents: PedestrianAgent[] = [];
  const bodyGeometry = new THREE.CylinderGeometry(.22, .28, .95, 7);
  const headGeometry = new THREE.SphereGeometry(.2, 8, 6);
  const shadowGeometry = new THREE.CircleGeometry(.3, 10);
  const skinMaterial = new THREE.MeshStandardMaterial({ color: 0xffc79f, emissive: 0x4a2012, emissiveIntensity: .12, roughness: .9 });
  const shadowMaterial = new THREE.MeshBasicMaterial({ color: 0x26312c, transparent: true, opacity: .18, depthWrite: false });
  const clothingColors = [0x315c80, 0xc96b55, 0x496c55, 0xd1a64c, 0x6d5c8e, 0x5e6663];
  const clothingMaterial = new THREE.MeshStandardMaterial({ color: 0xffffff, vertexColors: true, roughness: .85 });
  const bodies = new THREE.InstancedMesh(bodyGeometry, clothingMaterial, count);
  const heads = new THREE.InstancedMesh(headGeometry, skinMaterial, count);
  const shadows = new THREE.InstancedMesh(shadowGeometry, shadowMaterial, count);
  bodies.name = 'mock-pedestrian-bodies';
  heads.name = 'mock-pedestrian-heads';
  shadows.name = 'mock-pedestrian-shadows';
  bodies.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
  heads.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
  shadows.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
  const dummy = new THREE.Object3D();

  for (let index = 0; index < count; index += 1) {
    const position = randomWalkablePoint(bounds, obstacles, boundary);
    bodies.setColorAt(index, new THREE.Color(clothingColors[index % clothingColors.length]));
    agents.push({
      position,
      target: randomWalkablePoint(bounds, obstacles, boundary),
      speed: 1.5 + Math.random() * 1.1,
      phase: Math.random() * Math.PI * 2,
      heading: Math.random() * Math.PI * 2,
    });
  }
  bodies.instanceColor!.needsUpdate = true;
  scene.add(shadows, bodies, heads);
  return { agents, bodies, heads, shadows, dummy };
}

export type PreviewPedestrianSystem = PedestrianSystem & {
  bounds: THREE.Box3;
  obstacles: ObstacleGrid;
  boundary: [number, number][];
};

export function createPreviewPedestrians(
  scene: THREE.Scene, bounds: THREE.Box3, obstacles: THREE.Box3[], boundary: [number, number][], count = 700,
): PreviewPedestrianSystem {
  const grid = createObstacleGrid(obstacles);
  return { ...createMockPedestrians(scene, bounds, grid, boundary, count), bounds, obstacles: grid, boundary };
}

export function updatePreviewPedestrians(system: PreviewPedestrianSystem, delta: number, elapsed: number): void {
        const scale = 7.5;
        system.agents.forEach((agent, index) => {
          if (!pointInBoundary(agent.position.x, agent.position.z, system.boundary)) {
            agent.position.copy(randomWalkablePoint(system.bounds, system.obstacles, system.boundary));
            agent.target.copy(randomWalkablePoint(system.bounds, system.obstacles, system.boundary));
          }
          const dx = agent.target.x - agent.position.x;
          const dz = agent.target.z - agent.position.z;
          const distance = Math.hypot(dx, dz);
          if (distance < .9) {
            agent.target.copy(randomWalkablePoint(system.bounds, system.obstacles, system.boundary));
          } else {
            const step = Math.min(agent.speed * delta, distance);
            const nextX = agent.position.x + (dx / distance) * step;
            const nextZ = agent.position.z + (dz / distance) * step;
            if (!pointInBoundary(nextX, nextZ, system.boundary) || isBlocked(nextX, nextZ, system.obstacles)) {
              agent.target.copy(randomWalkablePoint(system.bounds, system.obstacles, system.boundary));
            } else {
              agent.position.x = nextX;
              agent.position.z = nextZ;
              agent.heading = Math.atan2(dx, dz);
            }
          }

          const bob = Math.abs(Math.sin(elapsed * 7 + agent.phase)) * .035;
          system.dummy.position.set(agent.position.x, bob + .68 * scale, agent.position.z);
          system.dummy.rotation.set(0, agent.heading, 0);
          system.dummy.scale.setScalar(scale);
          system.dummy.updateMatrix();
          system.bodies.setMatrixAt(index, system.dummy.matrix);

          system.dummy.position.y = bob + 1.36 * scale;
          system.dummy.rotation.set(0, 0, 0);
          system.dummy.updateMatrix();
          system.heads.setMatrixAt(index, system.dummy.matrix);

          system.dummy.position.set(agent.position.x, .02, agent.position.z);
          system.dummy.rotation.set(-Math.PI / 2, 0, 0);
          system.dummy.updateMatrix();
          system.shadows.setMatrixAt(index, system.dummy.matrix);
        });
        system.bodies.instanceMatrix.needsUpdate = true;
        system.heads.instanceMatrix.needsUpdate = true;
        system.shadows.instanceMatrix.needsUpdate = true;
}
