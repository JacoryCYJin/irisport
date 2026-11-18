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

function flattenCurve(start, curves) {
  const points = [start];
  let [x, y] = start;
  for (const [ax, ay, bx, by, endX, endY] of curves) {
    for (let step = 1; step <= 20; step += 1) {
      const t = step / 20;
      const s = 1 - t;
      points.push([
        s ** 3 * x + 3 * s ** 2 * t * ax + 3 * s * t ** 2 * bx + t ** 3 * endX,
        s ** 3 * y + 3 * s ** 2 * t * ay + 3 * s * t ** 2 * by + t ** 3 * endY,
      ]);
    }
    x = endX;
    y = endY;
  }
  return points;
}

function coversStroke(points, x, y, radiusSquared) {
  for (let i = 1; i < points.length; i += 1) {
    const [startX, startY] = points[i - 1];
    const [endX, endY] = points[i];
    const dx = endX - startX;
    const dy = endY - startY;
    const offsetX = x - startX;
    const offsetY = y - startY;
    const fraction = (offsetX * dx + offsetY * dy) / (dx * dx + dy * dy);
    const t = Math.min(1, Math.max(0, fraction));
    if ((offsetX - t * dx) ** 2 + (offsetY - t * dy) ** 2 <= radiusSquared) {
      return true;
    }
  }
  return false;
}

function containsPoint(points, x, y) {
  let inside = false;
  for (let i = 1; i < points.length; i += 1) {
    const [startX, startY] = points[i - 1];
    const [endX, endY] = points[i];
    if ((startY > y) !== (endY > y)) {
      const crossingX = startX + ((y - startY) * (endX - startX)) / (endY - startY);
      if (x < crossingX) inside = !inside;
    }
  }
  return inside;
}

function writeTrayIcons() {
  const canvasPoints = 18;
  const pixels = canvasPoints * 2;
  const samples = 4;
  const filenames = ["tray.png", "tray-lit.png", "tray-lit-medium.png", "tray-lit-high.png"];
  const images = filenames.map(() => Buffer.alloc(pixels * pixels * 4));

  // Bulb and socket, drawn in point coordinates with a 1.6 pt stroke.
  const outline = flattenCurve([6.25, 11.7], [
    [6.25, 10.6, 5.65, 10, 4.9, 9.2],
    [4.18, 8.42, 3.85, 7.45, 3.85, 6.35],
    [3.85, 3.65, 6.12, 1.9, 9, 1.9],
    [11.88, 1.9, 14.15, 3.65, 14.15, 6.35],
    [14.15, 7.45, 13.82, 8.42, 13.1, 9.2],
    [12.35, 10, 11.75, 10.6, 11.75, 11.7],
  ]);
  outline.push([11.75, 12.65], [6.25, 12.65], [6.25, 11.7]);
  const base = [[6.85, 15.1], [11.15, 15.1]];
  const filament = flattenCurve([6.6, 6.25], [
    [7.55, 6.1, 7.89, 8.45, 9, 8.5],
    [10.11, 8.45, 10.45, 6.1, 11.4, 6.25],
  ]);
  const support = [[9, 8.45], [9, 10.05]];
  const radiusSquared = 0.8 ** 2;

  function opacityAt(x, y) {
    if (
      coversStroke(outline, x, y, radiusSquared) ||
      coversStroke(base, x, y, radiusSquared)
    ) {
      return [1, 1, 1, 1];
    }
    if (!containsPoint(outline, x, y)) return [0, 0, 0, 0];
    if (coversStroke(filament, x, y, 0.6 ** 2)) return [0, 1, 1, 1];
    if (coversStroke(support, x, y, 0.36 ** 2)) return [0, 0.8, 0.8, 0.8];

    const halo = 0.38 * Math.exp(-(((x - 9) / 3.4) ** 2 + ((y - 7) / 3.85) ** 2));
    const fade = Math.min(1, Math.max(0, (12.5 - y) / 9.5));
    const glass = Math.max(halo, Math.min(0.58, 0.12 + 0.48 * fade));
    return [0, 0, halo, glass];
  }

  const pointsPerSample = canvasPoints / (pixels * samples);
  for (let y = 0; y < pixels; y += 1) {
    for (let x = 0; x < pixels; x += 1) {
      const coverage = [0, 0, 0, 0];
      for (let dy = 0; dy < samples; dy += 1) {
        for (let dx = 0; dx < samples; dx += 1) {
          const sampleX = (x * samples + dx + 0.5) * pointsPerSample;
          const sampleY = (y * samples + dy + 0.5) * pointsPerSample;
          const opacity = opacityAt(sampleX, sampleY);
          for (let level = 0; level < images.length; level += 1) {
            coverage[level] += opacity[level];
          }
        }
      }
      const alphaIndex = (y * pixels + x) * 4 + 3;
      for (let level = 0; level < images.length; level += 1) {
        images[level][alphaIndex] = Math.round((255 * coverage[level]) / (samples * samples));
      }
    }
  }
  const crop = 2;
  const croppedWidth = pixels - crop * 2;
  for (const [level, filename] of filenames.entries()) {
    const source = images[level];
    const cropped = Buffer.alloc(croppedWidth * pixels * 4);
    for (let y = 0; y < pixels; y += 1) {
      const sourceStart = (y * pixels + crop) * 4;
      const targetStart = y * croppedWidth * 4;
      source.copy(cropped, targetStart, sourceStart, sourceStart + croppedWidth * 4);
    }
    writeFileSync(
      resolve(outDir, filename),
      encodePng(croppedWidth, pixels, cropped),
    );
  }
}

if (process.argv.includes("--tray-only")) {
  writeTrayIcons();
  console.log(`tray icons written: ${outDir}`);
  process.exit(0);
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
writeTrayIcons();

console.log(`icons written: ${outDir}`);
