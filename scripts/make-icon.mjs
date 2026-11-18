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
  length.writeUInt32BE(data.length);
  const name = Buffer.from(type, "ascii");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([name, data])));
  return Buffer.concat([length, name, data, crc]);
}

function encodePng(width, height, rgba) {
  const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;

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

function blend(dst, index, r, g, b, a) {
  const alpha = a / 255;
  const inv = 1 - alpha;
  dst[index] = Math.round(r * alpha + dst[index] * inv);
  dst[index + 1] = Math.round(g * alpha + dst[index + 1] * inv);
  dst[index + 2] = Math.round(b * alpha + dst[index + 2] * inv);
  dst[index + 3] = Math.round((a + (dst[index + 3] * inv)) / 1);
}

function draw(size, painter) {
  const scale = 3;
  const hi = size * scale;
  const buffer = Buffer.alloc(hi * hi * 4);
  painter((x, y, r, g, b, a) => {
    const px = Math.round(x * scale);
    const py = Math.round(y * scale);
    if (px < 0 || py < 0 || px >= hi || py >= hi) return;
    const index = (py * hi + px) * 4;
    blend(buffer, index, r, g, b, a);
  }, hi);
  const out = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      let r = 0;
      let g = 0;
      let b = 0;
      let a = 0;
      for (let dy = 0; dy < scale; dy += 1) {
        for (let dx = 0; dx < scale; dx += 1) {
          const index = ((y * scale + dy) * hi + (x * scale + dx)) * 4;
          r += buffer[index];
          g += buffer[index + 1];
          b += buffer[index + 2];
          a += buffer[index + 3];
        }
      }
      const count = scale * scale;
      const index = (y * size + x) * 4;
      out[index] = Math.round(r / count);
      out[index + 1] = Math.round(g / count);
      out[index + 2] = Math.round(b / count);
      out[index + 3] = Math.round(a / count);
    }
  }
  return encodePng(size, size, out);
}

const appSize = 1024;
const appPng = draw(appSize, (set, hi) => {
  const inset = hi * 0.06;
  const radius = hi * 0.22;
  const white = [246, 248, 250, 255];
  const ink = [29, 33, 39, 255];
  const blue = [14, 102, 200, 255];

  for (let y = 0; y < hi; y += 1) {
    for (let x = 0; x < hi; x += 1) {
      const inside =
        x >= inset &&
        y >= inset &&
        x <= hi - inset &&
        y <= hi - inset;
      if (!inside) continue;
      const cx = Math.min(Math.max(x, inset + radius), hi - inset - radius);
      const cy = Math.min(Math.max(y, inset + radius), hi - inset - radius);
      const dx = x - cx;
      const dy = y - cy;
      if (dx * dx + dy * dy > radius * radius) continue;
      set(x, y, ...white);
    }
  }

  const center = hi / 2;
  const ringRadius = hi * 0.225;
  const ringWidth = hi * 0.045;
  for (let y = 0; y < hi; y += 1) {
    for (let x = 0; x < hi; x += 1) {
      const dx = x - center;
      const dy = y - center;
      const distance = Math.sqrt(dx * dx + dy * dy);
      if (Math.abs(distance - ringRadius) <= ringWidth / 2) {
        set(x, y, ...blue);
      }
    }
  }
  for (let y = 0; y < hi; y += 1) {
    for (let x = 0; x < hi; x += 1) {
      const dx = x - center;
      const dy = y - center;
      if (dx * dx + dy * dy <= (hi * 0.085) ** 2) {
        set(x, y, ...ink);
      }
    }
  }
  for (let y = center - hi * 0.32; y <= center - hi * 0.26; y += 1) {
    for (let x = center - hi * 0.012; x <= center + hi * 0.012; x += 1) {
      set(x, y, ...blue);
    }
  }
});

writeFileSync(resolve(outDir, "icon.png"), appPng);

const traySize = 36;
const trayPng = draw(traySize, (set, hi) => {
  const center = hi / 2;
  const ringRadius = hi * 0.28;
  const ringWidth = hi * 0.085;
  for (let y = 0; y < hi; y += 1) {
    for (let x = 0; x < hi; x += 1) {
      const dx = x - center;
      const dy = y - center;
      const distance = Math.sqrt(dx * dx + dy * dy);
      if (Math.abs(distance - ringRadius) <= ringWidth / 2) {
        set(x, y, 0, 0, 0, 255);
      }
      if (distance <= hi * 0.1) {
        set(x, y, 0, 0, 0, 255);
      }
    }
  }
});

writeFileSync(resolve(outDir, "tray.png"), trayPng);

console.log(`icons written: ${outDir}`);
