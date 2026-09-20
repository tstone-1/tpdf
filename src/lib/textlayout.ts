import type { TextLayout, TextRun } from "./textedit";

/**
 * Physical dimensions along the text axes, independent of zoom and font matrices.
 *
 * The width is the run's own advance, kerning and word gaps included, not the
 * width of its glyphs set again: that is the space the run occupies on the page,
 * and the writer keeps the source's own positioning for text left unchanged
 * (`layout::source_items`), so the unchanged run fits it exactly. For a kerned
 * run the glyph widths are wider, and a box that wide would claim room the line
 * never had, up to its neighbour. The size is rounded up to the thousandth the
 * control shows; the writer reads a size within that step as the source's own.
 *
 * `grow` is set, because this is the box the editor opens rather than one a
 * reader chose: the writer is free to follow the typed text past this width, as
 * far as the room after the run allows (`layout::free_width`). The width stays
 * the run's own advance all the same, and is what the box falls back to -- a
 * grown box is never narrower than the one the run already occupies, and the
 * reader still sees this number in the width control until they change it.
 */
export function defaultTextLayout(run: TextRun): TextLayout {
  const x = Math.hypot(run.matrix[0], run.matrix[1]);
  const y = Math.hypot(run.matrix[2], run.matrix[3]);
  const round = (value: number) => Math.ceil(value * 1000) / 1000;
  const sourceSize = run.size * y, size = round(sourceSize);
  const height = Math.max(sourceSize * 1.25, run.minimum_height ?? 0) * size / sourceSize;
  return { width: Math.max(0.1, round(run.advance * x)), height: Math.max(0.1, round(height)),
    size, wrap: false, font: "auto", grow: true };
}

/** Native form controls also provide keyboard access to resizing and wrapping. */
export class TextLayoutControls {
  /**
   * Whether a reader has typed a width of their own.
   *
   * It is the one thing that separates "this is the box the editor opened" from
   * "this is the box I want", and the two want opposite treatment: the first
   * follows the typed text up to the room on the line, the second is the
   * reader's and is left alone. It is set from the width control's own input
   * event -- not from any control change, because choosing a font or ticking
   * wrap says nothing about the width -- and reset by `set`, which takes the
   * answer from the layout it is given rather than assuming one.
   */
  private sized = false;
  readonly root = document.createElement("div");
  readonly width = document.createElement("input");
  readonly height = document.createElement("input");
  readonly size = document.createElement("input");
  readonly font = document.createElement("select");
  readonly wrap = document.createElement("input");

  constructor(change: () => void) {
    this.root.style.cssText = "display:flex;flex-wrap:wrap;gap:8px;margin:8px 0";
    for (const [title, input, min, max] of [
      ["Width (pt)", this.width, 0.1, 14400], ["Height (pt)", this.height, 0.1, 14400],
      ["Font size (pt)", this.size, 1, 512],
    ] as const) {
      const label = document.createElement("label"); label.textContent = title;
      input.type = "number"; input.min = String(min); input.max = String(max); input.step = "0.1";
      input.style.cssText = "display:block;width:105px"; input.setAttribute("aria-label", title);
      label.append(input); this.root.append(label);
      // One listener, so `sized` is already true when `change` reads the layout.
      input.addEventListener("input", () => { if (input === this.width) this.sized = true; change(); });
    }
    const label = document.createElement("label"); label.textContent = "Font";
    this.font.setAttribute("aria-label", "Font"); this.font.style.cssText = "display:block;max-width:260px";
    for (const [value, title] of [["auto", "Original with automatic fallback"], ["original", "Original only"],
      ["noto_sans", "Noto Sans"], ["noto_sans_bold", "Noto Sans Bold"],
      ["noto_sans_italic", "Noto Sans Italic"], ["noto_sans_bold_italic", "Noto Sans Bold Italic"],
      ["noto_sans_cjk_sc", "Noto Sans CJK SC"], ["noto_sans_cjk_sc_bold", "Noto Sans CJK SC Bold"]]) {
      const option = document.createElement("option"); option.value = value!; option.textContent = title!;
      this.font.append(option);
    }
    label.append(this.font); this.root.append(label); this.font.addEventListener("change", change);
    const wrapping = document.createElement("label"); wrapping.textContent = "Wrap within box";
    this.wrap.type = "checkbox"; this.wrap.setAttribute("aria-label", "Wrap within box");
    wrapping.append(this.wrap); this.root.append(wrapping); this.wrap.addEventListener("change", change);
  }
  read(): TextLayout {
    return { width: Number(this.width.value), height: Number(this.height.value), size: Number(this.size.value),
      font: this.font.value as TextLayout["font"], wrap: this.wrap.checked, grow: !this.sized };
  }
  set(value: TextLayout): void {
    this.width.value = String(value.width); this.height.value = String(value.height); this.size.value = String(value.size);
    this.font.value = value.font; this.wrap.checked = value.wrap; this.sized = !value.grow;
  }
  error(): string | null {
    const { width, height, size } = this.read();
    return ![width, height, size].every(Number.isFinite) || width < 0.1 || width > 14400 || height < 0.1 || height > 14400 || size < 1 || size > 512
      ? "Use a font size from 1 to 512 pt and a box from 0.1 to 14400 pt." : null;
  }
}
