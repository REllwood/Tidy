import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
export function updateManifest(version, signature, notes = '') {
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error('Use a stable release version such as 0.2.2.');
  if (!signature.trim() || !/^[A-Za-z0-9+/=\s]+$/.test(signature)) throw new Error('An updater signature is required.');
  return {
    version,
    notes,
    pub_date: new Date().toISOString(),
    platforms: {
      'darwin-aarch64': {
        signature: signature.trim(),
        url: `https://github.com/REllwood/Tidy/releases/download/v${version}/Tidy.app.tar.gz`,
      },
    },
  };
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const [dir, version, notesFile] = process.argv.slice(2);
  const signature = fs.readFileSync(path.join(dir, 'Tidy.app.tar.gz.sig'), 'utf8');
  const notes = notesFile ? fs.readFileSync(notesFile, 'utf8') : `Tidy ${version}`;
  fs.writeFileSync(path.join(dir, 'latest.json'), JSON.stringify(updateManifest(version,signature,notes),null,2)+'\n', {flag:'wx'});
}
