// PDF comment styles use UTF-16 offsets, matching DOM text offsets.
import { catalog } from "./i18n";

export type CommentStyle = [start: number, end: number, flags: number];
const strike = 8, subscript = 16, superscript = 32;
const basicCommands = ["bold", "italic", "underline"] as const;
const commands = [...basicCommands, "strikeThrough", "subscript", "superscript"] as const;

export type CommentBody = string | { text: string; styles: CommentStyle[] };

export function commentBody(text: string, styles?: CommentStyle[]): CommentBody {
  return styles?.length ? { text, styles } : text;
}

const text = catalog({
  "en-US": { bold: "Bold", italic: "Italic", underline: "Underline", strikeThrough: "Strikethrough", subscript: "Subscript", superscript: "Superscript", more: "More formatting", buttons: "BIU", toolbar: "Text formatting" },
  "de-DE": { bold: "Fett", italic: "Kursiv", underline: "Unterstreichen", strikeThrough: "Durchstreichen", subscript: "Tiefstellen", superscript: "Hochstellen", more: "Weitere Formatierungen", buttons: "FKU", toolbar: "Textformatierung" },
});

export type CommentField = HTMLElement & { value: string; readOnly: boolean };

function scriptElement(lowered: boolean): HTMLElement {
  return lowered ? document.createElement("sub") : document.createElement("sup");
}

export function paintComment(element: HTMLElement, words: string, styles: CommentStyle[]): void {
  element.replaceChildren();
  let from = 0;
  for (const [start, end, flags] of styles) {
    if (start > from) element.append(words.slice(from, start));
    const span = document.createElement("span");
    let content: HTMLElement = span;
    if (flags & strike) {
      content = document.createElement("s");
      span.append(content);
    }
    if (flags & (subscript | superscript)) {
      const script = scriptElement((flags & subscript) !== 0);
      content.append(script);
      content = script;
    }
    content.append(words.slice(start, end));
    if (flags & 1) span.style.fontWeight = "bold";
    if (flags & 2) span.style.fontStyle = "italic";
    if (flags & 4) span.style.textDecoration = "underline";
    element.append(span);
    from = end;
  }
  if (from < words.length) element.append(words.slice(from));
}

// The native editor owns selections, IME composition and undo. Read its text
// and the styles that the PDF annotation writer preserves.
export function readComment(root: HTMLElement): { text: string; styles: CommentStyle[] } {
  let text = "";
  const styles: CommentStyle[] = [];
  function append(words: string, flags: number) {
    if (!words) return;
    const start = text.length;
    text += words;
    if (!flags) return;
    const last = styles.at(-1);
    if (last && last[1] === start && last[2] === flags) last[1] = text.length;
    else styles.push([start, text.length, flags]);
  }
  function visit(parent: HTMLElement, flags: number, followedInline = false) {
    let previousBlock = false;
    const children = parent.childNodes;
    for (let index = 0; index < children.length; index++) {
      const node = children[index]!;
      const elem = node as HTMLElement;
      const tag = elem.tagName?.toLowerCase();
      const block = tag === "div" || tag === "p";
      if ((block || previousBlock) && node !== parent.firstChild) append("\n", 0);
      previousBlock = block;
      if (node.nodeType === 3) {
        append(node.textContent ?? "", flags);
      } else if (tag === "br") {
        // A trailing br in an editing block holds the caret on its empty line.
        if (node !== parent.lastChild || followedInline) append("\n", flags);
      } else if (tag) {
        let style = flags;
        const css = elem.style;
        if (tag === "b" || tag === "strong" || css.fontWeight === "bold" || Number(css.fontWeight) >= 600) style |= 1;
        if (css.fontWeight === "normal" || css.fontWeight === "400") style &= ~1;
        if (tag === "i" || tag === "em" || css.fontStyle === "italic") style |= 2;
        if (css.fontStyle === "normal") style &= ~2;
        const decoration = css.textDecorationLine || css.textDecoration || "";
        if (tag === "u" || decoration.includes("underline")) style |= 4;
        if (tag === "s" || tag === "strike" || tag === "del" || decoration.includes("line-through")) style |= strike;
        if (tag === "sub" || css.verticalAlign === "sub") style = (style & ~superscript) | subscript;
        if (tag === "sup" || css.verticalAlign === "super") style = (style & ~subscript) | superscript;
        if (css.verticalAlign === "baseline") style &= ~(subscript | superscript);
        const next = children[index + 1] as HTMLElement | undefined;
        const followed = next ? !["DIV", "P"].includes(next.tagName) : followedInline;
        visit(elem, style, !block && followed);
      }
    }
  }
  visit(root, 0);
  return { text, styles };
}

export class CommentFormatting {
  readonly element = document.createElement("div");
  readonly field: CommentField;
  styles: CommentStyle[] = [];
  baseline: CommentStyle[] = [];
  private savedRange: Range | null = null;
  private readonly more = document.createElement("button");
  private readonly extra = document.createElement("div");
  private readonly buttons: HTMLButtonElement[];

  constructor() {
    const field = document.createElement("div");
    field.className = "comment-editor";
    field.setAttribute("role", "textbox");
    field.setAttribute("aria-multiline", "true");
    field.contentEditable = "true";
    let value = "";
    this.field = Object.defineProperties(field, {
      value: {
        get: () => value,
        set: (words: string) => {
          value = words;
          field.replaceChildren(words);
          this.styles = [];
          this.savedRange = null;
        },
      },
      readOnly: {
        get: () => field.contentEditable === "false",
        set: (locked: boolean) => {
          field.contentEditable = locked ? "false" : "true";
          field.setAttribute("aria-readonly", String(locked));
        },
      },
    }) as HTMLDivElement & CommentField;
    const labels = text();
    this.element.className = "comment-formatting";
    this.element.setAttribute("role", "group");
    this.element.setAttribute("aria-label", labels.toolbar);
    this.more.type = "button";
    this.more.textContent = "…";
    this.more.title = labels.more;
    this.more.setAttribute("aria-label", labels.more);
    this.more.addEventListener("mousedown", e => e.preventDefault());
    this.more.addEventListener("click", () => this.expand(this.extra.hidden === true));
    this.extra.className = "comment-extra-formats";
    this.buttons = commands.map((command, i) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "comment-format-" + command;
      if (command === "subscript" || command === "superscript") {
        const exponent = scriptElement(command === "subscript");
        exponent.textContent = "2";
        button.append("x", exponent);
      } else {
        button.textContent = command === "strikeThrough" ? labels.strikeThrough.charAt(0) : labels.buttons.charAt(i);
      }
      button.title = labels[command];
      button.setAttribute("aria-label", labels[command]);
      button.setAttribute("aria-pressed", "false");
      button.addEventListener("mousedown", e => e.preventDefault());
      button.addEventListener("click", () => apply(command));
      (i < basicCommands.length ? this.element : this.extra).append(button);
      return button;
    });
    this.element.append(this.more, this.extra);
    this.expand(false);
    const selectionChanged = () => {
      const sel = document.getSelection?.();
      if (!sel?.rangeCount || !field.contains(sel.anchorNode) || !field.contains(sel.focusNode)) return;
      this.savedRange = sel.getRangeAt(0).cloneRange();
      commands.forEach((cmd, i) => this.buttons[i]!.setAttribute("aria-pressed",
        document.queryCommandIndeterm(cmd) ? "mixed" : String(document.queryCommandState(cmd))));
    };
    const apply = (cmd: typeof commands[number]) => {
      if (this.field.readOnly) return;
      if (document.activeElement !== field) {
        const range = this.savedRange;
        field.focus();
        if (range && field.contains(range.commonAncestorContainer)) {
          const sel = document.getSelection();
          sel?.removeAllRanges();
          sel?.addRange(range);
        }
      }
      // Native editing commands share the browser's typing and undo history.
      document.execCommand(cmd);
      selectionChanged();
    };
    for (const event of ["focus", "keyup", "pointerup", "blur"]) {
      field.addEventListener(event, selectionChanged);
    }
    field.addEventListener("input", () => {
      const body = readComment(field);
      value = body.text;
      this.styles = body.styles;
      selectionChanged();
    });
    field.addEventListener("keydown", (event) => {
      if (!(event.metaKey || event.ctrlKey) || event.shiftKey || event.altKey || event.isComposing) return;
      const cmd = basicCommands.find(name => name[0] === event.key.toLowerCase());
      if (cmd) { event.preventDefault(); event.stopPropagation(); apply(cmd); }
    });
    field.addEventListener("paste", event => {
      event.preventDefault();
      event.stopPropagation();
      const words = event.clipboardData?.getData("text/plain");
      if (words && !this.field.readOnly) document.execCommand("insertText", false, words);
    });
    field.addEventListener("drop", event => { event.preventDefault(); event.stopPropagation(); });
  }

  private expand(open: boolean): void {
    this.extra.hidden = !open;
    this.more.setAttribute("aria-expanded", String(open));
  }

  load(words: string, styles: CommentStyle[] = []): void {
    this.field.value = words;
    this.styles = this.baseline = styles;
    if (styles.length) paintComment(this.field, words, styles);
    this.expand(false);
    for (const button of this.buttons) button.setAttribute("aria-pressed", "false");
  }

  get args(): [] | [CommentStyle[]] { return this.styles.length ? [this.styles] : []; }
  get changed(): boolean {
    return this.styles !== this.baseline && JSON.stringify(this.styles) !== JSON.stringify(this.baseline);
  }
}
