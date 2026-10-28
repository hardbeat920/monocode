const state = visual.state({ shape: 0, angle: 0.4, height: 1.5, guides: true });

return () => {
  const geometry = ["box", "sphere", "cylinder"][state.shape];
  const objects = [
    visual.object({ kind: geometry }, {
      position: [0, 0.25, 0], rotation: [0, state.angle, 0],
      scale: [1.8, state.height, 1.8], color: "accent",
    }),
    visual.object({ kind: "cone" }, {
      position: [2.2, 0, 0], scale: [1, 1.8, 1], color: "series2",
    }),
    visual.object({ kind: "sphere" }, {
      position: [-2.2, 0, 0], scale: [1.3, 1.3, 1.3], color: "series3",
    }),
  ];
  if (state.guides) {
    for (let offset = -3; offset <= 3; offset += 1) {
      objects.push(visual.object({ kind: "line", points: [[offset, -1, -3], [offset, -1, 3]], width: 1 }, { color: "faint" }));
      objects.push(visual.object({ kind: "line", points: [[-3, -1, offset], [3, -1, offset]], width: 1 }, { color: "faint" }));
    }
  }
  return visual.column([
    visual.text("Geometry explorer", { size: "title" }),
    visual.text("Drag to orbit. Hold Command or Control while scrolling to zoom.", { color: "muted" }),
    visual.tabs(["Box", "Sphere", "Cylinder"], state.shape, value => { state.shape = value; }),
    visual.scene(objects, { width: 720, height: 360, camera: { eye: [6, 4, 8], target: [0, 0, 0], fov: 45 } }),
    visual.slider("Rotation", state.angle, 0, 6.2, value => { state.angle = value; }, { step: 0.1 }),
    visual.slider("Height", state.height, 0.5, 3, value => { state.height = value; }, { step: 0.1 }),
    visual.toggle("Show reference grid", state.guides, value => { state.guides = value; }),
    visual.text("A 2D projection", { size: "subtitle" }),
    visual.canvas([
      visual.line(35, 145, 600, 145, { color: "faint" }),
      visual.rect(100, 145 - state.height * 35, 90, state.height * 35, { color: "accent" }),
      visual.circle(340, 110, 35, { color: "series3" }),
      visual.polyline([[460, 145], [505, 55], [550, 145]], { width: 2, color: "series2" }),
      visual.label(100, 175, "Height responds to the slider", { color: "muted" }),
    ], { width: 640, height: 200 }),
  ], { gap: 14 });
};
