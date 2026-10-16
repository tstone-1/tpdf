/**
 * The parts tpdf's own dialogs are built from, in the classes `dialog.css`
 * gives a look. Built from `textContent` only, since what a dialog shows is
 * often somebody else's text: a certificate's subject, a file's name.
 */

/** A paragraph with one class and its words. */
export function paragraph(className: string, text: string): HTMLParagraphElement {
  const p = document.createElement("p");
  p.className = className;
  p.textContent = text;
  return p;
}

/**
 * A section of a dialog: its label, then whatever is appended. A question is
 * set as a sentence rather than as a small heading.
 */
export function section(label: string, question = false): HTMLDivElement {
  const div = document.createElement("div");
  div.className = "tpdf-dialog-section";
  div.append(paragraph(question ? "tpdf-dialog-label question" : "tpdf-dialog-label", label));
  return div;
}

/** A radio button or checkbox with its words, and a smaller line under them when there is one. */
export function option(input: HTMLInputElement, title: string, hint = ""): HTMLLabelElement {
  const label = document.createElement("label");
  label.className = "tpdf-dialog-option";
  const words = document.createElement("span");
  const top = document.createElement("span");
  top.className = "tpdf-dialog-option-title";
  top.textContent = title;
  words.append(top);
  if (hint !== "") {
    const under = document.createElement("span");
    under.className = "tpdf-dialog-hint";
    under.textContent = hint;
    words.append(under);
  }
  label.append(input, words);
  return label;
}

/** A row of controls that wraps: buttons, short options, fields. */
export function row(): HTMLDivElement {
  const div = document.createElement("div");
  div.className = "tpdf-dialog-row";
  return div;
}

/** The row of buttons at the foot of a dialog, in the order given. */
export function footer(...buttons: HTMLButtonElement[]): HTMLDivElement {
  const div = document.createElement("div");
  div.className = "tpdf-dialog-footer";
  div.append(...buttons);
  return div;
}

/** A button; `primary` is the one that does the dialog's work, drawn in the accent colour. */
export function button(text: string, primary = false): HTMLButtonElement {
  const b = document.createElement("button");
  b.type = "button";
  b.textContent = text;
  if (primary) b.className = "primary";
  return b;
}
