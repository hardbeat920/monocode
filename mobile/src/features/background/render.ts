import { Directory, File, Paths } from "expo-file-system";
import { ImageManipulator, SaveFormat } from "expo-image-manipulator";
import { decode } from "jpeg-js";
import { Dimensions } from "react-native";

import { ditherPixels, encodePng, fadeOut } from "./dither";

/**
 * The tallest a background is kept. The dither is drawn per pixel in JS, so
 * this bounds its cost; it is about the screen's height in points times two,
 * enough that its dots stay fine on the phone.
 */
const MAX_HEIGHT = 1600;

/** Where picked photos are kept, cropped and scaled for the screen. */
export const BACKGROUNDS = new Directory(Paths.document, "backgrounds");
/** Their dithered renders, which can be redrawn whenever they go missing. */
const RENDERS = new Directory(Paths.cache, "background-dither");

function ensure(directory: Directory) {
  if (!directory.exists) directory.create({ intermediates: true, idempotent: true });
}

/**
 * Crops an image to the screen's shape, as the empty session covers it, and
 * scales it down to at most MAX_HEIGHT, saved as a JPEG at `destination`.
 */
export async function prepareImage(uri: string, destination: File) {
  const screen = Dimensions.get("screen");
  const aspect = screen.width / screen.height;
  const image = await ImageManipulator.manipulate(uri).renderAsync();
  const width = Math.min(image.width, Math.round(image.height * aspect));
  const height = Math.min(image.height, Math.round(image.width / aspect));
  const context = ImageManipulator.manipulate(image).crop({
    originX: Math.round((image.width - width) / 2),
    originY: Math.round((image.height - height) / 2),
    width,
    height,
  });
  if (height > MAX_HEIGHT) context.resize({ height: MAX_HEIGHT });
  const prepared = await (
    await context.renderAsync()
  ).saveAsync({
    format: SaveFormat.JPEG,
    compress: 0.9,
  });
  ensure(destination.parentDirectory);
  if (destination.exists) destination.delete();
  new File(prepared.uri).move(destination);
}

/** A render's file. Named for what is baked into it, so older renders, faded
 *  into a screen's colour rather than out to clear, are drawn again. */
const renderFile = (key: string) => new File(RENDERS, `${key}.clear.png`);

/**
 * The prepared image at `uri` dithered and faded out to clear toward its
 * foot, as a PNG file URI, for any screen colour. Renders are cached under
 * `key`, which must change whenever the image does.
 */
export async function renderDither(uri: string, key: string) {
  ensure(RENDERS);
  const output = renderFile(key);
  if (output.exists) return output.uri;
  const jpeg = await new File(uri).bytes();
  const { width, height, data } = decode(jpeg, { useTArray: true, formatAsRGBA: true });
  const pixels = fadeOut({ width, height, pixels: ditherPixels({ width, height, pixels: data }) });
  output.create({ overwrite: true });
  output.write(encodePng(width, height, pixels));
  return output.uri;
}

/** Forgets every render of `key`, as its image changed or was removed. */
export function forgetRenders(key: string) {
  if (!RENDERS.exists) return;
  for (const render of RENDERS.list())
    if (render instanceof File && render.name.startsWith(`${key}.`)) render.delete();
}
