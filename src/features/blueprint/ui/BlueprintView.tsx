import "@xyflow/react/dist/style.css";
import "./BlueprintView.css";
import {
  Component,
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  addEdge,
  BaseEdge,
  EdgeLabelRenderer,
  getBezierPath,
  Background,
  BackgroundVariant,
  ControlButton,
  Controls,
  Handle,
  MarkerType,
  Position,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  useEdgesState,
  useConnection,
  useInternalNode,
  useNodeConnections,
  useStore,
  useStoreApi,
  useReactFlow,
  useNodesState,
  type Connection,
  type ConnectionLineComponentProps,
  type EdgeProps,
  type InternalNode,
  type Edge,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  createPath,
  readTextFile,
  writeTextFile,
} from "../../../platform/tauri/fs";
import {
  borderPoint,
  center,
  sideOf,
  spreadBoxes,
  type Rect,
} from "../geometry";
import {
  CursorMagicSelection,
  DashboardSquare,
  Flowchart,
  RedoArrow,
  UndoArrow,
} from "../../../shared/ui/icons";
import { autoLayout } from "../layout";
import { radioArrowStep } from "../../../shared/lib/radioKeys";
import { useRailTooltip } from "../../../app/shell/RailTooltip";
import { MOD, SHIFT } from "../../../platform/tauri/platform";
import { blueprintFromCodePrompt, blueprintPrompt } from "../prompt";
import { createHistory, record, redo, undo, type History } from "../history";
import type { NoteComposerCard } from "../../notes";
import {
  ARROW_COLOR,
  BLUEPRINT_FILE,
  BLUEPRINT_KINDS,
  BLUEPRINT_LINES,
  BLUEPRINT_SHAPES,
  kindColor,
  parseBlueprint,
  serializeBlueprint,
  type Blueprint,
  type BlueprintKind,
  type BlueprintLine,
  type BlueprintShape,
} from "../blueprint";

type BoxData = {
  kind: BlueprintKind;
  shape: BlueprintShape;
  line: BlueprintLine;
  title: string;
  instructions: string;
  color: string;
};
type BoxNode = Node<BoxData, "box">;

/**
 * A blueprint box on the canvas: type, title and instructions, with a
 * hidden drop target on the left and the start dot for new arrows on the
 * right. Its border, shape, line style and selection ring follow its data.
 */
function Box({ data, selected }: NodeProps<BoxNode>) {
  const linked = useNodeConnections({ handleType: "source" }).length > 0;
  const connecting = useConnection((c) => c.inProgress);
  const stadium = data.shape === "stadium";
  const dashed = data.line === "dashed";
  return (
    <div
      className={`min-w-[140px] max-w-[240px] border-[3px] text-[12px] text-content ${
        stadium ? "rounded-full" : "rounded-lg"
      } ${
        // A dashed border sits just outside the fill, so the canvas shows
        // through the gaps instead of the box's own background.
        dashed ? "p-[3px]" : ""
      }`}
      style={{
        borderColor: data.color,
        borderStyle: data.line,
        // Selection must read at a glance on the blue canvas, whatever the
        // box color: a white ring with a soft glow.
        outline: selected ? "3px solid #fff" : undefined,
        outlineOffset: 3,
        boxShadow: selected
          ? "0 0 0 9px rgba(255,255,255,0.25), 0 0 24px 6px rgba(255,255,255,0.35)"
          : undefined,
      }}
    >
      <Handle
        type="target"
        position={Position.Left}
        className="blueprint-target"
        // While an arrow is being dragged the whole box takes the drop; the
        // rest of the time it lets clicks and drags through to the box.
        style={
          connecting
            ? {
                inset: 0,
                width: "100%",
                height: "100%",
                transform: "none",
                borderRadius: 0,
                zIndex: 1,
              }
            : undefined
        }
      />
      <div
        className={`bg-background-base py-2 ${
          stadium
            ? "rounded-full px-6 text-center"
            : `${dashed ? "rounded-[4px]" : "rounded-[6px]"} px-3`
        }`}
      >
        <div className="text-[10px] uppercase tracking-wide text-content/50">
          {data.kind}
        </div>
        <div className="font-medium">{data.title || "Untitled"}</div>
        {data.instructions ? (
          <div className="mt-1 line-clamp-3 whitespace-pre-wrap text-content/60">
            {data.instructions}
          </div>
        ) : null}
      </div>
      <Handle
        type="source"
        position={Position.Right}
        className={`blueprint-dot ${linked ? "blueprint-dot-linked" : ""}`}
        style={{ background: data.color, right: DOT_INSET }}
      />
    </div>
  );
}

const NODE_TYPES = { box: Box };

/** Labelled segmented control for a small set of options (Shape, Line). */
function Choice<T extends string>({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: readonly { id: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <div>
      <div className="mb-1 text-content/60">{label}</div>
      <div
        role="radiogroup"
        aria-label={label}
        className="grid grid-cols-2 gap-0.5 rounded-md border border-content/10 p-0.5"
      >
        {options.map((o) => (
          <button
            key={o.id}
            type="button"
            role="radio"
            aria-checked={value === o.id}
            tabIndex={value === o.id ? 0 : -1}
            onClick={() => onChange(o.id)}
            onKeyDown={(e) => {
              const next = radioArrowStep(
                e.key,
                options.map((x) => x.id),
                o.id,
              );
              if (!next) return;
              e.preventDefault();
              onChange(next);
              const i = options.findIndex((x) => x.id === next);
              (
                e.currentTarget.parentElement?.children[i] as
                  HTMLElement | undefined
              )?.focus();
            }}
            className={`rounded-[5px] px-2 py-1 ${
              value === o.id
                ? "bg-selection text-content"
                : "text-content/50 hover:text-content"
            }`}
          >
            {o.label}
          </button>
        ))}
      </div>
    </div>
  );
}

/*
 * React Flow measures dot positions with getBoundingClientRect, which under
 * CrispZoom's CSS zoom comes back in the wrong units, so arrows drift off the
 * dots. Anchor arrows on the box itself instead: offsetWidth/Height are
 * unaffected by zoom.
 */
/** How far a start dot sits inside the box's border. */
const DOT_INSET = 2;

/** Where the creation dot sits: middle of the right edge, just inside the border. */
function rightMiddle(n: InternalNode) {
  return {
    x: n.internals.positionAbsolute.x + (n.measured.width ?? 0) - DOT_INSET,
    y: n.internals.positionAbsolute.y + (n.measured.height ?? 0) / 2,
  };
}

/** A node's box in canvas coordinates, from React Flow's measured size. */
function rectOf(n: InternalNode): Rect {
  return {
    ...n.internals.positionAbsolute,
    width: n.measured.width ?? 0,
    height: n.measured.height ?? 0,
  };
}

const SIDE_POSITION = {
  top: Position.Top,
  right: Position.Right,
  bottom: Position.Bottom,
  left: Position.Left,
} as const;

/**
 * Each arrow attaches where the line between the two boxes crosses their
 * borders, so its start dot slides along the edge as the boxes move.
 */
function BoxEdge({ source, target, label, markerEnd, style, data }: EdgeProps) {
  const from = useInternalNode<BoxNode>(source);
  const to = useInternalNode<BoxNode>(target);
  if (!from || !to) return null;
  const fromRect = rectOf(from);
  const toRect = rectOf(to);
  const a = borderPoint(fromRect, from.data.shape, center(toRect), DOT_INSET);
  const b = borderPoint(toRect, to.data.shape, center(fromRect));
  const [path, labelX, labelY] = getBezierPath({
    sourceX: a.x,
    sourceY: a.y,
    sourcePosition: SIDE_POSITION[sideOf(fromRect, a)],
    targetX: b.x,
    targetY: b.y,
    targetPosition: SIDE_POSITION[sideOf(toRect, b)],
  });
  return (
    <>
      <BaseEdge
        path={path}
        markerEnd={markerEnd}
        style={{
          ...style,
          stroke: String(data?.color ?? ARROW_COLOR),
          strokeDasharray: data?.line === "dashed" ? "6 4" : undefined,
        }}
      />
      {typeof label === "string" && label.trim() ? (
        // HTML label instead of React Flow's SVG text: SVG text is measured
        // with getBBox, which CrispZoom's CSS zoom throws off, leaving the
        // text off-center. A flex box centers it at any zoom.
        <EdgeLabelRenderer>
          <div
            className="blueprint-edge-label"
            style={{
              transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
            }}
          >
            {label}
          </div>
        </EdgeLabelRenderer>
      ) : null}
      <circle
        cx={a.x}
        cy={a.y}
        r={6}
        fill={from.data.color}
        stroke="#fff"
        strokeWidth={2}
      />
    </>
  );
}

/** Preview line while dragging a new arrow: from the start dot to the mouse, or to the hovered box's border. */
function BoxConnectionLine({
  fromNode,
  toNode,
  pointer,
  connectionLineStyle,
}: ConnectionLineComponentProps<BoxNode>) {
  const [tx, ty, zoom] = useStore((st) => st.transform);
  const a = rightMiddle(fromNode);
  // `pointer` is in screen pixels relative to the pane; bring it into the
  // canvas. Over a box, the preview snaps to its border in the mouse's
  // direction instead of the hidden left drop point.
  const mouse = { x: (pointer.x - tx) / zoom, y: (pointer.y - ty) / zoom };
  const overBox = toNode && toNode.id !== fromNode.id ? toNode : null;
  const end = overBox
    ? borderPoint(rectOf(overBox), overBox.data.shape, mouse)
    : mouse;
  const [path] = getBezierPath({
    sourceX: a.x,
    sourceY: a.y,
    sourcePosition: Position.Right,
    targetX: end.x,
    targetY: end.y,
    targetPosition: overBox
      ? SIDE_POSITION[sideOf(rectOf(overBox), end)]
      : Position.Left,
  });
  return (
    <path
      d={path}
      fill="none"
      stroke="#e8f1ff"
      strokeWidth={1.5}
      style={connectionLineStyle}
    />
  );
}

const EDGE_TYPES = { box: BoxEdge };

/**
 * WebKit bitmap-scales content under `transform: scale()`, so boxes blur when
 * zoomed (bugs.webkit.org/264954). After React Flow writes its viewport
 * transform, swap the scale for CSS `zoom`, which re-lays out and redraws the
 * boxes at the new size. Translate is divided by zoom because `zoom` also
 * scales it.
 */
function CrispZoom() {
  const store = useStoreApi();
  useEffect(() => {
    const apply = () => {
      const { domNode, transform } = store.getState();
      const viewport = domNode?.querySelector<HTMLElement>(
        ".react-flow__viewport",
      );
      if (!viewport) return;
      const [x, y, zoom] = transform;
      viewport.style.transform = `translate(${x / zoom}px, ${y / zoom}px)`;
      viewport.style.zoom = String(zoom);
    };
    apply();
    // Subscribed after React Flow's own viewport listener, so this runs last.
    return store.subscribe(apply);
  }, [store]);
  return null;
}
/** Closed arrowhead in the arrow's own color. */
const arrowHead = (color: string) =>
  ({ type: MarkerType.ArrowClosed, color }) as const;

/** Writes the blueprint file, creating `.monocode/` on the first save. */
async function writeBlueprint(save: {
  path: string;
  cwd: string;
  text: string;
}) {
  try {
    await writeTextFile(save.path, save.text);
  } catch {
    // First save: .monocode/ does not exist yet.
    await createPath(save.cwd, BLUEPRINT_FILE, false).catch(() => {});
    await writeTextFile(save.path, save.text);
  }
}

/** File shape → canvas state. */
function nodesFrom(bp: Blueprint): BoxNode[] {
  return bp.nodes.map(({ id, x, y, ...data }) => ({
    id,
    type: "box",
    position: { x, y },
    data,
  }));
}
/** File arrows → canvas edges, drawn above the boxes so their start dots show. */
function edgesFrom(bp: Blueprint): Edge[] {
  return bp.edges.map(({ line, color, ...e }) => ({
    ...e,
    type: "box",
    zIndex: 1,
    data: { line, color },
    markerEnd: arrowHead(color),
  }));
}

/** The canvas state in the file's shape. */
function toBlueprint(nodes: BoxNode[], edges: Edge[]): Blueprint {
  return {
    version: 1,
    nodes: nodes.map((n) => ({
      id: n.id,
      x: Math.round(n.position.x),
      y: Math.round(n.position.y),
      ...n.data,
    })),
    edges: edges.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      label: typeof e.label === "string" ? e.label : "",
      line: e.data?.line === "dashed" ? "dashed" : "solid",
      color: String(e.data?.color ?? ARROW_COLOR),
    })),
  };
}

type ImplementProps = {
  /** Opens a chat with the brief attached; the parent switches to Development. */
  onImplement?: (card: NoteComposerCard) => void;
};

/**
 * The Blueprint editor: canvas, controls and sidebar. Loads and autosaves
 * the project's blueprint file, keeps undo/redo history, and hands briefs to
 * the chat through `onImplement`.
 */
function BlueprintCanvas({
  cwd,
  onImplement,
}: { cwd: string } & ImplementProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState<BoxNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [ready, setReady] = useState(false);
  // Empty blueprint: offer to start drawing or to draw it from the code.
  const [startedDrawing, setStartedDrawing] = useState(false);
  const controlTips = useRailTooltip("right");
  const [history, setHistory] = useState<History>(() => createHistory(""));
  // Select mode: dragging on empty canvas draws a selection box instead of panning.
  const [selectMode, setSelectMode] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const lastSaved = useRef("");
  // The file the canvas currently holds. Saves only go there, so a project
  // switch can never write one project's drawing into another's file.
  const loadedPath = useRef<string | null>(null);
  // A save waiting for its debounce, flushed if Blueprint closes first.
  const pending = useRef<{ path: string; cwd: string; text: string } | null>(
    null,
  );
  const { fitView } = useReactFlow();

  const boxRects = () =>
    nodes.map((n) => ({
      id: n.id,
      x: n.position.x,
      y: n.position.y,
      width: n.measured?.width ?? 0,
      height: n.measured?.height ?? 0,
    }));
  // Moves boxes to new positions, saved like a drag, then refits the view.
  const placeBoxes = (placed: { id: string; x: number; y: number }[]) => {
    const at = new Map(placed.map((b) => [b.id, b]));
    setNodes((ns) =>
      ns.map((n) => {
        const b = at.get(n.id);
        return b ? { ...n, position: { x: b.x, y: b.y } } : n;
      }),
    );
    requestAnimationFrame(() => void fitView({ duration: 300 }));
  };
  // Spread: keep the drawing, only push apart boxes that overlap or touch.
  const arrange = () => placeBoxes(spreadBoxes(boxRects()));
  // Auto layout: redo the whole drawing in columns that follow the arrows.
  const relayout = () => placeBoxes(autoLayout(boxRects(), edges));
  const path = `${cwd}/${BLUEPRINT_FILE}`;

  useEffect(() => {
    let live = true;
    loadedPath.current = null;
    setReady(false);
    setError(null);
    readTextFile(path)
      .then((text) => {
        // A slower read for the previous project must not land here.
        if (!live) return;
        const bp = parseBlueprint(text);
        lastSaved.current = serializeBlueprint(bp);
        loadedPath.current = path;
        setHistory(createHistory(lastSaved.current));
        setNodes(nodesFrom(bp));
        setEdges(edgesFrom(bp));
        setReady(true);
      })
      .catch((e) => {
        if (!live) return;
        setNodes([]);
        setEdges([]);
        // Same text an untouched empty canvas serializes to, so just opening
        // Blueprint never creates .monocode/blueprint.json.
        lastSaved.current = serializeBlueprint({
          version: 1,
          nodes: [],
          edges: [],
        });
        setHistory(createHistory(lastSaved.current));
        // A missing file starts an empty blueprint; anything else (corrupt,
        // unreadable) stays untouched and is never overwritten.
        if (/no such file|not found|os error 2/i.test(String(e))) {
          loadedPath.current = path;
          setReady(true);
        } else setError(`Cannot load ${BLUEPRINT_FILE}: ${String(e)}`);
      });
    return () => {
      live = false;
    };
  }, [path, setNodes, setEdges]);

  // Undo/redo: a snapshot once the canvas has been still for a moment, so a
  // drag or a burst of typing is one step. Selection and box sizes are not
  // part of the file, so they never create steps.
  useEffect(() => {
    if (!ready) return;
    const text = serializeBlueprint(toBlueprint(nodes, edges));
    const timer = setTimeout(() => setHistory((h) => record(h, text)), 400);
    return () => clearTimeout(timer);
  }, [nodes, edges, ready]);

  const travel = useCallback(
    (step: (h: History) => History) => {
      const next = step(history);
      if (next === history) return;
      // The restored state equals next.present, so the snapshot effect
      // records nothing and redo stays available.
      setHistory(next);
      const bp = parseBlueprint(next.present);
      setNodes(nodesFrom(bp));
      setEdges(edgesFrom(bp));
    },
    [history, setNodes, setEdges],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      // Text fields keep their own undo.
      const t = e.target as HTMLElement | null;
      if (t?.closest("input, textarea, select, [contenteditable]")) return;
      const key = e.key.toLowerCase();
      const redoKey = (key === "z" && e.shiftKey) || (key === "y" && e.ctrlKey);
      if (!redoKey && key !== "z") return;
      e.preventDefault();
      travel(redoKey ? redo : undo);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [travel]);

  useEffect(() => {
    if (!ready || loadedPath.current !== path) return;
    const text = serializeBlueprint(toBlueprint(nodes, edges));
    if (text === lastSaved.current) {
      pending.current = null;
      return;
    }
    const save = { path, cwd, text };
    pending.current = save;
    const timer = setTimeout(async () => {
      try {
        await writeBlueprint(save);
        if (pending.current === save) pending.current = null;
        lastSaved.current = text;
        setError(null);
      } catch (e) {
        setError(`Cannot save blueprint: ${String(e)}`);
      }
    }, 500);
    return () => clearTimeout(timer);
  }, [nodes, edges, ready, path, cwd]);

  // Leaving Blueprint (e.g. Implement) or switching project unmounts or
  // reloads the canvas before the debounce fires: write the last edit now,
  // to the file it belongs to.
  useEffect(
    () => () => {
      const save = pending.current;
      pending.current = null;
      if (save) void writeBlueprint(save).catch(() => {});
    },
    [path],
  );

  const onConnect = useCallback(
    (c: Connection) =>
      setEdges((es) =>
        addEdge(
          {
            ...c,
            id: crypto.randomUUID(),
            type: "box",
            zIndex: 1,
            label: "",
            data: { line: "solid", color: ARROW_COLOR },
            markerEnd: arrowHead(ARROW_COLOR),
          },
          es,
        ),
      ),
    [setEdges],
  );

  const addBox = (kind: BlueprintKind) => {
    const n = nodes.length;
    setNodes((ns) => [
      ...ns.map((x) => ({ ...x, selected: false })),
      {
        id: crypto.randomUUID(),
        type: "box",
        position: { x: 40 + (n % 6) * 40, y: 40 + (n % 6) * 40 },
        selected: true,
        data: {
          kind,
          shape: "rectangle",
          line: "solid",
          title: "",
          instructions: "",
          color: kindColor(kind),
        },
      },
    ]);
  };

  const selectedNodes = nodes.filter((n) => n.selected);
  const selectedIds = new Set(selectedNodes.map((n) => n.id));
  // The box editor only makes sense for a single box.
  const selectedNode =
    selectedNodes.length === 1 ? selectedNodes[0] : undefined;
  const implement = (scope: "selection" | "all") => {
    const bp = toBlueprint(nodes, edges);
    const ids = scope === "selection" ? selectedIds : undefined;
    const brief = blueprintPrompt(bp, ids);
    onImplement?.({
      id: `blueprint-${crypto.randomUUID()}`,
      slug: "blueprint",
      title:
        scope === "selection"
          ? `Blueprint: ${selectedIds.size} selected box${selectedIds.size === 1 ? "" : "es"}`
          : "Blueprint",
      sourceCwd: cwd,
      body: brief,
      brief,
    });
  };
  const drawFromCode = () => {
    const brief = blueprintFromCodePrompt();
    onImplement?.({
      id: `blueprint-${crypto.randomUUID()}`,
      slug: "blueprint",
      title: "Blueprint from code",
      sourceCwd: cwd,
      body: brief,
      brief,
    });
  };
  const showStart = ready && nodes.length === 0 && !startedDrawing;
  const selectedEdge =
    selectedNodes.length > 0 ? undefined : edges.find((e) => e.selected);
  const patchNode = (patch: Partial<BoxData>) =>
    setNodes((ns) =>
      ns.map((n) =>
        n.id === selectedNode?.id ? { ...n, data: { ...n.data, ...patch } } : n,
      ),
    );

  const field =
    "w-full rounded-md border border-content/10 bg-transparent px-2 py-1 text-[12px] text-content outline-none focus:border-content/30";

  return (
    <div className="flex h-full w-full bg-background-base text-content">
      <div className="relative min-w-0 flex-1" data-testid="blueprint-canvas">
        {showStart ? (
          <div className="absolute inset-0 z-10 grid place-items-center p-6">
            <div className="w-full max-w-md rounded-xl border border-white/20 bg-background-base/95 p-5 text-[13px] text-content shadow-2xl">
              <div className="text-[15px] font-semibold">No blueprint yet</div>
              <div className="mt-1 text-content/60">
                Draw this project's structure by hand, or let the agent read the
                code and draw it for you.
              </div>
              <div className="mt-4 grid gap-2">
                <button
                  type="button"
                  onClick={() => setStartedDrawing(true)}
                  className="rounded-lg border border-content/10 px-3 py-2.5 text-left hover:bg-selection"
                >
                  <div className="font-medium">Start drawing</div>
                  <div className="text-content/55">
                    Empty canvas: add boxes from the sidebar.
                  </div>
                </button>
                {onImplement ? (
                  <button
                    type="button"
                    onClick={drawFromCode}
                    className="rounded-lg border border-content/10 px-3 py-2.5 text-left hover:bg-selection"
                  >
                    <div className="font-medium">Draw it from the code</div>
                    <div className="text-content/55">
                      Opens a chat: the agent reads the codebase and writes the
                      blueprint. Come back to Blueprint when it is done.
                    </div>
                  </button>
                ) : null}
              </div>
            </div>
          </div>
        ) : null}
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={NODE_TYPES}
          edgeTypes={EDGE_TYPES}
          connectionLineComponent={BoxConnectionLine}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          isValidConnection={(c) => c.source !== c.target}
          connectionRadius={36}
          connectOnClick={false}
          colorMode="dark"
          style={{ background: "#1a4d9c" }}
          defaultEdgeOptions={{
            style: { stroke: "#e8f1ff", strokeWidth: 1.5 },
          }}
          fitView
          fitViewOptions={{ maxZoom: 1 }}
          proOptions={{ hideAttribution: true }}
          // Cmd/Ctrl/Shift-click adds to the selection; Shift-drag draws a box.
          multiSelectionKeyCode={["Meta", "Control", "Shift"]}
          selectionOnDrag={selectMode}
          panOnDrag={selectMode ? [1, 2] : true}
        >
          {/* Blueprint paper: fine grid with a stronger line every 5 squares. */}
          <Background
            id="minor"
            bgColor="#1a4d9c"
            variant={BackgroundVariant.Lines}
            gap={16}
            color="rgba(255,255,255,0.14)"
          />
          <Background
            id="major"
            bgColor="transparent"
            variant={BackgroundVariant.Lines}
            gap={80}
            color="rgba(255,255,255,0.3)"
          />
          {/* The app's instant tooltip; native titles are slow in the webview. */}
          <div {...controlTips.handlers}>
            <Controls showInteractive={false}>
              <ControlButton
                onClick={() => travel(undo)}
                disabled={history.past.length === 0}
                data-rail-tip="Undo"
                data-rail-tip-detail={`${MOD}Z`}
                aria-label="Undo"
              >
                <UndoArrow className="blueprint-icon size-3" />
              </ControlButton>
              <ControlButton
                onClick={() => travel(redo)}
                disabled={history.future.length === 0}
                data-rail-tip="Redo"
                data-rail-tip-detail={`${SHIFT}${MOD}Z`}
                aria-label="Redo"
              >
                <RedoArrow className="blueprint-icon size-3" />
              </ControlButton>
              <ControlButton
                onClick={() => setSelectMode((on) => !on)}
                data-rail-tip={selectMode ? "Select mode: on" : "Select mode"}
                data-rail-tip-detail="Drag to select boxes"
                aria-label="Select mode"
                aria-pressed={selectMode}
                className={selectMode ? "blueprint-control-on" : undefined}
              >
                <CursorMagicSelection className="blueprint-icon" />
              </ControlButton>
              <ControlButton
                onClick={arrange}
                data-rail-tip="Arrange"
                data-rail-tip-detail="Space out overlapping boxes"
                aria-label="Arrange boxes"
              >
                <DashboardSquare className="blueprint-icon" />
              </ControlButton>
              <ControlButton
                onClick={relayout}
                data-rail-tip="Auto layout"
                data-rail-tip-detail="Redo the layout following the arrows"
                aria-label="Auto layout"
              >
                <Flowchart className="blueprint-icon" />
              </ControlButton>
            </Controls>
          </div>
          {controlTips.tooltip}
          <CrispZoom />
        </ReactFlow>
      </div>
      <aside className="flex w-64 shrink-0 flex-col gap-3 overflow-y-auto border-l border-stroke p-3 text-[12px]">
        {/* MiniMap scales by style.width/height: they must be numbers. */}
        <MiniMap
          pannable
          zoomable
          ariaLabel="Blueprint overview"
          style={{
            position: "static",
            margin: 0,
            width: 232,
            height: 140,
            borderRadius: 6,
            background: "#1a4d9c",
            border: "1px solid rgba(128,128,128,0.3)",
          }}
          maskColor="rgba(0,0,0,0.35)"
          nodeColor={(n) => (n.data as BoxData).color}
        />
        {onImplement ? (
          <div>
            <div className="mb-1.5 text-[11px] uppercase tracking-wide text-content/50">
              Implement
            </div>
            <div className="grid grid-cols-2 gap-1.5">
              <button
                type="button"
                disabled={!ready || selectedIds.size === 0}
                onClick={() => implement("selection")}
                className="rounded-md border border-content/10 px-2 py-1 hover:bg-selection disabled:opacity-40"
              >
                Selection
              </button>
              <button
                type="button"
                disabled={!ready || nodes.length === 0}
                onClick={() => implement("all")}
                className="rounded-md border border-content/10 px-2 py-1 hover:bg-selection disabled:opacity-40"
              >
                All
              </button>
            </div>
            <div className="mt-2 text-content/50">
              Opens a chat with the blueprint. The agent plans first and waits
              for your OK.
            </div>
          </div>
        ) : null}
        <div>
          <div className="mb-1.5 text-[11px] uppercase tracking-wide text-content/50">
            Add box
          </div>
          <div className="grid grid-cols-2 gap-1.5">
            {BLUEPRINT_KINDS.map((k) => (
              <button
                key={k.id}
                type="button"
                disabled={!ready}
                onClick={() => addBox(k.id)}
                className="flex items-center gap-1.5 rounded-md border border-content/10 px-2 py-1 text-left hover:bg-selection disabled:opacity-40"
              >
                <span
                  className="size-2 shrink-0 rounded-full"
                  style={{ background: k.color }}
                />
                {k.label}
              </button>
            ))}
          </div>
          <div className="mt-2 text-content/50">
            Drag from a box's right dot to another box to draw an arrow.
          </div>
        </div>

        {selectedNodes.length > 1 ? (
          <div className="text-content/60">
            {selectedNodes.length} boxes selected
          </div>
        ) : null}

        {selectedNode ? (
          <div className="flex flex-col gap-2">
            <div className="text-[11px] uppercase tracking-wide text-content/50">
              Box
            </div>
            <input
              className={field}
              placeholder="Title"
              value={selectedNode.data.title}
              onChange={(e) => patchNode({ title: e.target.value })}
            />
            <textarea
              className={`${field} min-h-[120px] resize-y`}
              placeholder="Instructions"
              value={selectedNode.data.instructions}
              onChange={(e) => patchNode({ instructions: e.target.value })}
            />
            <select
              className={field}
              value={selectedNode.data.kind}
              onChange={(e) => {
                const kind = e.target.value as BlueprintKind;
                patchNode({ kind, color: kindColor(kind) });
              }}
            >
              {BLUEPRINT_KINDS.map((k) => (
                <option key={k.id} value={k.id}>
                  {k.label}
                </option>
              ))}
            </select>
            <Choice
              label="Shape"
              options={BLUEPRINT_SHAPES}
              value={selectedNode.data.shape}
              onChange={(shape) => patchNode({ shape })}
            />
            <Choice
              label="Line"
              options={BLUEPRINT_LINES}
              value={selectedNode.data.line}
              onChange={(line) => patchNode({ line })}
            />
            <label className="flex items-center gap-2 text-content/60">
              Color
              <input
                type="color"
                value={selectedNode.data.color}
                onChange={(e) => patchNode({ color: e.target.value })}
              />
            </label>
            <button
              type="button"
              className="rounded-md bg-red-600 px-2 py-1 text-white hover:bg-red-500"
              onClick={async () => {
                const name = selectedNode.data.title || "this box";
                const ok = await ask(`Delete ${name} and its arrows?`, {
                  title: "MonoCode",
                  kind: "warning",
                  okLabel: "Delete",
                });
                if (!ok) return;
                setNodes((ns) => ns.filter((n) => n.id !== selectedNode.id));
                setEdges((es) =>
                  es.filter(
                    (e) =>
                      e.source !== selectedNode.id &&
                      e.target !== selectedNode.id,
                  ),
                );
              }}
            >
              Delete box
            </button>
          </div>
        ) : null}

        {selectedEdge ? (
          <div className="flex flex-col gap-2">
            <div className="text-[11px] uppercase tracking-wide text-content/50">
              Arrow
            </div>
            <input
              className={field}
              placeholder="Label (calls, reads from…)"
              value={
                typeof selectedEdge.label === "string" ? selectedEdge.label : ""
              }
              onChange={(e) =>
                setEdges((es) =>
                  es.map((x) =>
                    x.id === selectedEdge.id
                      ? { ...x, label: e.target.value }
                      : x,
                  ),
                )
              }
            />
            <Choice
              label="Line"
              options={BLUEPRINT_LINES}
              value={selectedEdge.data?.line === "dashed" ? "dashed" : "solid"}
              onChange={(line) =>
                setEdges((es) =>
                  es.map((x) =>
                    x.id === selectedEdge.id
                      ? { ...x, data: { ...x.data, line } }
                      : x,
                  ),
                )
              }
            />
            <label className="flex items-center gap-2 text-content/60">
              Color
              <input
                type="color"
                value={String(selectedEdge.data?.color ?? ARROW_COLOR)}
                onChange={(e) => {
                  const color = e.target.value;
                  setEdges((es) =>
                    es.map((x) =>
                      x.id === selectedEdge.id
                        ? {
                            ...x,
                            data: { ...x.data, color },
                            markerEnd: arrowHead(color),
                          }
                        : x,
                    ),
                  );
                }}
              />
            </label>
            <button
              type="button"
              className="rounded-md bg-red-600 px-2 py-1 text-white hover:bg-red-500"
              onClick={async () => {
                const ok = await ask("Delete this arrow?", {
                  title: "MonoCode",
                  kind: "warning",
                  okLabel: "Delete",
                });
                if (!ok) return;
                setEdges((es) => es.filter((x) => x.id !== selectedEdge.id));
              }}
            >
              Delete arrow
            </button>
          </div>
        ) : null}

        {error ? <div className="text-red-400">{error}</div> : null}
      </aside>
    </div>
  );
}

/**
 * Shows the error in place of the canvas: nothing else in the app catches
 * render errors, so one here would blank the whole window.
 */
class BlueprintBoundary extends Component<
  { children: ReactNode },
  { error: Error | null }
> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="h-full w-full overflow-auto bg-background-base p-4 text-[12px] text-red-400">
        <div className="mb-2 font-medium">
          Blueprint crashed: {error.message}
        </div>
        <pre className="whitespace-pre-wrap text-content/60">{error.stack}</pre>
      </div>
    );
  }
}

/** Blueprint mode for a project, with its own React Flow store and error boundary. */
export function BlueprintView({
  cwd,
  onImplement,
}: { cwd: string } & ImplementProps) {
  return (
    <BlueprintBoundary>
      {/* One canvas per project: a fresh viewport (fitted to that drawing),
          selection and undo history, instead of carrying the last project's
          zoom and position over. */}
      <ReactFlowProvider key={cwd}>
        <BlueprintCanvas cwd={cwd} onImplement={onImplement} />
      </ReactFlowProvider>
    </BlueprintBoundary>
  );
}
