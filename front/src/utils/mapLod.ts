export type SelectionKind = 'node' | 'pipe' | null;

export const LARGE_NETWORK_NODE_COUNT = 80;
export const MEDIUM_NETWORK_NODE_COUNT = 30;

export const LARGE_NETWORK_LABEL_MAX_HEIGHT = 150_000;
export const MEDIUM_NETWORK_LABEL_MAX_HEIGHT = 400_000;
export const SMALL_NETWORK_LABEL_MAX_HEIGHT = 2_000_000;

export type LabelLodContext = {
  nodeId: string;
  selectedKind: SelectionKind;
  selectedNodeId: string | null;
};

export function labelLodVisible(
  cameraHeight: number,
  networkSize: number,
  context: LabelLodContext,
): boolean {
  const isSelected =
    context.selectedKind === 'node' && context.selectedNodeId === context.nodeId;
  if (isSelected) return true;

  if (networkSize > LARGE_NETWORK_NODE_COUNT) {
    return cameraHeight < LARGE_NETWORK_LABEL_MAX_HEIGHT;
  }
  if (networkSize > MEDIUM_NETWORK_NODE_COUNT) {
    return cameraHeight < MEDIUM_NETWORK_LABEL_MAX_HEIGHT;
  }
  return cameraHeight < SMALL_NETWORK_LABEL_MAX_HEIGHT;
}

export function nodeStride(cameraHeight: number, networkSize: number): number {
  let stride =
    cameraHeight > 8_000_000 ? 8 : cameraHeight > 4_000_000 ? 4 : cameraHeight > 2_000_000 ? 2 : 1;

  if (networkSize > LARGE_NETWORK_NODE_COUNT) {
    if (cameraHeight > 8_000_000) stride = Math.max(stride, 16);
    else if (cameraHeight > 4_000_000) stride = Math.max(stride, 8);
    else if (cameraHeight > 2_000_000) stride = Math.max(stride, 4);
    else if (cameraHeight > 1_000_000) stride = Math.max(stride, 2);
  }

  return stride;
}

export function nodePointPixelSize(
  basePixelSize: number,
  cameraHeight: number,
  networkSize: number,
): number {
  if (networkSize <= LARGE_NETWORK_NODE_COUNT) return basePixelSize;

  if (cameraHeight > 8_000_000) return Math.max(2, Math.round(basePixelSize * 0.4));
  if (cameraHeight > 4_000_000) return Math.max(3, Math.round(basePixelSize * 0.55));
  if (cameraHeight > 2_000_000) return Math.max(4, Math.round(basePixelSize * 0.7));
  return basePixelSize;
}

export function nodePointVisible(
  index: number,
  stride: number,
  context: {
    nodeId: string;
    selectedKind: SelectionKind;
    selectedNodeId: string | null;
    isContingency: boolean;
  },
): boolean {
  if (context.selectedKind === 'node' && context.selectedNodeId === context.nodeId) {
    return true;
  }
  if (context.isContingency) return true;
  return index % stride === 0;
}
