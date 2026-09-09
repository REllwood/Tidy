import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';

const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
const tag = process.env.RELEASE_TAG;
if (!/^\d+\.\d+\.\d+$/.test(config.version) || tag !== `v${config.version}` || pkg.version !== config.version) {
  throw new Error('Release tag must be vX.Y.Z and match package.json and tauri.conf.json.');
}
// A manual run must build the selected tag, and must never invent a new tag.
const tagged = execFileSync('git', ['rev-parse', '--verify', `refs/tags/${tag}^{commit}`], { encoding: 'utf8' }).trim();
const head = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
if (tagged !== head) throw new Error('The checkout does not match the requested release tag.');
console.log(`Release version and checkout verified: ${tag}`);
