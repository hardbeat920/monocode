import { View } from "react-native";

import { MASCOT_GRID, projectMascot } from "./projectMascots";
import { useFrameToggle } from "./useFrameToggle";

type Run = { x: number; y: number; w: number };

/** Filled runs per row, so each run is one View instead of one per cell. */
function runs(rows: readonly string[]): Run[] {
  const out: Run[] = [];
  rows.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      if (row[x] !== "#") {
        x += 1;
        continue;
      }
      let w = 1;
      while (row[x + w] === "#") w += 1;
      out.push({ x, y, w });
      x += w;
    }
  });
  return out;
}

/** Flat 8×8 project mascot, the native take on the desktop ProjectMascot SVG. */
export function ProjectMascot({
  project,
  color,
  name,
  size = 18,
  active = false,
}: {
  project: string;
  color: string;
  /** Explicit pick; falls back to the hashed one like desktop. */
  name?: string | null;
  size?: number;
  /** Cycles the two frames while a turn is in flight. */
  active?: boolean;
}) {
  const mascot = projectMascot(project, name);
  const talking = useFrameToggle(active);
  const cell = size / MASCOT_GRID;

  return (
    <View style={{ width: size, height: size }} accessibilityElementsHidden>
      {runs(talking ? mascot.talk : mascot.rest).map(({ x, y, w }) => (
        <View
          key={`${x}-${y}`}
          style={{
            position: "absolute",
            left: x * cell,
            top: y * cell,
            width: w * cell,
            height: cell,
            backgroundColor: color,
          }}
        />
      ))}
    </View>
  );
}
