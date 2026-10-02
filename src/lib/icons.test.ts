/**
 * The icons against the files they claim to be, and against the toolbar.
 *
 * `icons.ts` says most of its pictures are Lucide's, and the notices file tells
 * a reader the same. That is only true while each `body` is the vendored file's
 * own markup, so this compares them --- and in the other direction too, because
 * a file in `vendor/icons/` that nothing draws is shipped attribution for
 * nothing and usually means a rename left one behind.
 */

import { describe, expect, it } from "vitest";

import { ICONS, type IconName, type IconPart } from "./icons";
import { TOOL_ACTIONS, TOOL_GROUPS } from "./toolbar";

const vendored = import.meta.glob("../../vendor/icons/*.svg", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

type Shape = { tag: string; attrs: Record<string, string> };

/** The shapes inside a Lucide file, with whatever attributes each carries. */
function shapesOf(file: string): Shape[] {
  const inner = /<svg[^>]*>\n([\s\S]*)\n<\/svg>/.exec(file)?.[1] ?? "";
  return [...inner.matchAll(/<(\w+)((?:\s+[\w-]+="[^"]*")*)\s*\/>/g)].map((shape) => ({
    tag: shape[1]!,
    attrs: Object.fromEntries(
      [...shape[2]!.matchAll(/([\w-]+)="([^"]*)"/g)].map((pair) => [pair[1]!, pair[2]!]),
    ),
  }));
}

/** One of ours in the same form, so the two compare whatever the attribute order. */
function shapeOf(part: IconPart): Shape {
  if ("d" in part) return { tag: "path", attrs: { d: part.d } };
  if ("rect" in part) {
    const [x, y, width, height, rx] = part.rect.map(String);
    return { tag: "rect", attrs: { x: x!, y: y!, width: width!, height: height!, rx: rx! } };
  }
  const [cx, cy, r] = part.circle.map(String);
  return { tag: "circle", attrs: { cx: cx!, cy: cy!, r: r! } };
}

const names = Object.keys(ICONS) as IconName[];
const files = new Map(
  Object.entries(vendored).map(([path, text]) => [path.split("/").pop()!.replace(".svg", ""), text]),
);

describe("icons", () => {
  it("draws every vendored file exactly as it is vendored", () => {
    expect(files.size).toBeGreaterThan(20);
    for (const name of names) {
      const { source, parts } = ICONS[name];
      if (source === null) continue;
      expect(files.has(source), `${name} names ${source}`).toBe(true);
      const file = files.get(source)!;
      // The count against the raw text, so a shape the pattern above cannot
      // read is a failure here and not a shorter list that still matches.
      expect(shapesOf(file).length, name).toBe(file.split("<").length - 3);
      expect((parts as readonly IconPart[]).map(shapeOf), name).toEqual(shapesOf(file));
    }
  });

  it("vendors no file that nothing draws, and two icons are ours", () => {
    const used = new Set(names.map((name) => ICONS[name].source).filter((source) => source !== null));
    expect([...files.keys()].sort()).toEqual([...used].sort());
    expect(names.filter((name) => ICONS[name].source === null).sort()).toEqual(["redact", "width"]);
  });

  it("gives every button on the tool row a picture of its own", () => {
    const row = [
      ...TOOL_ACTIONS,
      ...TOOL_GROUPS.filter((group) => group.id !== "color"),
    ];
    expect(row.length).toBe(10);
    for (const entry of row) expect(entry.icon, entry.id).toBeDefined();
    const drawn = row.map((entry) => entry.icon);
    expect(new Set(drawn).size).toBe(drawn.length);
    for (const icon of drawn) expect(names).toContain(icon);
  });
});
