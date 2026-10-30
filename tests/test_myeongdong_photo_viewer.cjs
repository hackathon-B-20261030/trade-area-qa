const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const root = path.resolve(__dirname, '..');
const output = path.join(root, 'data/myeongdong-3d-map-photo');
const THREE = require(path.join(output, 'vendor/three.min.js'));
const context = {THREE};
vm.createContext(context);
vm.runInContext(fs.readFileSync(path.join(output, 'viewer.js'), 'utf8'), context);
const data = JSON.parse(fs.readFileSync(path.join(output, 'map_data.json')));
const metadata = JSON.parse(fs.readFileSync(path.join(output, 'satellite_metadata.json')));
const photos = JSON.parse(fs.readFileSync(path.join(output, 'photos_metadata.json')));
const [west, south, east, north] = metadata.bounds_wgs84;
const center = data.metadata.center;
const scale = 111320 * Math.cos(center.lat * Math.PI / 180);
function local(lon, lat) { return [(lon - center.lon) * scale, -(lat - center.lat) * 111132]; }
function near(actual, expected, tolerance = 1e-6) { assert.ok(Math.abs(actual - expected) <= tolerance, `${actual} != ${expected}`); }

for (const [lon, lat, u, v] of [[west, north, 0, 1], [east, north, 1, 1], [west, south, 0, 0],
  [east, south, 1, 0], [(west + east) / 2, (south + north) / 2, .5, .5]]) {
  const [x, z] = local(lon, lat);
  const actual = context.roofUV(x, z, metadata.bounds_wgs84, center);
  near(actual[0], u); near(actual[1], v);
}
for (const building of data.buildings) {
  for (const [x, z] of building.footprint) {
    const uv = context.roofUV(x, z, metadata.bounds_wgs84, center);
    assert.ok(uv.every(n => n >= 0 && n <= 1), `roof outside image: ${building.id}`);
  }
}
const buildingById = new Map(data.buildings.map(b => [b.id, b]));
for (const photo of photos) {
  const building = buildingById.get(photo.id);
  const geometry = context.facadeGeometry(building, photo);
  const positions = geometry.getAttribute('position');
  const uv = geometry.getAttribute('uv');
  assert.equal(positions.count, photo.edge_indices.length * 6);
  assert.equal(uv.count, positions.count);
  for (let vertex = 0; vertex < uv.count; vertex++) {
    assert.ok(uv.getX(vertex) >= photo.uv_crop[0] - 1e-5 && uv.getX(vertex) <= photo.uv_crop[2] + 1e-5);
    assert.ok(uv.getY(vertex) >= 1 - photo.uv_crop[3] - 1e-5 && uv.getY(vertex) <= 1 - photo.uv_crop[1] + 1e-5);
    const y = positions.getY(vertex);
    assert.ok(Math.abs(y - (building.base + building.minHeight)) < .001 ||
              Math.abs(y - (building.base + building.height)) < .001);
    const edge = photo.edge_indices[Math.floor(vertex / 6)];
    const a = building.footprint[edge], b = building.footprint[(edge + 1) % building.footprint.length];
    const length = Math.hypot(...photo.camera_direction);
    const x = positions.getX(vertex) - photo.camera_direction[0] / length * .05;
    const z = positions.getZ(vertex) - photo.camera_direction[1] / length * .05;
    assert.ok(Math.hypot(x - a[0], z - a[1]) < .001 || Math.hypot(x - b[0], z - b[1]) < .001);
  }
}
const shape = new THREE.Shape();
shape.moveTo(0, 0); shape.lineTo(20, 0); shape.lineTo(20, 10); shape.lineTo(0, 10); shape.closePath();
const extruded = new THREE.ExtrudeGeometry(shape, {depth: 20, bevelEnabled: false});
assert.deepEqual(extruded.groups.map(g => g.materialIndex), [0, 1]);
assert.equal(photos.reduce((sum, photo) => sum + photo.edge_indices.length, 0), 14);
console.log('roof UV: all building vertices and five reference points; facade geometry: four buildings, fourteen edges');
