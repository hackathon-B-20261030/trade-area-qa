import * as THREE from 'three';
import type { Frame } from './frames.ts';
import type { Alignment } from './replay.ts';

export type EngineAgents = { mesh: THREE.InstancedMesh; capacity: number };

export function createEngineAgents(scene: THREE.Scene, capacity: number): EngineAgents {
  // Symbols are enlarged for visibility; their radius is not the model's body radius.
  const geometry = new THREE.CylinderGeometry(1.8, 2.3, 7, 7);
  const material = new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: .65 });
  const mesh = new THREE.InstancedMesh(geometry, material, capacity);
  mesh.count = 0;
  mesh.frustumCulled = false;
  mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
  scene.add(mesh);
  return { mesh, capacity };
}

export function renderEngineFrame(system: EngineAgents, frame: Frame, alignment: Alignment): void {
  const matrix = new THREE.Matrix4();
  const color = new THREE.Color();
  const colors: Record<number, number> = { 1: 0xf35b3f, 2: 0xf8b84d, 3: 0x8a59d1, 4: 0x4f87e1, 5: 0x54b48f, 6: 0xe17fb1, 7: 0x6b7474 };
  system.mesh.count = Math.min(frame.records.length, system.capacity);
  frame.records.slice(0, system.capacity).forEach((record, i) => {
    const x = record.x_m * alignment.center_fit_model_x.slope + alignment.center_fit_model_x.offset_m;
    const z = record.z_m * alignment.center_fit_model_z.slope + alignment.center_fit_model_z.offset_m;
    matrix.makeTranslation(x, 3.5, z);
    system.mesh.setMatrixAt(i, matrix);
    system.mesh.setColorAt(i, color.setHex(colors[record.activity] ?? 0xf35b3f));
  });
  system.mesh.instanceMatrix.needsUpdate = true;
  if (system.mesh.instanceColor) system.mesh.instanceColor.needsUpdate = true;
}
