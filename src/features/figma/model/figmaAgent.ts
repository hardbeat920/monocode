import type {
  FigmaBridgeStatus,
  FigmaConnection,
  FigmaGeneration,
  FigmaPreviewWorkspace,
  FigmaSource,
} from "./figma";

export type FigmaCapture = {
  generation: FigmaGeneration;
  preview: FigmaPreviewWorkspace;
};

function requireBridge(status: FigmaBridgeStatus) {
  if (!status.enabled)
    throw new Error(
      "The Figma bridge is off. Turn it on in Settings → Connections → Figma.",
    );
}

function figmaAgentLayer(source: FigmaSource) {
  return {
    nodeId: source.nodeId,
    name: source.name,
    type: source.type,
    width: source.width,
    height: source.height,
  };
}

export function figmaAgentFiles(status: FigmaBridgeStatus) {
  requireBridge(status);
  return {
    files: status.connections.map(({ id, selection }) => ({
      connectionId: id,
      file: selection?.document.name ?? null,
      page: selection?.document.pageName ?? null,
      selectionCount: selection?.selectionCount ?? 0,
      selected: selection?.source ? figmaAgentLayer(selection.source) : null,
    })),
  };
}

export function figmaAgentConnection(
  status: FigmaBridgeStatus,
  connectionId: string | undefined,
): FigmaConnection {
  requireBridge(status);
  if (connectionId) {
    const connection = status.connections.find(
      (entry) => entry.id === connectionId,
    );
    if (!connection)
      throw new Error(
        "That Figma file is not connected; run figma.selection for connected files",
      );
    return connection;
  }
  const [only, ...others] = status.connections;
  if (!only)
    throw new Error(
      "No Figma file is connected. Run MonoCode Figma Bridge in Figma Desktop.",
    );
  if (others.length > 0)
    throw new Error(
      "Several Figma files are connected; pass connectionId from figma.selection",
    );
  return only;
}

export function figmaAgentNodeId(value: string): string {
  return value.replace(/-/g, ":");
}

export function figmaAgentCapture({ generation, preview }: FigmaCapture) {
  const design = `${preview.relativeDirectory}/design`;
  return {
    file: generation.document.name,
    page: generation.document.pageName,
    layer: figmaAgentLayer(generation.source),
    directory: design,
    files: {
      bundle: `${design}/source-bundle.json`,
      assets: `${design}/assets/manifest.json`,
      preview: `${design}/preview.png`,
    },
    diagnostics: generation.diagnostics,
  };
}
