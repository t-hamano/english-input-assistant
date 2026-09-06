import { readFileSync } from 'node:fs';

const root = new URL('../', import.meta.url);
const read = (path) => readFileSync(new URL(path, root), 'utf8');
const field = (section, name) => section?.match(new RegExp(`^${name}\\s*=\\s*"([^"]+)"`, 'm'))?.[1];

try {
  const pkg = JSON.parse(read('package.json'));
  const cargo = read('src-tauri/Cargo.toml').match(/^\[package\]\s*\r?\n([\s\S]*?)(?=^\[|$(?![\s\S]))/m)?.[1];
  const locked = read('src-tauri/Cargo.lock').split(/^\[\[package\]\]\s*$/m)
    .filter((section) => field(section, 'name') === pkg.name);
  if (locked.length !== 1) throw new Error('Expected exactly one application package in Cargo.lock.');
  const versions = {
    'package.json': pkg.version,
    'src-tauri/tauri.conf.json': JSON.parse(read('src-tauri/tauri.conf.json')).version,
    'src-tauri/Cargo.toml': field(cargo, 'version'),
    'src-tauri/Cargo.lock': field(locked[0], 'version'),
  };
  const semver = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;
  if (typeof pkg.version !== 'string' || !semver.test(pkg.version)) {
    throw new Error(`Invalid application version: ${pkg.version}`);
  }
  for (const [path, version] of Object.entries(versions)) {
    if (version !== pkg.version) throw new Error(`${path}: expected ${pkg.version}, got ${version}`);
  }
  if (process.argv.includes('--release') && process.env.RELEASE_TAG !== `v${pkg.version}`) {
    throw new Error(`Expected release tag v${pkg.version}, got ${process.env.RELEASE_TAG ?? '(missing)'}`);
  }
  console.log(`Application versions match: ${pkg.version}`);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
