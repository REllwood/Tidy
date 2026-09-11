// Verify pinned files without downloading model weights or following CDN redirects.
import fs from 'node:fs/promises';
const catalogue = JSON.parse(await fs.readFile(new URL('../src/lib/modelCatalogue.json', import.meta.url), 'utf8'));
let failures = 0;
for (const model of [...catalogue.transcription, ...catalogue.ai]) {
  try {
    const url = new URL(model.url);
    const [, , resolve, revision, ...file] = url.pathname.slice(1).split('/');
    if (url.origin !== 'https://huggingface.co' || resolve !== 'resolve' || !/^[a-f0-9]{40}$/.test(revision) || file.join('/') !== model.file) throw new Error('Unpinned or invalid URL');
    const response = await fetch(url, {method: 'HEAD', redirect: 'manual', signal: AbortSignal.timeout(30000)});
    if (response.status !== 302 && response.status !== 200) throw new Error(`HTTP ${response.status}`);
    const hash = response.headers.get('x-linked-etag')?.replaceAll('"', '');
    const size = Number(response.headers.get('x-linked-size'));
    if (hash !== model.sha256 || size !== model.size || response.headers.get('x-repo-commit') !== revision) throw new Error('Revision, size or SHA-256 differs');
    console.log(`Verified ${model.id}: pinned file, size and SHA-256`);
  } catch (error) {
    failures++;
    console.error(`${model.id}: ${error.message}`);
  }
}
if (failures) process.exitCode = 1;
