// Pixel treatments ported from the desktop background worker.
import { ditherPixels, fadeOut, type Source as ImageSource } from "./dither";
import type { BackgroundEffect } from "./settings";
type Source = ImageSource & { luma: Uint8Array };
function coverIndex(source: Source, x: number, y: number) {
  return (
    Math.max(0, Math.min(source.height - 1, Math.floor(y))) * source.width +
    Math.max(0, Math.min(source.width - 1, Math.floor(x)))
  );
}
function sampleCover(source: Source, x: number, y: number) {
  return source.luma[coverIndex(source, x, y)];
}
function asciiPixels(source: Source, light: boolean) {
  const glyphs = [
    [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 4, 0],
    [0, 4, 0, 0, 4, 0, 0],
    [0, 0, 0, 14, 0, 0, 0],
    [0, 0, 14, 0, 14, 0, 0],
    [0, 4, 4, 31, 4, 4, 0],
    [0, 21, 14, 31, 14, 21, 0],
    [10, 10, 31, 10, 31, 10, 10],
    [17, 2, 4, 4, 8, 16, 17],
    [14, 17, 23, 21, 23, 16, 14],
  ];
  const output = new Uint8ClampedArray(source.pixels.length);
  const paper = light ? 255 : 0;
  for (let y = 0; y < source.height; y += 1) {
    for (let x = 0; x < source.width; x += 1) {
      const sx = Math.min(Math.floor(x / 6) * 6 + 3, source.width - 1);
      const sy = Math.min(Math.floor(y / 8) * 8 + 4, source.height - 1);
      const sample = coverIndex(source, sx, sy);
      const inkDensity = light
        ? 255 - source.luma[sample]
        : source.luma[sample];
      const glyph = glyphs[Math.floor(Math.sqrt(inkDensity / 255) * 9)];
      const ink =
        x % 6 < 5 && y % 8 < 7 && (glyph[y % 8] & (1 << (4 - (x % 6)))) !== 0;
      const pixel = (y * source.width + x) * 4;
      const sampled = sample * 4;
      for (let color = 0; color < 3; color += 1) {
        const texture = ink ? source.pixels[sampled + color] : paper;
        output[pixel + color] =
          source.pixels[pixel + color] * 0.6 + texture * 0.4;
      }
      output[pixel + 3] = source.pixels[pixel + 3];
    }
  }
  return output;
}

function halftonePixels(source: Source, light: boolean) {
  const output = new Uint8ClampedArray(source.pixels.length);
  const paper = light ? 255 : 0;
  output.fill(paper);
  for (let alpha = 3; alpha < output.length; alpha += 4) output[alpha] = 255;
  for (let y = 0; y < source.height; y += 4) {
    for (let x = 0; x < source.width; x += 4) {
      const sampledLuma = sampleCover(source, x, y);
      const luma = light ? 255 - sampledLuma : sampledLuma;
      const radius = 2 * (0.3 + 0.7 * Math.sqrt(luma / 255));
      const sx = Math.min(x + 2, source.width - 1);
      const sy = Math.min(y + 2, source.height - 1);
      const dot = coverIndex(source, sx, sy) * 4;
      for (let dy = 0; dy < Math.min(4, source.height - y); dy += 1) {
        for (let dx = 0; dx < Math.min(4, source.width - x); dx += 1) {
          const distance = Math.hypot(dx - 1.5, dy - 1.5);
          const sourcePixel = ((y + dy) * source.width + x + dx) * 4;
          const coverage =
            Math.max(0, Math.min(1, radius + 0.5 - distance)) *
            (source.pixels[dot + 3] / 255);
          for (let color = 0; color < 3; color += 1) {
            const texture =
              source.pixels[dot + color] * coverage + paper * (1 - coverage);
            output[sourcePixel + color] =
              source.pixels[sourcePixel + color] * 0.6 + texture * 0.4;
          }
          output[sourcePixel + 3] = source.pixels[sourcePixel + 3];
        }
      }
    }
  }
  return output;
}

function scanlinePixels(source: Source, light: boolean) {
  const output = new Uint8ClampedArray(source.pixels.length);
  for (let y = 0; y < source.height; y += 1) {
    const gain = y % 3 === 0 ? 0.52 : 1;
    for (let x = 0; x < source.width; x += 1) {
      const pixel = (y * source.width + x) * 4;
      for (let color = 0; color < 3; color += 1) {
        const value = source.pixels[pixel + color];
        output[pixel + color] = light
          ? value + (255 - value) * (1 - gain)
          : value * gain;
      }
      output[pixel + 3] = source.pixels[pixel + 3];
    }
  }
  return output;
}

// Separable sliding-window blur: linear cost, baked once, no per-frame phone work.
function blur(source: ImageSource, radius: number) {
  let pixels = new Uint8Array(source.pixels);
  const { width, height } = source;
  for (const vertical of [false, true]) {
    const output = new Uint8Array(pixels.length);
    const length = vertical ? height : width;
    const lines = vertical ? width : height;
    for (let line = 0; line < lines; line++) {
      const at = (p: number) =>
        (vertical
          ? Math.max(0, Math.min(length - 1, p)) * width + line
          : line * width + Math.max(0, Math.min(length - 1, p))) * 4;
      const sums = [0, 0, 0, 0];
      for (let p = -radius; p <= radius; p++)
        for (let c = 0; c < 4; c++) sums[c] += pixels[at(p) + c];
      for (let p = 0; p < length; p++) {
        for (let c = 0; c < 4; c++) {
          output[at(p) + c] = Math.round(sums[c] / (radius * 2 + 1));
          sums[c] +=
            pixels[at(p + radius + 1) + c] - pixels[at(p - radius) + c];
        }
      }
    }
    pixels = output;
  }
  return pixels;
}
export function effectPixels(
  image: ImageSource,
  effect: BackgroundEffect,
  light: boolean,
  empty: boolean,
) {
  const luma = new Uint8Array(image.width * image.height);
  for (let i = 0; i < luma.length; i++)
    luma[i] = Math.round(
      image.pixels[i * 4] * 0.2126 +
        image.pixels[i * 4 + 1] * 0.7152 +
        image.pixels[i * 4 + 2] * 0.0722,
    );
  const source = { ...image, luma };
  const pixels =
    effect === "dither"
      ? ditherPixels(source)
      : effect === "ascii"
        ? asciiPixels(source, light)
        : effect === "halftone"
          ? halftonePixels(source, light)
          : effect === "scanlines"
            ? scanlinePixels(source, light)
            : new Uint8ClampedArray(source.pixels);
  if (effect === "gradient-blur") {
    const soft = blur(source, empty ? 7 : 5);
    const strong = blur(source, empty ? 18 : 12);
    for (let y = 0; y < source.height; y++) {
      const t = y / source.height;
      const softMix = Math.max(0, Math.min(1, (t - 0.18) / 0.42));
      const strongMix = Math.max(0, Math.min(1, (t - 0.28) / 0.48));
      for (let x = 0; x < source.width; x++)
        for (let c = 0; c < 3; c++) {
          const i = (y * source.width + x) * 4 + c;
          pixels[i] =
            (pixels[i] * (1 - softMix) + soft[i] * softMix) * (1 - strongMix) +
            strong[i] * strongMix;
        }
    }
    const faded = empty
      ? fadeOut({ ...image, pixels: new Uint8Array(pixels) })
      : new Uint8Array(pixels);
    for (let y = 0; y < image.height; y++) {
      const t = (y + 0.5) / image.height;
      const mask = Math.max(0, Math.min(1, 1 - (t - 0.18) / 0.58));
      const overlay = Math.max(0, Math.min(0.7, ((t - 0.15) / 0.61) * 0.7));
      const alpha =
        (empty ? 1 : mask * (1 - overlay)) * (y % 3 === 0 ? 0.64 : 1);
      for (let x = 0; x < image.width; x++)
        faded[(y * image.width + x) * 4 + 3] *= alpha;
    }
    return faded;
  }
  // Desktop uses a shorter, quieter fade in conversations with messages.
  const stops = empty
    ? [
        [0.19, 1],
        [0.3, 0.79],
        [0.42, 0.5],
        [0.54, 0.2],
        [0.64, 0.05],
        [0.72, 0],
      ]
    : [
        [0.18, 1],
        [0.32, 0.61],
        [0.47, 0.32],
        [0.62, 0.11],
        [0.72, 0],
      ];
  for (let y = 0; y < image.height; y++) {
    const t = (y + 0.5) / image.height;
    let alpha = stops[stops.length - 1][1];
    for (let s = 0; s < stops.length; s++)
      if (t <= stops[s][0]) {
        const [end, b] = stops[s];
        const [start, a] = s ? stops[s - 1] : [0, 1];
        alpha = a + (b - a) * Math.max(0, (t - start) / (end - start));
        break;
      }
    for (let x = 0; x < image.width; x++)
      pixels[(y * image.width + x) * 4 + 3] *= alpha;
  }
  return new Uint8Array(pixels);
}
