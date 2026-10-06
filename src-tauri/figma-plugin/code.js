const PREVIEW_MAX_EDGE = 1600;
const PREVIEW_MAX_BYTES = 8 * 1024 * 1024;
const BUNDLE_PREVIEW_MAX_BYTES = 16 * 1024 * 1024;
const SINGLE_ASSET_LIMIT_BYTES = 32 * 1024 * 1024;
const ASSET_CAPTURE_LIMIT_BYTES = 64 * 1024 * 1024;
const ASSET_RELAY_BUDGET_BYTES = 8 * 1024 * 1024;
const MAX_IMAGE_REFS = 50;
const MAX_NODES = 5000;
const MAX_DEPTH = 48;
const SVG_SKIPPED_FOR_IMAGES = "SVG export skipped because the layer has image fills";
const SUPPORTED_ROOT_TYPES = ["FRAME", "GROUP", "COMPONENT", "COMPONENT_SET", "INSTANCE", "SECTION"];
const GEOMETRY_TYPES = ["VECTOR", "REGULAR_POLYGON", "POLYGON", "STAR", "LINE", "BOOLEAN_OPERATION", "ELLIPSE"];

figma.showUI(__html__, { width: 320, height: 360, themeColors: true });

figma.ui.onmessage = async (message) => {
  if (!message || typeof message !== "object") return;
  if (message.type !== "execute-command") return;
  try {
    const result = await handleCommand(message.command, message.params);
    figma.ui.postMessage({ type: "command-result", id: message.id, result });
  } catch (error) {
    figma.ui.postMessage({ type: "command-error", id: message.id, error: errorMessage(error) });
  }
};

figma.on("selectionchange", publishSelection);
figma.on("currentpagechange", publishSelection);

function publishSelection() {
  figma.ui.postMessage({ type: "selection-changed", selection: currentSelection() });
}

async function handleCommand(command, params) {
  switch (command) {
    case "get_selection":
      return currentSelection();
    case "export_selection_preview":
      return exportSelectionPreview(params);
    case "get_codegen_bundle":
      return codegenBundle(params);
    default:
      throw new Error(`Unsupported command: ${command}`);
  }
}

function documentInfo(page = figma.currentPage) {
  return {
    id: figma.root.id,
    name: figma.root.name,
    fileKey: typeof figma.fileKey === "string" ? figma.fileKey : null,
    pageId: page.id,
    pageName: page.name,
  };
}

function pageOf(node) {
  let current = node.parent;
  while (current && current.type !== "PAGE") current = current.parent;
  return current || figma.currentPage;
}

function nodeSize(node) {
  const width = "width" in node && typeof node.width === "number" ? node.width : null;
  const height = "height" in node && typeof node.height === "number" ? node.height : null;
  return width && height ? { width, height } : null;
}

function currentSelection() {
  const selection = figma.currentPage.selection;
  const selected = selection.length === 1 ? selection[0] : null;
  const size = selected ? nodeSize(selected) : null;
  return {
    selectionCount: selection.length,
    document: documentInfo(),
    source: selected && size
      ? { nodeId: selected.id, name: selected.name, type: selected.type, width: size.width, height: size.height }
      : null,
  };
}

async function exportSelectionPreview(params) {
  const nodeId = params && typeof params.nodeId === "string" ? params.nodeId : "";
  if (!nodeId) throw new Error("Missing nodeId parameter");
  const node = await figma.getNodeByIdAsync(nodeId);
  if (!node || !("exportAsync" in node)) throw new Error(`Node cannot be exported: ${nodeId}`);
  const size = nodeSize(node);
  if (!size) throw new Error(`Node has no visible size: ${nodeId}`);
  const scale = Math.max(0.01, Math.min(2, PREVIEW_MAX_EDGE / Math.max(size.width, size.height)));
  const bytes = await node.exportAsync({ format: "PNG", constraint: { type: "SCALE", value: scale } });
  if (bytes.byteLength === 0 || bytes.byteLength > PREVIEW_MAX_BYTES) {
    throw new Error(`The preview for ${node.name} exceeds the 8 MB limit`);
  }
  return { nodeId: node.id, mimeType: "image/png", bytes: bytes.byteLength, imageData: figma.base64Encode(bytes) };
}

async function codegenTarget(params) {
  const nodeId = params && typeof params.nodeId === "string" ? params.nodeId : "";
  if (nodeId) {
    const node = await figma.getNodeByIdAsync(nodeId);
    if (!node || node.type === "DOCUMENT" || node.type === "PAGE") {
      throw new Error(`No layer ${nodeId} in this Figma file`);
    }
    return node;
  }
  const selection = figma.currentPage.selection;
  if (selection.length !== 1) {
    throw new Error(`Select exactly one layer to generate a component (selected: ${selection.length})`);
  }
  return selection[0];
}

async function codegenBundle(params) {
  const sourceNode = await codegenTarget(params);
  if (!("exportAsync" in sourceNode)) throw new Error(`The selected layer cannot be exported: ${sourceNode.name}`);
  const response = await sourceNode.exportAsync({ format: "JSON_REST_V1" });
  await enrichRootSvg(response.document, sourceNode);
  await enrichGeometryExports(response.document, sourceNode);
  const document = normalizeNode(response.document);
  const rootBox = document.absoluteBoundingBox;
  if (!rootBox || typeof rootBox.width !== "number" || typeof rootBox.height !== "number") {
    throw new Error(`The selected layer has no bounds: ${sourceNode.name}`);
  }
  const diagnostics = collectDiagnostics(document, sourceNode.type);
  const imageRefs = [...collectImageRefs(document)];
  if (imageRefs.length > MAX_IMAGE_REFS) {
    throw new Error(`The selection uses ${imageRefs.length} images; the limit is ${MAX_IMAGE_REFS}`);
  }
  const assets = [];
  let assetBytes = 0;
  for (const hash of imageRefs) {
    const asset = await imageAsset(hash);
    assetBytes += asset.bytes;
    if (assetBytes > ASSET_CAPTURE_LIMIT_BYTES) throw new Error("The selection images exceed the 64 MB capture limit");
    assets.push(asset);
  }
  const previewBytes = await sourceNode.exportAsync({ format: "PNG" });
  if (previewBytes.byteLength === 0 || previewBytes.byteLength > BUNDLE_PREVIEW_MAX_BYTES) {
    throw new Error(`The preview for ${sourceNode.name} exceeds the 16 MB limit`);
  }
  return {
    schemaVersion: 1,
    captureId: `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`,
    capturedAt: Date.now(),
    document: documentInfo(pageOf(sourceNode)),
    source: {
      nodeId: sourceNode.id,
      name: sourceNode.name,
      type: sourceNode.type,
      width: rootBox.width,
      height: rootBox.height,
    },
    root: codegenNode(document, null, "", 0),
    preview: { mimeType: "image/png", bytes: previewBytes.byteLength, imageData: figma.base64Encode(previewBytes) },
    assets,
    properties: document.componentPropertyDefinitions || {},
    transport: {
      assetBytes,
      assetBudgetBytes: ASSET_RELAY_BUDGET_BYTES,
      assetOptimizationRequired: assetBytes > ASSET_RELAY_BUDGET_BYTES,
    },
    diagnostics,
  };
}

async function imageAsset(hash) {
  const image = figma.getImageByHash(hash);
  if (!image) throw new Error(`Image not found: ${hash}`);
  const bytes = await image.getBytesAsync();
  if (bytes.byteLength === 0 || bytes.byteLength > SINGLE_ASSET_LIMIT_BYTES) {
    throw new Error(`Image ${hash} exceeds the 32 MB limit`);
  }
  return { hash, mimeType: imageMimeType(bytes), bytes: bytes.byteLength, imageData: figma.base64Encode(bytes) };
}

function imageMimeType(bytes) {
  if (bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e && bytes[3] === 0x47) return "image/png";
  if (bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) return "image/jpeg";
  if (bytes[0] === 0x47 && bytes[1] === 0x49 && bytes[2] === 0x46) return "image/gif";
  if (bytes[0] === 0x52 && bytes[1] === 0x49 && bytes[2] === 0x46 && bytes[8] === 0x57 && bytes[9] === 0x45 && bytes[10] === 0x42 && bytes[11] === 0x50) {
    return "image/webp";
  }
  return "application/octet-stream";
}

function visiblePaints(node) {
  return [...(node.fills || []), ...(node.background || []), ...(node.strokes || [])].filter(
    (paint) => paint && paint.visible !== false,
  );
}

function collectImageRefs(node, refs = new Set()) {
  if (node.visible === false) return refs;
  for (const paint of visiblePaints(node)) {
    if (paint.type === "IMAGE" && typeof paint.imageRef === "string") refs.add(paint.imageRef);
  }
  for (const child of node.children || []) collectImageRefs(child, refs);
  return refs;
}

function collectDiagnostics(document, rootType) {
  const diagnostics = [];
  if (!SUPPORTED_ROOT_TYPES.includes(rootType)) diagnostics.push(`root layer type ${rootType} is unusual for a component`);
  if (document.componentPropertyDefinitionError) {
    diagnostics.push(`component properties could not be read: ${document.componentPropertyDefinitionError}`);
  }
  let count = 0;
  const walk = (node) => {
    count += 1;
    if (node.isMask) diagnostics.push(`mask: ${node.id}`);
    if (typeof node.rotation === "number" && Math.abs(node.rotation) > 0.001) diagnostics.push(`rotation: ${node.id}`);
    if (node.effects && node.effects.some((effect) => effect && effect.visible !== false)) diagnostics.push(`effects: ${node.id}`);
    if (node.svgError && !node.svg && GEOMETRY_TYPES.includes(node.type)) diagnostics.push(`SVG unavailable for ${node.id}: ${node.svgError}`);
    for (const child of node.children || []) walk(child);
  };
  walk(document);
  if (count > MAX_NODES) diagnostics.push(`${count} layers exceed the ${MAX_NODES} layer budget`);
  return [...new Set(diagnostics)];
}

function codegenNode(node, parentBox, parentPath, depth) {
  if (depth > MAX_DEPTH) {
    throw new Error(`The selection nests layers more than ${MAX_DEPTH} levels deep. Select a layer inside it instead.`);
  }
  const box = node.absoluteBoundingBox;
  if (!box || typeof box.x !== "number" || typeof box.y !== "number" || typeof box.width !== "number" || typeof box.height !== "number") {
    throw new Error(`Layer has invalid bounds: ${String(node.id)}`);
  }
  const nodePath = parentPath ? `${parentPath} > ${node.name}` : node.name;
  const imageRefs = [
    ...new Set(
      [...(node.fills || []), ...(node.background || [])]
        .filter((paint) => paint && paint.visible !== false && paint.type === "IMAGE" && typeof paint.imageRef === "string")
        .map((paint) => paint.imageRef),
    ),
  ];
  return {
    id: node.id,
    name: node.name,
    type: node.type,
    nodePath,
    componentId: node.componentId,
    componentProperties: node.componentProperties,
    visible: node.visible !== false,
    x: parentBox ? box.x - parentBox.x : 0,
    y: parentBox ? box.y - parentBox.y : 0,
    width: box.width,
    height: box.height,
    opacity: node.opacity,
    blendMode: node.blendMode,
    layoutMode: node.layoutMode,
    layoutWrap: node.layoutWrap,
    primaryAxisAlignItems: node.primaryAxisAlignItems,
    counterAxisAlignItems: node.counterAxisAlignItems,
    layoutSizingHorizontal: node.layoutSizingHorizontal,
    layoutSizingVertical: node.layoutSizingVertical,
    itemSpacing: node.itemSpacing,
    counterAxisSpacing: node.counterAxisSpacing,
    paddingTop: node.paddingTop,
    paddingRight: node.paddingRight,
    paddingBottom: node.paddingBottom,
    paddingLeft: node.paddingLeft,
    constraints: node.constraints,
    fills: node.fills || [],
    strokes: node.strokes || [],
    strokeWeight: node.strokeWeight,
    strokeAlign: node.strokeAlign,
    cornerRadius: node.cornerRadius,
    rectangleCornerRadii: node.rectangleCornerRadii,
    effects: node.effects || [],
    clipsContent: node.clipsContent,
    characters: node.characters,
    style: node.style,
    boundVariables: node.boundVariables,
    svg: node.svg,
    svgError: node.svgError,
    imageRefs,
    children: (node.children || []).map((child) => codegenNode(child, box, nodePath, depth + 1)),
  };
}

function normalizeNode(node) {
  if (!node || typeof node !== "object") throw new Error("Figma returned an invalid layer");
  const normalized = {};
  for (const [key, value] of Object.entries(node)) {
    if (key !== "children") normalized[key] = normalizeValue(value, key);
  }
  if (Array.isArray(node.children)) normalized.children = node.children.map(normalizeNode);
  return normalized;
}

function normalizeValue(value, key) {
  if (key === "color" && isColor(value)) return hexColor(value);
  if (Array.isArray(value)) return value.map((entry) => normalizeValue(entry, key));
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([entryKey, entryValue]) => [entryKey, normalizeValue(entryValue, entryKey)]));
  }
  return value;
}

function isColor(value) {
  return Boolean(value) && typeof value === "object" && ["r", "g", "b"].every((key) => typeof value[key] === "number");
}

function hexColor(color) {
  const channel = (value) => Math.round(value * 255).toString(16).padStart(2, "0");
  const alpha = typeof color.a === "number" ? Math.round(color.a * 255) : 255;
  return `#${channel(color.r)}${channel(color.g)}${channel(color.b)}${alpha === 255 ? "" : alpha.toString(16).padStart(2, "0")}`;
}

async function enrichRootSvg(documentNode, sourceNode) {
  if (documentNode.type !== "COMPONENT" && documentNode.type !== "INSTANCE") return;
  if (collectImageRefs(documentNode).size > 0) {
    documentNode.svgError = SVG_SKIPPED_FOR_IMAGES;
    return;
  }
  try {
    documentNode.svg = await sourceNode.exportAsync({ format: "SVG_STRING" });
  } catch (error) {
    documentNode.svgError = errorMessage(error);
  }
}

async function enrichGeometryExports(documentNode, sourceNode) {
  if (GEOMETRY_TYPES.includes(documentNode.type) && sourceNode && typeof sourceNode.exportAsync === "function") {
    if (collectImageRefs(documentNode).size > 0) {
      documentNode.svgError = SVG_SKIPPED_FOR_IMAGES;
    } else {
      try {
        documentNode.svg = await sourceNode.exportAsync({ format: "SVG_STRING" });
      } catch (error) {
        documentNode.svgError = errorMessage(error);
      }
    }
  }
  if (!documentNode.children || !sourceNode || !("children" in sourceNode)) return;
  for (const child of documentNode.children) {
    const sourceChild = sourceNode.children.find((candidate) => candidate.id === child.id);
    if (!sourceChild) {
      child.svgError = `Source layer mismatch: ${child.id}`;
      continue;
    }
    await enrichGeometryExports(child, sourceChild);
  }
}

function errorMessage(error) {
  if (error instanceof Error && error.message) return error.message;
  if (error && typeof error.message === "string" && error.message) return error.message;
  if (typeof error === "string" && error) return error;
  return "The Figma command failed";
}
