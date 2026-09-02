// Generates the placeholder icon set and the bundled fallback wallpaper.
// Run with: node scripts/gen-assets.mjs
import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const iconsDir = resolve(here, "../src-tauri/icons");
const resourcesDir = resolve(here, "../src-tauri/resources");
mkdirSync(iconsDir, { recursive: true });
mkdirSync(resourcesDir, { recursive: true });

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

/** pixel(x, y) -> [r, g, b, a] */
function png(width, height, pixel) {
  const raw = Buffer.alloc(height * (width * 4 + 1));
  let o = 0;
  for (let y = 0; y < height; y++) {
    raw[o++] = 0; // filter: none
    for (let x = 0; x < width; x++) {
      const [r, g, b, a] = pixel(x, y);
      raw[o++] = r;
      raw[o++] = g;
      raw[o++] = b;
      raw[o++] = a;
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

// --- icon: a rounded blue tile with a lighter diagonal "flow" band ---
function iconPixel(size) {
  return (x, y) => {
    const r = size * 0.22;
    const inset = size * 0.06;
    const min = inset;
    const max = size - inset;
    const cx = Math.min(Math.max(x, min + r), max - r);
    const cy = Math.min(Math.max(y, min + r), max - r);
    const outside =
      x < min || y < min || x > max || y > max || Math.hypot(x - cx, y - cy) > r;
    if (outside) return [0, 0, 0, 0];
    const band = (x + y) / (size * 2);
    const t = Math.abs(Math.sin(band * Math.PI * 1.4));
    const rr = Math.round(48 + t * 60);
    const gg = Math.round(104 + t * 60);
    const bb = Math.round(232 + t * 20);
    return [rr, gg, bb, 255];
  };
}

for (const size of [32, 128, 256, 512]) {
  const name = size === 256 ? "128x128@2x.png" : `${size}x${size}.png`;
  writeFileSync(resolve(iconsDir, name), png(size, size, iconPixel(size)));
}
writeFileSync(resolve(iconsDir, "icon.png"), png(512, 512, iconPixel(512)));

// --- .ico containing a single 256px PNG entry (Vista+ reads PNG-in-ICO) ---
const icoPng = png(256, 256, iconPixel(256));
const header = Buffer.alloc(6);
header.writeUInt16LE(0, 0);
header.writeUInt16LE(1, 2); // type: icon
header.writeUInt16LE(1, 4); // one image
const entry = Buffer.alloc(16);
entry[0] = 0; // 0 means 256
entry[1] = 0;
entry[2] = 0; // colors
entry[3] = 0;
entry.writeUInt16LE(1, 4); // planes
entry.writeUInt16LE(32, 6); // bpp
entry.writeUInt32LE(icoPng.length, 8);
entry.writeUInt32LE(6 + 16, 12);
writeFileSync(resolve(iconsDir, "icon.ico"), Buffer.concat([header, entry, icoPng]));

// --- bundled fallback wallpaper: a calm vertical gradient ---
const W = 1920;
const H = 1080;
const wallpaper = png(W, H, (x, y) => {
  const v = y / H;
  const u = x / W;
  const r = Math.round(18 + v * 34 + u * 8);
  const g = Math.round(26 + v * 46 + u * 10);
  const b = Math.round(46 + v * 70 + u * 16);
  return [r, g, b, 255];
});
writeFileSync(resolve(resourcesDir, "default-wallpaper.png"), wallpaper);

console.log("Wrote icons to src-tauri/icons and the fallback wallpaper to src-tauri/resources");
