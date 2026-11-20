/**
 * Split-screen layout: a binary tree whose leaves are terminal tabs. Pure
 * functions only; the terminal store owns the trees and the view draws them.
 */

/** `row` puts the two halves side by side, `column` stacks them. */
export type SplitDirection = "row" | "column";
export type SplitSide = "left" | "right" | "top" | "bottom";

export type SplitNode =
  | { kind: "pane"; tabId: string }
  | {
      kind: "split";
      id: string;
      direction: SplitDirection;
      /** Share of the first child, 0–1. */
      ratio: number;
      first: SplitNode;
      second: SplitNode;
    };

/** Fractions (0–1) of the terminal area. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface PaneBox {
  tabId: string;
  rect: Rect;
}

export interface DividerBox {
  splitId: string;
  direction: SplitDirection;
  ratio: number;
  /** Area of the whole split the divider cuts, to turn a pointer into a ratio. */
  rect: Rect;
}

export const MIN_SPLIT_RATIO = 0.1;
export const MAX_SPLIT_RATIO = 0.9;

export function clampRatio(ratio: number): number {
  if (!Number.isFinite(ratio)) return 0.5;
  return Math.min(MAX_SPLIT_RATIO, Math.max(MIN_SPLIT_RATIO, ratio));
}

export function paneIds(node: SplitNode): string[] {
  return node.kind === "pane" ? [node.tabId] : [...paneIds(node.first), ...paneIds(node.second)];
}

export function containsPane(node: SplitNode, tabId: string): boolean {
  return node.kind === "pane" ? node.tabId === tabId : containsPane(node.first, tabId) || containsPane(node.second, tabId);
}

/** Replace the pane of `targetTabId` with a split holding it and `tabId` on `side`. */
export function insertPane(node: SplitNode, targetTabId: string, tabId: string, side: SplitSide): SplitNode {
  if (node.kind === "pane") {
    if (node.tabId !== targetTabId) return node;
    const added: SplitNode = { kind: "pane", tabId };
    const before = side === "left" || side === "top";
    return {
      kind: "split",
      id: crypto.randomUUID(),
      direction: side === "left" || side === "right" ? "row" : "column",
      ratio: 0.5,
      first: before ? added : node,
      second: before ? node : added,
    };
  }
  const first = insertPane(node.first, targetTabId, tabId, side);
  const second = first === node.first ? insertPane(node.second, targetTabId, tabId, side) : node.second;
  return first === node.first && second === node.second ? node : { ...node, first, second };
}

/** Drop a pane; its sibling takes the parent's place. Null when nothing is left. */
export function removePane(node: SplitNode, tabId: string): SplitNode | null {
  if (node.kind === "pane") return node.tabId === tabId ? null : node;
  const first = removePane(node.first, tabId);
  const second = removePane(node.second, tabId);
  if (!first) return second;
  if (!second) return first;
  return first === node.first && second === node.second ? node : { ...node, first, second };
}

export function setSplitRatio(node: SplitNode, splitId: string, ratio: number): SplitNode {
  if (node.kind === "pane") return node;
  if (node.id === splitId) return { ...node, ratio: clampRatio(ratio) };
  const first = setSplitRatio(node.first, splitId, ratio);
  const second = setSplitRatio(node.second, splitId, ratio);
  return first === node.first && second === node.second ? node : { ...node, first, second };
}

/** Where every pane and divider sits, as fractions of the terminal area. */
export function layoutBoxes(node: SplitNode): { panes: PaneBox[]; dividers: DividerBox[] } {
  const panes: PaneBox[] = [];
  const dividers: DividerBox[] = [];
  const walk = (n: SplitNode, rect: Rect) => {
    if (n.kind === "pane") {
      panes.push({ tabId: n.tabId, rect });
      return;
    }
    dividers.push({ splitId: n.id, direction: n.direction, ratio: n.ratio, rect });
    if (n.direction === "row") {
      const w = rect.w * n.ratio;
      walk(n.first, { ...rect, w });
      walk(n.second, { ...rect, x: rect.x + w, w: rect.w - w });
    } else {
      const h = rect.h * n.ratio;
      walk(n.first, { ...rect, h });
      walk(n.second, { ...rect, y: rect.y + h, h: rect.h - h });
    }
  };
  walk(node, { x: 0, y: 0, w: 1, h: 1 });
  return { panes, dividers };
}

/**
 * Drop zone for a point inside a pane (fractions of that pane): an edge
 * quarter splits toward that edge, the middle is `center`.
 */
export function dropZone(x: number, y: number): SplitSide | "center" {
  const edges: Array<[SplitSide, number]> = [
    ["left", x],
    ["right", 1 - x],
    ["top", y],
    ["bottom", 1 - y],
  ];
  const [side, distance] = edges.reduce((a, b) => (b[1] < a[1] ? b : a));
  return distance < 0.25 ? side : "center";
}

/** Pane containing a point (fractions of the terminal area). */
export function paneAt(panes: readonly PaneBox[], x: number, y: number): PaneBox | undefined {
  return panes.find((p) => x >= p.rect.x && x <= p.rect.x + p.rect.w && y >= p.rect.y && y <= p.rect.y + p.rect.h);
}

/** Part of a pane a split drop toward `side` would fill: a half, or all of it for `center`. */
export function zoneRect(rect: Rect, side: SplitSide | "center"): Rect {
  switch (side) {
    case "left":
      return { ...rect, w: rect.w / 2 };
    case "right":
      return { ...rect, x: rect.x + rect.w / 2, w: rect.w / 2 };
    case "top":
      return { ...rect, h: rect.h / 2 };
    case "bottom":
      return { ...rect, y: rect.y + rect.h / 2, h: rect.h / 2 };
    default:
      return rect;
  }
}

/** A layout fraction as a CSS percentage. */
export function cssPercent(value: number): string {
  return `${(value * 100).toFixed(4)}%`;
}
