import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { decodeNamed, decodeParams } from '../src/codec.ts';

const vectors = JSON.parse(readFileSync(new URL('../../vectors/messages.json', import.meta.url), 'utf8'));
for (const vector of vectors) {
  test(`message vector: ${vector.name}`, () => {
    const value = decodeNamed(vector.type, JSON.stringify(vector.value));
    assert.deepEqual(JSON.parse(JSON.stringify(value)), vector.value);
  });
}

test('rejects missing key, wrong scalar, enum and bounds', () => {
  const valid = { paneId: 'pane-1', cols: 80, rows: 24, idempotencyKey: 'key-1' };
  assert.equal(decodeParams('pane.resize', JSON.stringify(valid)).cols, 80);
  for (const patch of [{ cols: '80' }, { rows: 0 }, { cols: 1001 }, { idempotencyKey: null }]) {
    assert.throws(() => decodeParams('pane.resize', JSON.stringify({ ...valid, ...patch })));
  }
  assert.throws(() => decodeParams('pane.resize', '{}'));
  assert.throws(() => decodeNamed('PaneState', '"future_state"'));
});

test('unknown fields remain compatible with the next protocol version', () => {
  const value = decodeParams('pane.close', '{"paneId":"pane-1","idempotencyKey":"key-1","future":true}');
  assert.equal(value.paneId, 'pane-1');
});

test('shared negative message vectors fail', () => {
  const negatives = JSON.parse(readFileSync(new URL('../../vectors/invalid-messages.json', import.meta.url), 'utf8'));
  for (const vector of negatives) assert.throws(() => decodeNamed(vector.type, JSON.stringify(vector.value)));
});
