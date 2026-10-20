// Релиз Горна Next: подписанный установщик NSIS и latest.json для обновлятора.
//
//   set TAURI_SIGNING_PRIVATE_KEY=%USERPROFILE%\.tauri\gorn-next.key
//   set TAURI_SIGNING_PRIVATE_KEY_PASSWORD=
//   set CARGO_TARGET_DIR=target\dist
//   ui\node_modules\.bin\tauri build
//   node tools/release.mjs [--notes "что нового"] [--publish]
//
// Без --publish файлы только складываются в target/dist/release-v<версия>;
// с --publish — релиз v<версия> на GitHub (gh), он же становится latest:
// обновлятор установленных копий читает
// https://github.com/lanqvist/GornNext/releases/latest/download/latest.json.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const conf = JSON.parse(readFileSync(join(root, 'app/tauri.conf.json'), 'utf8'));
const version = conf.version;
const repo = 'lanqvist/GornNext';
const tag = `v${version}`;

const arg = (name) => {
  const i = process.argv.indexOf(name);
  return i === -1 ? null : (process.argv[i + 1] ?? '');
};
const notes = arg('--notes') ?? `Горн Next ${version}`;
const publish = process.argv.includes('--publish');

const nsis = join(root, 'target/dist/release/bundle/nsis');
const setup = readdirSync(nsis).find((f) => f.endsWith(`_${version}_x64-setup.exe`));
if (!setup || !existsSync(join(nsis, `${setup}.sig`))) {
  console.error(`нет подписанного установщика ${version} в ${nsis} — сначала tauri build с ключом подписи`);
  process.exit(1);
}

// Имя вложения — латиницей: кириллицу и пробелы GitHub в именах файлов заменяет.
const asset = `GornNext-${version}-setup.exe`;
const out = join(root, `target/dist/release-${tag}`);
mkdirSync(out, { recursive: true });
copyFileSync(join(nsis, setup), join(out, asset));
const signature = readFileSync(join(nsis, `${setup}.sig`), 'utf8').trim();
const latest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    'windows-x86_64': {
      signature,
      url: `https://github.com/${repo}/releases/download/${tag}/${asset}`,
    },
  },
};
writeFileSync(join(out, 'latest.json'), JSON.stringify(latest, null, 2) + '\n');
console.log(`готово: ${out}\n  ${asset}\n  latest.json`);

if (publish) {
  execFileSync(
    'gh',
    ['release', 'create', tag, join(out, asset), join(out, 'latest.json'), '--repo', repo, '--title', `Горн Next ${version}`, '--notes', notes, '--latest'],
    { stdio: 'inherit' },
  );
}
