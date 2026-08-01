/**
 * Typography shared by the canvas-rendered surfaces (Graph via Sigma, Garden
 * via Konva).
 *
 * Canvas text does not inherit CSS, so every canvas surface has to restate the
 * font and colour that DOM panes get for free. Restating them *independently*
 * is how they drift: the Garden shipped with Konva's default Arial and a
 * hardcoded `#cbd5e1`, which is a light slate — nearly invisible against the
 * light theme's `#fcfaf5` background, and a different typeface from every other
 * pane. Keeping the values here means the two canvases cannot disagree.
 */

/**
 * Matches the stack the rest of the app resolves to. Declared explicitly rather
 * than read from computed style because Konva and Sigma both want a concrete
 * font string, and a computed value can arrive empty before fonts settle.
 */
export const CANVAS_LABEL_FONT =
  "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif";

/** Label size in canvas pixels, matching the Graph view. */
export const CANVAS_LABEL_SIZE = 12;

/** Secondary text (counts, kind tags) sits one step down. */
export const CANVAS_SUBLABEL_SIZE = 10;
