// Layout widths are proportional to the window, with pixel floors and ceilings.
// The user's choice is stored as a fraction of the frame width; only the
// displayed pixel sizes are constrained, so a small window never loses it.
export const DEFAULT_SIDEBAR = 0.2;
export const DEFAULT_DETAIL = 0.3;
export const MIN_SIDEBAR = 200;
export const MAX_SIDEBAR = 420;
export const MIN_DETAIL = 300;
export const MAX_DETAIL = 640;
/// Narrowest timeline we keep while panels are being sized.
export const MIN_CENTER = 360;
/// Below this frame width the detail panel overlays the timeline.
export const OVERLAY_WIDTH = 920;

export const clamp = (value: number, min: number, max: number) =>
  Math.max(min, Math.min(max, value));

export function panelSizes(
  width: number,
  sidebar: number,
  detail: number,
  detailOpen: boolean,
) {
  const overlay = detailOpen && width < OVERLAY_WIDTH;
  // An overlaying panel does not take room from the timeline.
  const docked = detailOpen && !overlay;
  const centerMin = Math.min(
    MIN_CENTER,
    Math.max(0, width - MIN_SIDEBAR - (docked ? MIN_DETAIL : 0)),
  );
  const detailCeil = overlay
    ? Math.max(MIN_DETAIL, Math.min(MAX_DETAIL, width - MIN_SIDEBAR))
    : Math.min(MAX_DETAIL, width - MIN_SIDEBAR - centerMin);
  const right = detailOpen
    ? clamp(detail * width, MIN_DETAIL, Math.max(MIN_DETAIL, detailCeil))
    : 0;
  const taken = docked ? right : 0;
  const sidebarCeil = Math.max(
    MIN_SIDEBAR,
    Math.min(MAX_SIDEBAR, width - centerMin - taken),
  );
  const left = clamp(sidebar * width, MIN_SIDEBAR, sidebarCeil);
  return {
    sidebar: left,
    detail: right,
    overlay,
    sidebarMax: sidebarCeil,
    detailMax: overlay
      ? detailCeil
      : Math.max(MIN_DETAIL, Math.min(MAX_DETAIL, width - centerMin - left)),
  };
}

/// Saved values above this are pixels from before sizes became fractions.
const FRACTION_LIMIT = 1;

/// Reads a saved size as a fraction of `width`. Old pixel values (> 1) are
/// converted once; the caller saves the result back.
export function readPanelSize(
  key: string,
  fallback: number,
  width: number,
): number {
  try {
    const saved = localStorage.getItem(key);
    const value = saved === null ? NaN : Number(saved);
    if (!Number.isFinite(value) || value <= 0) return fallback;
    return migratePanelSize(value, width);
  } catch {
    return fallback;
  }
}

export function migratePanelSize(value: number, width: number): number {
  const fraction = value > FRACTION_LIMIT ? value / Math.max(width, 1) : value;
  return clamp(fraction, 0.1, 0.6);
}

export function savePanelSize(key: string, value: number) {
  try {
    localStorage.setItem(key, String(value));
  } catch {
    /* optional preference */
  }
}
