/**
 * The pictures on the toolbar, the header and the sidebar tabs.
 *
 * ## Where they come from
 *
 * All but two are Lucide's (ISC), copied from `vendor/icons/` rather than taken
 * from the npm package: twenty-odd paths do not need a dependency, and a vendored
 * file with a digest in `vendor/icons/manifest.json` is something
 * `scripts/third_party_notices.py` can enumerate. `icons.test.ts` holds each
 * icon here against the file its `source` names, in both directions, so an icon
 * cannot be edited in place and still be called Lucide's, and a vendored file
 * nothing draws is a finding.
 *
 * The two with `source: null` are drawn here. Lucide has no picture for a
 * redaction and none for a line weight.
 *
 * ## Shapes as data, because the frontend parses no markup
 *
 * An icon is the obvious use for a string of SVG handed to the web view to
 * parse, and that is the one thing this frontend never does:
 * `scripts/check_webview_sinks.py` refuses every markup sink, so that a
 * document's text can only ever be data. An exemption for strings that happen to
 * be constants would be fifteen of them, each a place the next edit could put a
 * variable. So an icon is a list of three shapes, and {@link iconElement} builds
 * it with element and attribute names that are literals in the source.
 *
 * ## A picture is never the only name
 *
 * Every button that shows an icon without a word carries an `aria-label` and a
 * `title`; the tool row keeps the word beside the picture. The SVG itself is
 * `aria-hidden`, so a screen reader hears the label once and not a path.
 */

/** A stroked path; `weight` replaces the icon's stroke width for this one. */
interface PathPart {
  readonly d: string;
  readonly weight?: number;
}

/** `[x, y, width, height, rx]`; `filled` paints it in the text colour. */
interface RectPart {
  readonly rect: readonly [number, number, number, number, number];
  readonly filled?: boolean;
}

/** `[cx, cy, r]`. */
interface CirclePart {
  readonly circle: readonly [number, number, number];
}

export type IconPart = PathPart | RectPart | CirclePart;

interface Icon {
  /** The file in `vendor/icons/` without its extension, or `null` for our own. */
  readonly source: string | null;
  /** The shapes, on a 24 by 24 grid, in drawing order. */
  readonly parts: readonly IconPart[];
}

export const ICONS = {
  open: {
    source: "folder-open",
    parts: [
      { d: "m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2" },
    ],
  },
  sidebar: {
    source: "panel-left",
    parts: [
      { rect: [3, 3, 18, 18, 2] },
      { d: "M9 3v18" },
    ],
  },
  save: {
    source: "save",
    parts: [
      { d: "M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" },
      { d: "M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7" },
      { d: "M7 3v4a1 1 0 0 0 1 1h7" },
    ],
  },
  print: {
    source: "printer",
    parts: [
      { d: "M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2" },
      { d: "M6 9V3a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v6" },
      { rect: [6, 14, 12, 8, 1] },
    ],
  },
  find: {
    source: "search",
    parts: [
      { d: "m21 21-4.34-4.34" },
      { circle: [11, 11, 8] },
    ],
  },
  undo: {
    source: "undo-2",
    parts: [
      { d: "M9 14 4 9l5-5" },
      { d: "M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11" },
    ],
  },
  redo: {
    source: "redo-2",
    parts: [
      { d: "m15 14 5-5-5-5" },
      { d: "M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13" },
    ],
  },
  previous: {
    source: "chevron-left",
    parts: [
      { d: "m15 18-6-6 6-6" },
    ],
  },
  next: {
    source: "chevron-right",
    parts: [
      { d: "m9 18 6-6-6-6" },
    ],
  },
  close: {
    source: "x",
    parts: [
      { d: "M18 6 6 18" },
      { d: "m6 6 12 12" },
    ],
  },
  add: {
    source: "plus",
    parts: [
      { d: "M5 12h14" },
      { d: "M12 5v14" },
    ],
  },
  select: {
    source: "mouse-pointer",
    parts: [
      { d: "M12.586 12.586 19 19" },
      { d: "M3.688 3.037a.497.497 0 0 0-.651.651l6.5 15.999a.501.501 0 0 0 .947-.062l1.569-6.083a2 2 0 0 1 1.448-1.479l6.124-1.579a.5.5 0 0 0 .063-.947z" },
    ],
  },
  highlight: {
    source: "highlighter",
    parts: [
      { d: "m9 11-6 6v3h9l3-3" },
      { d: "m22 12-4.6 4.6a2 2 0 0 1-2.8 0l-5.2-5.2a2 2 0 0 1 0-2.8L14 4" },
    ],
  },
  comment: {
    source: "message-square",
    parts: [
      { d: "M22 17a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 21.286V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2z" },
    ],
  },
  textBox: {
    source: "type",
    parts: [
      { d: "M12 4v16" },
      { d: "M4 7V5a1 1 0 0 1 1-1h14a1 1 0 0 1 1 1v2" },
      { d: "M9 20h6" },
    ],
  },
  editText: {
    source: "text-cursor-input",
    parts: [
      { d: "M12 20h-1a2 2 0 0 1-2-2 2 2 0 0 1-2 2H6" },
      { d: "M13 8h7a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2h-7" },
      { d: "M5 16H4a2 2 0 0 1-2-2v-4a2 2 0 0 1 2-2h1" },
      { d: "M6 4h1a2 2 0 0 1 2 2 2 2 0 0 1 2-2h1" },
      { d: "M9 6v12" },
    ],
  },
  signature: {
    source: "signature",
    parts: [
      { d: "m21 17-2.156-1.868A.5.5 0 0 0 18 15.5v.5a1 1 0 0 1-1 1h-2a1 1 0 0 1-1-1c0-2.545-3.991-3.97-8.5-4a1 1 0 0 0 0 5c4.153 0 4.745-11.295 5.708-13.5a2.5 2.5 0 1 1 3.31 3.284" },
      { d: "M3 21h18" },
    ],
  },
  draw: {
    source: "pencil",
    parts: [
      { d: "M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" },
      { d: "m15 5 4 4" },
    ],
  },
  pages: {
    source: "files",
    parts: [
      { d: "M15 2h-4a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V8" },
      { d: "M16.706 2.706A2.4 2.4 0 0 0 15 2v5a1 1 0 0 0 1 1h5a2.4 2.4 0 0 0-.706-1.706z" },
      { d: "M5 7a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h8a2 2 0 0 0 1.732-1" },
    ],
  },
  document: {
    source: "file-text",
    parts: [
      { d: "M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z" },
      { d: "M14 2v5a1 1 0 0 0 1 1h5" },
      { d: "M10 9H8" },
      { d: "M16 13H8" },
      { d: "M16 17H8" },
    ],
  },
  outline: {
    source: "list-tree",
    parts: [
      { d: "M8 5h13" },
      { d: "M13 12h8" },
      { d: "M13 19h8" },
      { d: "M3 10a2 2 0 0 0 2 2h3" },
      { d: "M3 5v12a2 2 0 0 0 2 2h3" },
    ],
  },
  thumbnails: {
    source: "layout-grid",
    parts: [
      { rect: [3, 3, 7, 7, 1] },
      { rect: [14, 3, 7, 7, 1] },
      { rect: [14, 14, 7, 7, 1] },
      { rect: [3, 14, 7, 7, 1] },
    ],
  },
  results: {
    source: "text-search",
    parts: [
      { d: "M21 5H3" },
      { d: "M10 12H3" },
      { d: "M10 19H3" },
      { circle: [17, 15, 3] },
      { d: "m21 19-1.9-1.9" },
    ],
  },
  // Two lines of text with a filled bar where the third was.
  redact: {
    source: null,
    parts: [
      { d: "M4 5h16" },
      { rect: [4, 9.5, 12, 5, 1], filled: true },
      { d: "M4 19h10" },
    ],
  },
  // Three rules, thin to thick.
  width: {
    source: null,
    parts: [
      { d: "M4 5h16", weight: 1 },
      { d: "M4 11h16", weight: 2.5 },
      { d: "M4 18h16", weight: 4 },
    ],
  },
} as const satisfies Record<string, Icon>;

export type IconName = keyof typeof ICONS;

const SVG = "http://www.w3.org/2000/svg";

/** One icon as an element, 16 px square and stroked in its text's colour. */
export function iconElement(name: IconName): SVGSVGElement {
  return iconFrom(ICONS[name].parts);
}

/**
 * The same element from a list of shapes, for a picture that is not in
 * {@link ICONS}: the arrange bar keeps its ten beside the commands they stand
 * for, and they are drawn exactly as the toolbar's are.
 */
export function iconFrom(parts: readonly IconPart[]): SVGSVGElement {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("class", "tpdf-icon");
  svg.setAttribute("width", "16");
  svg.setAttribute("height", "16");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "2");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("focusable", "false");
  svg.style.cssText = "flex:none;display:block;";
  for (const part of parts) {
    if ("d" in part) {
      const path = document.createElementNS(SVG, "path");
      path.setAttribute("d", part.d);
      if (part.weight !== undefined) path.setAttribute("stroke-width", String(part.weight));
      svg.appendChild(path);
    } else if ("rect" in part) {
      const [x, y, width, height, rx] = part.rect;
      const rect = document.createElementNS(SVG, "rect");
      rect.setAttribute("x", String(x));
      rect.setAttribute("y", String(y));
      rect.setAttribute("width", String(width));
      rect.setAttribute("height", String(height));
      rect.setAttribute("rx", String(rx));
      if (part.filled) rect.setAttribute("fill", "currentColor");
      svg.appendChild(rect);
    } else {
      const [cx, cy, r] = part.circle;
      const circle = document.createElementNS(SVG, "circle");
      circle.setAttribute("cx", String(cx));
      circle.setAttribute("cy", String(cy));
      circle.setAttribute("r", String(r));
      svg.appendChild(circle);
    }
  }
  return svg;
}

/**
 * Svelte's `use:` form of {@link iconElement}: `<span use:icon={"save"}></span>`.
 *
 * The span is the icon's own, with nothing else in it, so the framework never
 * has a child of its own beside the one put here.
 */
export function icon(node: Element, name: IconName): void {
  node.replaceChildren(iconElement(name));
}
