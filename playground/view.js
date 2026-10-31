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
 * zoom, given the client rectangle of the sheet the drawing offers (its
 * top-left corner is the drawn `<svg>`'s; see `sheetBox`); null off it.
 * Past the text's last row and widest column the cells are blank sheet, as
 * `Geometry.cell_at` gives them.
 */
export function cellUnder(geometry, x, y, box) {
  if (x < box.left || x >= box.right || y < box.top || y >= box.bottom) return null;
  return geometry.cell_at(x - box.left, y - box.top) ?? null;
}

/** Rows and columns of blank sheet the drawing offers past the text, to draw the next box in. */
export const ROOM_ROWS = 8;
export const ROOM_COLUMNS = 16;

/**
 * The size of the sheet the drawing offers, in CSS pixels from the drawn
 * `<svg>`'s top-left corner: the drawing (`size`, at the zoom) with
 * `ROOM_ROWS` rows and `ROOM_COLUMNS` columns of blank sheet past it, and
 * never less than `pane`, the part of the pane on show. `cell` is one
 * cell's area at the zoom, its strip included. `reach`, when a drag has
 * gone past the text, is the extent out to the farthest cell it reached,
 * and the room is taken past that instead, so the sheet grows ahead of
 * the drag.
 */
export function sheetExtent(size, cell, pane, reach = null) {
  const width = Math.max(size.width, reach?.width ?? 0);
  const height = Math.max(size.height, reach?.height ?? 0);
  return {
    width: Math.max(width + ROOM_COLUMNS * cell.width, pane.width),
    height: Math.max(height + ROOM_ROWS * cell.height, pane.height),
  };
}

/** The band along each edge of the pane on show, in CSS pixels, in which a held drag scrolls the pane. */
export const EDGE_PX = 40;
/** The most a held drag scrolls the pane in one frame, in CSS pixels, with the pointer at or past an edge. */
export const EDGE_STEP_PX = 20;

/**
 * How far a drag held at `pointer` (client pixels) scrolls the pane in one
 * frame, `{ dx, dy }` in CSS pixels: towards the edge of `shown` (the
 * client rectangle of the pane on show) whose band it is in, the faster
 * the nearer the edge, and at the most past it. Zero outside the bands.
 */
export function edgeScrollStep(pointer, shown) {
  const speed = (inside) =>
    inside < EDGE_PX ? Math.ceil(((EDGE_PX - Math.max(0, inside)) / EDGE_PX) * EDGE_STEP_PX) : 0;
  return {
    dx: speed(shown.right - pointer.x) - speed(pointer.x - shown.left),
    dy: speed(shown.bottom - pointer.y) - speed(pointer.y - shown.top),
  };
}

/**
 * What the drawing says while the text can't be drawn (a tab, a character
 * outside the repertoire): over the last drawing it could, kept and faded
 * (review item 27), or alone when it has drawn none yet.
 */
export const undrawable = (kept) =>
  kept ? "Last valid drawing" : "No drawing yet: the text has characters Studio can't draw, which Diagnostics lists.";

/** The client rectangle `cellUnder` takes: from the drawn `<svg>`'s client rectangle out to the sheet's extent. */
export const sheetBox = (svg, extent) => ({
  left: svg.left, top: svg.top, right: svg.left + extent.width, bottom: svg.top + extent.height,
});
