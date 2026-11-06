import type { DocumentState, LayerKind, LayerState } from "../api/types";

export const LAYER_KIND_LABEL: Record<LayerKind, string> = {
  base: "原始照片",
  portrait: "人像调整",
  color: "调色调整",
  aiResult: "AI 结果",
  retouch: "局部修复",
  blur: "背景虚化",
};

export function baseLayerOf(layers: LayerState[]): LayerState | null {
  return layers.find((layer) => layer.kind === "base") ?? null;
}

export function editableLayers(layers: LayerState[]): LayerState[] {
  return layers.filter((layer) => layer.kind !== "base");
}

/** UI lists layers top-first: reverse the backend's bottom-to-top order. */
export function layersTopFirst(layers: LayerState[]): LayerState[] {
  return [...editableLayers(layers)].reverse();
}

export function topLayerOfKind(layers: LayerState[], kind: LayerKind): LayerState | null {
  for (let index = layers.length - 1; index >= 0; index -= 1) {
    if (layers[index].kind === kind) return layers[index];
  }
  return null;
}

export function selectedOrKind(
  document: DocumentState | null,
  selectedLayerId: string | null,
  kind: LayerKind,
): LayerState | null {
  if (!document) return null;
  const selected = document.layers.find((layer) => layer.id === selectedLayerId);
  if (selected && selected.kind === kind) return selected;
  return topLayerOfKind(document.layers, kind);
}

export function layerById(document: DocumentState | null, layerId: string | null): LayerState | null {
  if (!document || !layerId) return null;
  return document.layers.find((layer) => layer.id === layerId) ?? null;
}

export function defaultSelection(document: DocumentState, preferredTool: "portrait" | "color" | "photo" | "ai"): string | null {
  if (preferredTool === "portrait") {
    const portrait = topLayerOfKind(document.layers, "portrait");
    if (portrait) return portrait.id;
  }
  if (preferredTool === "color") {
    const colorLayer = topLayerOfKind(document.layers, "color");
    if (colorLayer) return colorLayer.id;
  }
  const base = baseLayerOf(document.layers);
  return base?.id ?? document.layers[0]?.id ?? null;
}

export function canMoveLayer(document: DocumentState, layer: LayerState, direction: "up" | "down"): boolean {
  if (layer.kind === "base" || layer.locked) return false;
  const editable = editableLayers(document.layers);
  const index = editable.findIndex((item) => item.id === layer.id);
  if (index < 0) return false;
  return direction === "up" ? index < editable.length - 1 : index > 0;
}
