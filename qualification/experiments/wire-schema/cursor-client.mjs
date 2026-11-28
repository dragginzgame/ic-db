// Run the generated method and runtime against the first Rust page.
import assert from 'node:assert/strict';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { json } from 'node:stream/consumers';
import { pathToFileURL } from 'node:url';

const { module, runtime, reply } = await json(process.stdin);
const dir = await mkdtemp(join(tmpdir(), 'ic-db-cursor-'));
try {
  await writeFile(join(dir, 'runtime.ts'), runtime);
  await writeFile(join(dir, 'page.ts'), module);
  const { page } = await import(pathToFileURL(join(dir, 'page.ts')));
  const { id, next } = page.decodeResponse(Uint8Array.from(reply));
  assert.deepEqual(Array.from(id), [...Array(15).fill(0), 1]);
  assert.ok(next.some instanceof Uint8Array);
  assert.throws(() => page.encodeRequest({ after: { some: new Uint8Array(547) } }));
  process.stdout.write(page.encodeRequest({ after: next }));
} finally {
  await rm(dir, { recursive: true, force: true });
}
