// Zoom and pan for the drawing (PLAN P10, Decisions: Zoom and pan), without
// the page. Where cells sit is wasm's `Geometry`, made at the zoom; this
// only chooses the zoom and keeps the point under the pointer still.

export const ZOOM_MIN = 0.5;
export const ZOOM_MAX = 4;
/** The narrowest a cell is drawn at the default zoom, in CSS pixels. */
export const CELL_MIN_PX = 16;
/** How much one notch of the wheel, or one key press, zooms. */
export const ZOOM_STEP = 1.25;

export const clampZoom = (zoom) => Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, zoom));

/**
 * The zoom a sheet opens at: a cell `CELL_MIN_PX` wide, so a wide sheet
 * scrolls rather than shrinks. `cell` is a cell's width at zoom 1.
 */
export const defaultZoom = (cell) => clampZoom(CELL_MIN_PX / cell);

/** The zoom that fits a drawing `size` big at zoom 1 into `pane`. */
export const fitZoom = (size, pane) => clampZoom(Math.min(pane.width / size.width, pane.height / size.height));

/**
 * The scroll offsets that keep the drawing's point under `pointer` (in the
 * scroll box's own pixels) still while the zoom goes from `zoom` to `next`.
 */
export function zoomAbout(zoom, next, scroll, pointer) {
  const k = next / zoom;
  return { left: (scroll.left + pointer.x) * k - pointer.x, top: (scroll.top + pointer.y) * k - pointer.y };
}

/**
 * The cell under a client point, through `geometry` made at the drawing's
 * zoom, given the drawn `<svg>`'s client rectangle; null off the drawing.
 */
export function cellUnder(geometry, x, y, box) {
  if (x < box.left || x >= box.right || y < box.top || y >= box.bottom) return null;
  return geometry.cell_at(x - box.left, y - box.top) ?? null;
}
