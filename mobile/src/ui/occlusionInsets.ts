export type OcclusionMeasurement = {
  supported: boolean;
  width: number;
  height: number;
  regions: { x: number; y: number; width: number; height: number }[];
};

/** Frames include UIKit's recommended clearance margins, in control coordinates. */
export function occlusionInsets(measurement: OcclusionMeasurement) {
  const { width, height, regions } = measurement;
  let left = 0;
  let right = 0;
  for (const region of regions) {
    const maxX = region.x + region.width;
    const maxY = region.y + region.height;
    if (maxY <= 0 || region.y >= height || maxX <= 0 || region.x >= width) continue;
    // Keep the larger usable portion if a camera overlaps the middle of a bar.
    if (maxX <= width - region.x) left = Math.max(left, maxX);
    else right = Math.max(right, width - region.x);
  }
  return { left: Math.min(width, left), right: Math.min(width, right) };
}
