import { limits } from './limits.ts';

export interface BinaryFrame {
  kind: 1 | 2 | 3;
  payload: Uint8Array;
  header: Uint8Array;
}

function checkLength(kind: number, length: number): asserts kind is 1 | 2 | 3 {
  const bounds: Record<number, [number, number]> = {
    1: [1, limits.jsonBytes],
    2: [limits.terminalHeaderBytes, limits.terminalHeaderBytes + limits.terminalPayloadBytes],
    3: [limits.attachmentHeaderBytes, limits.attachmentHeaderBytes + limits.attachmentPayloadBytes],
  };
  const limit = bounds[kind];
  if (!limit || length < limit[0] || length > limit[1]) {
    throw new Error("Invalid frame type or length");
  }
}

export function decodeFrame(data: Uint8Array): BinaryFrame {
  if (data.length < 5) throw new Error("Truncated header");
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  const kind = view.getUint8(0);
  const length = view.getUint32(1, false);
  checkLength(kind, length);
  if (data.length !== length + 5) throw new Error("Frame size mismatch");
  const headerLength = kind === 1 ? 0 : 16;
  return { kind, header: data.slice(5, 5 + headerLength), payload: data.slice(5 + headerLength) };
}

export function encodeFrame(frame: BinaryFrame): Uint8Array {
  const headerLength = frame.kind === 1 ? 0 : 16;
  if (frame.header.length !== headerLength) throw new Error("Invalid subheader");
  const length = headerLength + frame.payload.length;
  checkLength(frame.kind, length);
  const bytes = new Uint8Array(5 + length);
  const view = new DataView(bytes.buffer);
  view.setUint8(0, frame.kind);
  view.setUint32(1, length, false);
  bytes.set(frame.header, 5);
  bytes.set(frame.payload, 5 + headerLength);
  return bytes;
}
