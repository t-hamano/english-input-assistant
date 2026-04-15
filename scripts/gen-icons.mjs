/**
 * Generates icons for macOS and Windows from translate.svg.
 * Run with: npm run gen-icons
 *
 * Outputs:
 *   icon.png           - 512×512 app icon (macOS / general)
 *   icon.icns          - macOS bundle icon (all sizes, macOS squircle corner radius)
 *   icon.ico           - Windows icon (16–256px)
 *   tray-icon-mac.png  - macOS menu bar template icon (44×44, black on transparent)
 *   32x32.png / 64x64.png / 128x128.png / 128x128@2x.png
 */
import sharp from 'sharp';
import { execSync } from 'child_process';
import { mkdirSync, rmSync, writeFileSync } from 'fs';
import { join } from 'path';

const ICON_DIR = './src-tauri/icons';
const BG_COLOR = '#3858e9';

// Apple HIG: 224 / 1024 ≈ 21.875%
const MACOS_RADIUS_RATIO = 0.21875;

// Path data from translate.svg (viewBox="0 -960 960 960")
const TRANSLATE_PATH =
  'm476-80 182-480h84L924-80h-84l-43-122H603L560-80h-84Z' +
  'M160-200l-56-56 202-202q-35-35-63.5-80T190-640h84q20 39 40 68t48 58' +
  'q33-33 68.5-92.5T484-720H40v-80h280v-80h80v80h280v80H564' +
  'q-21 72-63 148t-83 116l96 98-30 82-122-125-202 201Z' +
  'm468-72h144l-72-204-72 204Z';

/**
 * SVG transform that maps the icon's coordinate space (viewBox 0 -960 960 960)
 * into a [padding, size-padding] box within a canvas of `size` pixels.
 */
function iconTransform(size, padding) {
  const scale = (size - padding * 2) / 960;
  return `translate(${padding}, ${size - padding}) scale(${scale})`;
}

/** App icon SVG: colored background + white symbol. */
function appIconSvg(size) {
  const r = Math.round(size * MACOS_RADIUS_RATIO);
  const padding = Math.round(size * 0.1);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}">
  <rect width="${size}" height="${size}" rx="${r}" ry="${r}" fill="${BG_COLOR}"/>
  <path transform="${iconTransform(size, padding)}" fill="white" d="${TRANSLATE_PATH}"/>
</svg>`;
}

/** macOS menu bar template icon: black symbol on transparent background. */
function trayIconSvg(size) {
  const padding = Math.round(size * 0.1);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}">
  <path transform="${iconTransform(size, padding)}" fill="black" d="${TRANSLATE_PATH}"/>
</svg>`;
}

async function svgToPng(svgString, density) {
  let s = sharp(Buffer.from(svgString));
  if (density) s = s.withMetadata({ density });
  return s.png().toBuffer();
}

// ── General PNGs ────────────────────────────────────────────────────────────

async function genPngs() {
  const specs = [
    { file: '32x32.png',      size: 32 },
    { file: '64x64.png',      size: 64 },
    { file: '128x128.png',    size: 128 },
    { file: '128x128@2x.png', size: 256 },
    { file: 'icon.png',       size: 512 },
  ];
  for (const { file, size, density } of specs) {
    await sharp(await svgToPng(appIconSvg(size), density)).toFile(`${ICON_DIR}/${file}`);
    console.log(`  ${file} (${size}×${size})`);
  }
}

// ── macOS menu bar template icon ─────────────────────────────────────────────

async function genTrayIconMac() {
  await sharp(await svgToPng(trayIconSvg(44))).toFile(`${ICON_DIR}/tray-icon-mac.png`);
  console.log('  tray-icon-mac.png (44×44, black template)');
}

// ── macOS .icns ──────────────────────────────────────────────────────────────

async function genIcns() {
  const tmpDir = '/tmp/app.iconset';
  rmSync(tmpDir, { recursive: true, force: true });
  mkdirSync(tmpDir);

  for (const s of [16, 32, 128, 256, 512]) {
    await sharp(await svgToPng(appIconSvg(s))).toFile(join(tmpDir, `icon_${s}x${s}.png`));
    await sharp(await svgToPng(appIconSvg(s * 2))).toFile(join(tmpDir, `icon_${s}x${s}@2x.png`));
    console.log(`  icns: ${s}×${s} + @2x`);
  }

  execSync(`iconutil --convert icns ${tmpDir} --output ${ICON_DIR}/icon.icns`);
  console.log('  icon.icns');
}

// ── Windows .ico ─────────────────────────────────────────────────────────────

async function genIco() {
  const tmpDir = '/tmp/win-ico';
  rmSync(tmpDir, { recursive: true, force: true });
  mkdirSync(tmpDir);

  const sizes = [16, 32, 48, 64, 128, 256];
  const paths = [];
  for (const s of sizes) {
    const p = join(tmpDir, `${s}.png`);
    await sharp(await svgToPng(appIconSvg(s))).toFile(p);
    paths.push(p);
    console.log(`  ico: ${s}×${s}`);
  }

  const { default: pngToIco } = await import('png-to-ico');
  const ico = await pngToIco(paths);
  writeFileSync(`${ICON_DIR}/icon.ico`, ico);
  console.log('  icon.ico');
}

// ── Main ─────────────────────────────────────────────────────────────────────

async function main() {
  console.log('Generating icons from translate.svg...\n');
  await genPngs();
  await genTrayIconMac();
  await genIcns();
  await genIco();
  console.log('\nDone!');
}

main().catch((e) => { console.error(e); process.exit(1); });
