/** Reader for the Rust engine's mdsim-frames-v1 record format.
 * It does not start a simulation or connect a data source to the UI.
 */
const RECORD_BYTES = 14;
const POSITION_SCALE_M = 0.05;
const FIELDS = [
  'body:u32', 'x:i16', 'z:i16', 'heading:u8', 'gaze:u8',
  'focal_range_m:u8', 'focal_angle_deg:u8', 'activity:u8', 'flags:u8',
] as const;

export type FrameHeader = Readonly<{
  format: 'mdsim-frames-v1';
  tick_start: number;
  tick_end: number;
  dt_s: number;
  render_every: number;
  n_frames: number;
  record_bytes: 14;
  origin: readonly [number, number];
  pos_scale_m: number;
  fields: readonly string[];
}>;

export type FrameRecord = Readonly<{
  body: number;
  x_m: number;
  z_m: number;
  heading_rad: number;
  gaze_rad: number;
  focal_range_m: number;
  focal_angle_deg: number;
  activity: number;
  flags: number;
}>;
export type Frame = Readonly<{ tick: number; records: FrameRecord[] }>;
export type FrameChunk = Readonly<{
  header: FrameHeader;
  frameCount: number;
  readFrame(index: number): Frame;
}>;

type FrameOffset = { tick: number; count: number; recordsOffset: number };

function isUint32(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 0xffffffff;
}

function validateHeader(value: unknown): FrameHeader {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid frame header');
  const h = value as Record<string, unknown>;
  if (h.format !== 'mdsim-frames-v1' || h.record_bytes !== RECORD_BYTES || h.pos_scale_m !== POSITION_SCALE_M) {
    throw new Error('Unsupported frame format');
  }
  if (!Array.isArray(h.origin) || h.origin.length !== 2 || h.origin[0] !== 0 || h.origin[1] !== 0) {
    throw new Error('Unsupported frame origin');
  }
  if (!Array.isArray(h.fields) || h.fields.length !== FIELDS.length || h.fields.some((f, i) => f !== FIELDS[i])) {
    throw new Error('Unsupported frame fields');
  }
  if (!isUint32(h.tick_start) || !isUint32(h.tick_end) || h.tick_end < h.tick_start ||
      !isUint32(h.n_frames) || !isUint32(h.render_every) || h.render_every === 0 ||
      typeof h.dt_s !== 'number' || !Number.isFinite(h.dt_s) || h.dt_s <= 0) {
    throw new Error('Invalid frame header values');
  }
  return h as FrameHeader;
}

/** The caller must not modify the provided bytes while using the returned chunk. */
export function decodeFrameChunk(input: ArrayBuffer | Uint8Array): FrameChunk {
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
  const newline = bytes.indexOf(10);
  if (newline < 0) throw new Error('Missing frame header');
  let value: unknown;
  try {
    value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes.subarray(0, newline)));
  } catch {
    throw new Error('Invalid frame header JSON');
  }
  const header = validateHeader(value);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const offsets: FrameOffset[] = [];
  let cursor = newline + 1;
  let expectedTick = Math.ceil(header.tick_start / header.render_every) * header.render_every;
  while (cursor < bytes.byteLength) {
    if (bytes.byteLength - cursor < 8) throw new Error('Truncated frame header');
    const tick = view.getUint32(cursor, true);
    const count = view.getUint32(cursor + 4, true);
    cursor += 8;
    if (tick !== expectedTick || tick >= header.tick_end) {
      throw new Error('Invalid frame tick');
    }
    if (count > Math.floor((bytes.byteLength - cursor) / RECORD_BYTES)) throw new Error('Truncated frame records');
    offsets.push({ tick, count, recordsOffset: cursor });
    expectedTick += header.render_every;
    cursor += count * RECORD_BYTES;
  }
  if (offsets.length !== header.n_frames || expectedTick < header.tick_end) {
    throw new Error('Frame count does not match header');
  }

  return {
    header,
    frameCount: offsets.length,
    readFrame(index: number): Frame {
      if (!Number.isInteger(index) || index < 0 || index >= offsets.length) throw new RangeError('Frame index out of range');
      const frame = offsets[index];
      const records: FrameRecord[] = [];
      for (let i = 0; i < frame.count; i += 1) {
        const at = frame.recordsOffset + i * RECORD_BYTES;
        records.push({
          body: view.getUint32(at, true),
          x_m: view.getInt16(at + 4, true) * POSITION_SCALE_M,
          z_m: view.getInt16(at + 6, true) * POSITION_SCALE_M,
          heading_rad: view.getUint8(at + 8) * Math.PI * 2 / 256,
          gaze_rad: view.getUint8(at + 9) * Math.PI * 2 / 256,
          focal_range_m: view.getUint8(at + 10),
          focal_angle_deg: view.getUint8(at + 11),
          activity: view.getUint8(at + 12),
          flags: view.getUint8(at + 13),
        });
      }
      return { tick: frame.tick, records };
    },
  };
}
