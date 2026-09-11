import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { decodeFrame, encodeFrame } from '../src/frame.ts';

const vectors = JSON.parse(readFileSync(new URL('../../vectors/frames.json', import.meta.url), 'utf8'));
for (const vector of vectors) {
  test(`binary vector: ${vector.name}`, () => {
    const wire = Buffer.from(vector.hex, 'hex');
    const frame = decodeFrame(wire);
    assert.equal(frame.kind, vector.kind);
    assert.equal(Buffer.from(frame.payload).toString('hex'), vector.payloadHex);
    if (frame.kind === 2) {
      const header = new DataView(frame.header.buffer, frame.header.byteOffset, frame.header.byteLength);
      assert.equal(header.getUint32(0, false), vector.paneRef);
      assert.equal(header.getUint32(4, false), vector.generation);
      assert.equal(header.getBigUint64(8, false).toString(), vector.offset);
    }
    if (frame.kind === 3) assert.equal(Buffer.from(frame.header).toString('hex'), vector.headerHex);
    assert.deepEqual(Buffer.from(encodeFrame(frame)), wire);
  });
}

test('oversize, truncation and trailing data fail', () => {
  for (const hex of ['02ffffffff', '020000000f', '01000000027b', '0000000001ff', '01000000027b7d00']) {
    assert.throws(() => decodeFrame(Buffer.from(hex, 'hex')));
  }
});
