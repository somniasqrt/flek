// Requested sizes survive window changes; only the displayed sizes are constrained.
export const DEFAULT_SIDEBAR = 268;
export const DEFAULT_DETAIL = 400;
export const MIN_SIDEBAR = 180;
export const MIN_DETAIL = 280;
export const MAX_SIDEBAR = 480;
export const MAX_DETAIL = 760;
export const clamp = (value: number, min: number, max: number) =>
  Math.max(min, Math.min(max, value));

export function panelSizes(width: number, sidebar: number, detail: number, detailOpen: boolean) {
  const centerMin = Math.min(320, Math.max(0, width - MIN_SIDEBAR - (detailOpen ? MIN_DETAIL : 0)));
  const right = detailOpen
    ? clamp(detail, MIN_DETAIL, Math.min(MAX_DETAIL, width - MIN_SIDEBAR - centerMin))
    : 0;
  const left = clamp(sidebar, MIN_SIDEBAR, Math.min(MAX_SIDEBAR, width - centerMin - right));
  return {
    sidebar: left,
    detail: right,
    sidebarMax: Math.max(MIN_SIDEBAR, Math.min(MAX_SIDEBAR, width - centerMin - right)),
    detailMax: Math.max(MIN_DETAIL, Math.min(MAX_DETAIL, width - centerMin - left)),
  };
}

export function readPanelSize(key: string, fallback: number): number {
  try {
    const saved = localStorage.getItem(key);
    const value = saved === null ? NaN : Number(saved);
    return Number.isFinite(value) && value > 0 ? value : fallback;
  } catch {
    return fallback;
  }
}
export function savePanelSize(key: string, value: number) {
  try {
    localStorage.setItem(key, String(value));
  } catch {
    /* optional preference */
  }
}
