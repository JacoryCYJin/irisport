import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = resolve(root, "src-tauri/icons");
mkdirSync(outDir, { recursive: true });

const crcTable = new Uint32Array(256);
for (let n = 0; n < 256; n += 1) {
  let c = n;
  for (let k = 0; k < 8; k += 1) {
    c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  }
  crcTable[n] = c >>> 0;
}

function crc32(buffer) {
  let c = 0xffffffff;
  for (const byte of buffer) {
    c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  }
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length, 0);
  const name = Buffer.from(type, "ascii");
  const checksum = Buffer.alloc(4);
  checksum.writeUInt32BE(crc32(Buffer.concat([name, data])), 0);
  return Buffer.concat([length, name, data, checksum]);
}

function encodePng(width, height, rgba) {
  const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;

  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y += 1) {
    raw[y * (stride + 1)] = 0;
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  return Buffer.concat([
    signature,
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function inPortRing(x, y, cx, cy, radius, stroke, gapCenter, gapHalf) {
  const dx = x - cx;
  const dy = y - cy;
  const distance = Math.hypot(dx, dy);
  if (Math.abs(distance - radius) > stroke / 2) return false;
  const angle = Math.atan2(dy, dx);
  const difference = Math.atan2(Math.sin(angle - gapCenter), Math.cos(angle - gapCenter));
  return Math.abs(difference) > gapHalf;
}

function inCircle(x, y, cx, cy, radius) {
  return (x - cx) ** 2 + (y - cy) ** 2 <= radius ** 2;
}

function inCircleOutline(x, y, cx, cy, radius, stroke) {
  const distance = Math.hypot(x - cx, y - cy);
  return Math.abs(distance - radius) <= stroke / 2;
}

function writeTrayIcons() {
  const canvasPoints = 18;
  const pixels = canvasPoints * 2;
  const samples = 4;
  const pointsPerSample = canvasPoints / (pixels * samples);

  for (const [name, unread] of [["tray.png", false], ["tray-unread.png", true]]) {
    const rgba = Buffer.alloc(pixels * pixels * 4);
    for (let y = 0; y < pixels; y += 1) {
      for (let x = 0; x < pixels; x += 1) {
        let covered = 0;
        for (let dy = 0; dy < samples; dy += 1) {
          for (let dx = 0; dx < samples; dx += 1) {
            const sampleX = (x * samples + dx + 0.5) * pointsPerSample;
            const sampleY = (y * samples + dy + 0.5) * pointsPerSample;
            const ring = inPortRing(sampleX, sampleY, 8.25, 9.3, 5.35, 1.55, -0.8, 0.36);
            const dot = unread
              ? inCircle(sampleX, sampleY, 13.45, 4.1, 1.65)
              : inCircleOutline(sampleX, sampleY, 13.45, 4.1, 1.45, 0.9);
            if (ring || dot) covered += 1;
          }
        }
        const index = (y * pixels + x) * 4;
        rgba[index + 3] = Math.round((255 * covered) / (samples * samples));
      }
    }
    writeFileSync(resolve(outDir, name), encodePng(pixels, pixels, rgba));
  }
}

function drawAppIcon() {
  const size = 1024;
  const scale = 3;
  const hi = size * scale;
  const buffer = Buffer.alloc(hi * hi * 4);
  const set = (x, y, r, g, b) => {
    if (x < 0 || y < 0 || x >= hi || y >= hi) return;
    const index = (y * hi + x) * 4;
    buffer[index] = r;
    buffer[index + 1] = g;
    buffer[index + 2] = b;
    buffer[index + 3] = 255;
  };

  for (let y = 0; y < hi; y += 1) {
    for (let x = 0; x < hi; x += 1) {
      const vertical = y / hi;
      const horizontal = x / hi;
      const light = Math.max(0, 1 - Math.hypot(horizontal - 0.18, vertical - 0.08));
      set(
        x,
        y,
        Math.round(20 + 22 * (1 - vertical) + 8 * light),
        Math.round(31 + 25 * (1 - vertical) + 12 * light),
        Math.round(51 + 30 * (1 - vertical) + 18 * light),
      );
    }
  }

  const centerX = hi * 0.44;
  const centerY = hi * 0.54;
  const radius = hi * 0.245;
  const stroke = hi * 0.09;
  const dotX = hi * 0.71;
  const dotY = hi * 0.315;
  const dotRadius = hi * 0.103;

  for (let y = 0; y < hi; y += 1) {
    for (let x = 0; x < hi; x += 1) {
      if (inPortRing(x, y, centerX, centerY, radius, stroke, -0.8, 0.36)) {
        const shade = Math.round(244 + 11 * (1 - y / hi));
        set(x, y, shade, Math.min(255, shade + 4), 255);
      }
      if (inCircle(x, y, dotX, dotY, dotRadius)) {
        const glow = Math.max(0, 1 - Math.hypot(x - dotX, y - dotY) / dotRadius);
        set(x, y, Math.round(72 + 45 * glow), Math.round(174 + 45 * glow), 255);
      }
    }
  }

  const out = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      const color = [0, 0, 0, 0];
      for (let dy = 0; dy < scale; dy += 1) {
        for (let dx = 0; dx < scale; dx += 1) {
          const index = ((y * scale + dy) * hi + x * scale + dx) * 4;
          for (let channel = 0; channel < 4; channel += 1) {
            color[channel] += buffer[index + channel];
          }
        }
      }
      const index = (y * size + x) * 4;
      for (let channel = 0; channel < 4; channel += 1) {
        out[index + channel] = Math.round(color[channel] / (scale * scale));
      }
    }
  }
  writeFileSync(resolve(outDir, "icon.png"), encodePng(size, size, out));
}

if (!process.argv.includes("--tray-only")) {
  drawAppIcon();
}
writeTrayIcons();
console.log(`icons written: ${outDir}`);
