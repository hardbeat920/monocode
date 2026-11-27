import { zlibSync } from "fflate";

/** An image as RGBA, `width` × `height`. */
export type Source = {
  width: number;
  height: number;
  pixels: Uint8Array;
};

// The desktop's dither effect, from
// src/features/settings/model/newThreadBackgroundEffects.worker.ts, so both
// draw the same artwork alike.

function coverIndex(source: Source, x: number, y: number) {
  const sourceX = Math.max(0, Math.min(source.width - 1, Math.floor(x)));
  const sourceY = Math.max(0, Math.min(source.height - 1, Math.floor(y)));
  return sourceY * source.width + sourceX;
}

const BAYER = [
  [0, 8, 2, 10],
  [12, 4, 14, 6],
  [3, 11, 1, 9],
  [15, 7, 13, 5],
];

/** The image rebuilt with a dithered colour palette, in 2×2 blocks, as RGBA. */
export function ditherPixels(source: Source) {
  const output = new Uint8Array(source.pixels.length);
  const color = new Uint8Array(4);
  for (let y = 0; y < source.height; y += 2) {
    for (let x = 0; x < source.width; x += 2) {
      const base =
        coverIndex(source, Math.min(x + 1, source.width - 1), Math.min(y + 1, source.height - 1)) *
        4;
      const r = source.pixels[base];
      const g = source.pixels[base + 1];
      const b = source.pixels[base + 2];
      const peak = Math.max(r, g, b);
      const threshold = BAYER[Math.floor(y / 2) % 4][Math.floor(x / 2) % 4];
      const gain = peak / 255 > (threshold + 0.5) / 16 ? 255 / Math.max(peak, 1) : 0.08;
      color[0] = Math.min(255, Math.round(r * gain));
      color[1] = Math.min(255, Math.round(g * gain));
      color[2] = Math.min(255, Math.round(b * gain));
      color[3] = source.pixels[base + 3];
      for (let dy = 0; dy < Math.min(2, source.height - y); dy += 1) {
        for (let dx = 0; dx < Math.min(2, source.width - x); dx += 1) {
          output.set(color, ((y + dy) * source.width + x + dx) * 4);
        }
      }
    }
  }
  return output;
}

// The fade of desktop's "Haze" chat background (`.gradient-blur-background`
// in src/styles/index.css), stops copied from there: the image darkens toward
// the bottom under an overlay of the background colour, and fades out into
// the background by 90% of the height.

/** How much of the image shows, top to bottom. */
const MASK = {
  values: [1, 1, 0.94, 0.72, 0.42, 0.16, 0.04, 0],
  locations: [0, 0.19, 0.3, 0.42, 0.54, 0.66, 0.78, 0.9],
};
/** How much of the background colour lies over it, top to bottom. */
const OVERLAY = {
  values: [0, 0.26, 0.55, 0.78],
  locations: [0.16, 0.38, 0.55, 0.75],
};

/** A gradient's value at `t`, held at its first and last stops outside them. */
function gradientAt({ values, locations }: { values: number[]; locations: number[] }, t: number) {
  if (t <= locations[0]) return values[0];
  for (let i = 1; i < locations.length; i += 1) {
    if (t <= locations[i]) {
      const span = (t - locations[i - 1]) / (locations[i] - locations[i - 1]);
      return values[i - 1] + (values[i] - values[i - 1]) * span;
    }
  }
  return values[values.length - 1];
}

/**
 * The image with its fade baked into its alpha, so whatever colour the screen
 * is shows through as the mask and overlay would let it. Drawn with an opacity
 * over that colour, it looks as the image would through them, without the
 * masked layers the phone would composite off screen every frame the screen
 * moves, and without being drawn again for each scheme's or theme's colour.
 */
export function fadeOut(source: Source) {
  const output = new Uint8Array(source.pixels);
  for (let y = 0; y < source.height; y += 1) {
    const t = (y + 0.5) / source.height;
    // Under the overlay, then through the mask, both letting the screen through.
    const alpha = Math.round(255 * (1 - gradientAt(OVERLAY, t)) * gradientAt(MASK, t));
    for (let x = 0; x < source.width; x += 1) output[(y * source.width + x) * 4 + 3] = alpha;
  }
  return output;
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(bytes: Uint8Array) {
  let c = 0xffffffff;
  for (let i = 0; i < bytes.length; i += 1) c = CRC_TABLE[(c ^ bytes[i]) & 255] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type: string, data: Uint8Array) {
  const out = new Uint8Array(12 + data.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, data.length);
  for (let i = 0; i < 4; i += 1) out[4 + i] = type.charCodeAt(i);
  out.set(data, 8);
  view.setUint32(8 + data.length, crc32(out.subarray(4, 8 + data.length)));
  return out;
}

/**
 * RGBA as a PNG: lossless, as JPEG would blur the effects' dots. Rows are
 * unfiltered, which suits their hard-edged, repeating patterns well.
 */
export function encodePng(width: number, height: number, rgba: Uint8Array): Uint8Array {
  const stride = width * 4 + 1;
  const raw = new Uint8Array(stride * height);
  // Each row after its filter byte, 0 for none.
  for (let y = 0; y < height; y += 1)
    raw.set(rgba.subarray(y * width * 4, (y + 1) * width * 4), y * stride + 1);
  const header = new Uint8Array(13);
  const view = new DataView(header.buffer);
  view.setUint32(0, width);
  view.setUint32(4, height);
  header[8] = 8; // bit depth
  header[9] = 6; // RGBA
  const parts = [
    new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", header),
    chunk("IDAT", zlibSync(raw, { level: 6 })),
    chunk("IEND", new Uint8Array(0)),
  ];
  const png = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    png.set(part, offset);
    offset += part.length;
  }
  return png;
}
