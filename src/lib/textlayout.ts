import type { TextLayout, TextRun } from "./textedit";

/** Physical dimensions along the text axes, independent of zoom and font matrices. */
export function defaultTextLayout(run: TextRun): TextLayout {
  const x = Math.hypot(run.matrix[0], run.matrix[1]);
  const y = Math.hypot(run.matrix[2], run.matrix[3]);
  const round = (value: number) => Math.ceil(value * 1000) / 1000;
  return { width: Math.max(0.1, round(run.advance * x)), height: Math.max(0.1, round(run.size * y * 1.25)),
    size: round(run.size * y), wrap: false, font: "auto" };
}

/** Native form controls also provide keyboard access to resizing and wrapping. */
export class TextLayoutControls {
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
      label.append(input); this.root.append(label); input.addEventListener("input", change);
    }
    const label = document.createElement("label"); label.textContent = "Font";
    this.font.setAttribute("aria-label", "Font"); this.font.style.cssText = "display:block;max-width:260px";
    for (const [value, title] of [["auto", "Original with automatic fallback"], ["original", "Original only"],
      ["noto_sans", "Noto Sans"], ["noto_sans_bold", "Noto Sans Bold"],
      ["noto_sans_italic", "Noto Sans Italic"], ["noto_sans_bold_italic", "Noto Sans Bold Italic"]]) {
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
      font: this.font.value as TextLayout["font"], wrap: this.wrap.checked };
  }
  set(value: TextLayout): void {
    this.width.value = String(value.width); this.height.value = String(value.height); this.size.value = String(value.size);
    this.font.value = value.font; this.wrap.checked = value.wrap;
  }
  error(): string | null {
    const { width, height, size } = this.read();
    return ![width, height, size].every(Number.isFinite) || width < 0.1 || width > 14400 || height < 0.1 || height > 14400 || size < 1 || size > 512
      ? "Use a font size from 1 to 512 pt and a box from 0.1 to 14400 pt." : null;
  }
}
