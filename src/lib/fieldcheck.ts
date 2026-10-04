/**
 * Placing form fields in the real application, and finding them in the file.
 *
 * `formfields.rs` and the journal are tested where they live. What no test
 * there reaches is the join: a command arming the drag, the drag reaching the
 * model with a kind and a name, the placed field drawn, renamed, undone, and
 * a save that turns it from a mark into a field of the document's form the
 * window then offers for filling. `App.svelte` is where those meet, and
 * nothing imports it.
 *
 * The driver reads the saved file once more with the command-line tool, which
 * shares the worker with this application and none of this window.
 */

import { call } from "./ipc";
import { SAVED_BASE } from "./savedfields";
import { DIALOG_CLASS as PROPERTIES_CLASS } from "./fieldprops";
import type { OpenCheckHost } from "./opencheck";
import { pause, settle, type Report } from "./checkreport";
import type { Viewer } from "./viewer";
import type { Anchor } from "./popup";

const SETTLE_MS = 20_000;

/**
 * The fraction of a rectangle of the page picture that is dark, or `null`
 * when there is nothing to read. The rectangle is in the surface's own CSS
 * pixels, as an anchor is.
 */
function dark(viewer: Viewer, box: { left: number; top: number; right: number; bottom: number }): number | null {
  const canvas = viewer.compositedSurface;
  const ctx = canvas?.getContext("2d", { willReadFrequently: true });
  if (!canvas || !ctx) return null;
  const dpr = window.devicePixelRatio || 1;
  const x0 = Math.max(0, Math.round(box.left * dpr));
  const y0 = Math.max(0, Math.round(box.top * dpr));
  const x1 = Math.min(canvas.width, Math.round(box.right * dpr));
  const y1 = Math.min(canvas.height, Math.round(box.bottom * dpr));
  if (x1 - x0 < 2 || y1 - y0 < 2) return null;
  const { data } = ctx.getImageData(x0, y0, x1 - x0, y1 - y0);
  let hit = 0;
  for (let at = 0; at < data.length; at += 4) {
    if ((data[at] ?? 255) + (data[at + 1] ?? 255) + (data[at + 2] ?? 255) < 384) hit++;
  }
  return hit / (data.length / 4);
}

/** A press, a move and a release on the page, in client pixels. */
function drag(root: HTMLElement, from: { x: number; y: number }, to: { x: number; y: number }): void {
  const at = (type: string, point: { x: number; y: number }) =>
    root.dispatchEvent(
      new PointerEvent(type, {
        button: 0, pointerId: 1, clientX: point.x, clientY: point.y, bubbles: true,
      }),
    );
  at("pointerdown", from);
  at("pointermove", to);
  at("pointerup", to);
}

export async function fieldCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const [path] = expected.split("|");
  if (!path) throw new Error("a disposable document is required");
  const check = (name: string, ok: boolean, detail = "") => report.check(name, ok, detail);
  await host.open(path);
  if (!(await settle(() => host.viewer()?.idle === true, SETTLE_MS))) {
    throw new Error("the document did not open");
  }
  const viewer = host.viewer()!;
  const root = document.querySelector<HTMLElement>(".surface");
  if (!root) throw new Error("the page surface is not mounted");
  const box = root.getBoundingClientRect();
  const marks = () => host.edits()?.state.marks ?? [];
  const fields = () => marks().filter((mark) => mark.kind === "field");
  const names = () => fields().map((mark) => mark.note).join(", ");
  const shown = () => document.querySelector('[data-testid="problem"]')?.textContent ?? "";
  /** A point on the page, as fractions of the visible surface. */
  const at = (x: number, y: number) => ({ x: box.left + box.width * x, y: box.top + box.height * y });
  const place = async (command: string, from: { x: number; y: number }, to: { x: number; y: number }) => {
    const before = fields().length;
    host.run(command);
    const armed = viewer.drawArmed;
    drag(root, from, to);
    await settle(() => fields().length > before, SETTLE_MS);
    await host.idle();
    return armed;
  };

  // The control: the same drag with no tool armed places nothing.
  drag(root, at(0.3, 0.2), at(0.5, 0.24));
  await pause(300);
  check("a drag with no tool armed places no field", marks().length === 0, `${marks().length} marks`);

  const armed = await place("edit.addTextField", at(0.3, 0.2), at(0.55, 0.24));
  const first = fields()[0];
  check("the text field command arms the field tool", armed === "field", String(armed));
  check(
    "a drag places a text field with a name of its own",
    fields().length === 1 && first?.field?.kind === "text" && first.note === "Text 1",
    `${fields().length} fields: ${names()}; kind ${first?.field?.kind}`,
  );
  check("the tool is spent by the drag", viewer.drawArmed === null, String(viewer.drawArmed));
  check("a text field is placed with a line round it, by default", first?.field?.border === true, String(first?.field?.border));

  // Dragged by its lower right corner, it grows, and stays where it was.
  if (first) {
    const anchor = viewer.markAnchor(first.id);
    const was = [...first.quads];
    if (anchor) {
      const corner = { x: box.left + anchor.right, y: box.top + anchor.bottom };
      drag(root, corner, { x: corner.x + 60, y: corner.y + 12 });
      await settle(() => fields()[0]?.quads[2] !== was[2], SETTLE_MS);
      await host.idle();
    }
    const now = fields()[0]?.quads ?? [];
    check(
      "dragging its corner makes it larger and leaves its upper left where it was",
      now[0] === was[0] && now[1] === was[1] && (now[2] ?? 0) > (was[2] ?? 0) && (now[3] ?? 0) > (was[3] ?? 0),
      `${was.map((v) => v.toFixed(1))} to ${now.map((v) => v.toFixed(1))}`,
    );
    host.run("edit.undo");
    await host.idle();
    check("undo gives it its size back", (fields()[0]?.quads ?? []).join() === was.join(), String(fields()[0]?.quads));
    host.run("edit.redo");
    await host.idle();
  }

  await place("edit.addCheckbox", at(0.3, 0.3), at(0.33, 0.33));
  const box1 = fields().find((mark) => mark.field?.kind === "checkbox");
  check(
    "the checkbox command places a checkbox, named apart from the text field",
    fields().length === 2 && box1?.note === "Checkbox 1",
    names(),
  );

  // With the line turned off, the next text field has none.
  host.run("edit.fieldBorderOff");
  await place("edit.addMultilineField", at(0.3, 0.4), at(0.6, 0.5));
  host.run("edit.fieldBorderOn");
  const lines = fields().find((mark) => mark.field?.kind === "multiline");
  check("a field placed with the line turned off has none", lines?.field?.border === false, String(lines?.field?.border));
  check(
    "the several-lines command places one, and it takes the next free name",
    fields().length === 3 && lines?.note === "Text 2",
    names(),
  );

  // Picked with Shift and arranged: every left edge goes to the first one's,
  // as one edit.
  {
    const press = (id: number, shiftKey: boolean) => {
      const anchor = viewer.markAnchor(id);
      if (!anchor) return;
      const point = {
        clientX: box.left + (anchor.left + anchor.right) / 2,
        clientY: box.top + (anchor.top + anchor.bottom) / 2,
      };
      for (const type of ["pointerdown", "pointerup"]) {
        root.dispatchEvent(new PointerEvent(type, { button: 0, pointerId: 1, bubbles: true, shiftKey, ...point }));
      }
    };
    const told = () => document.querySelector('[data-testid="notice"]')?.textContent ?? "";
    const lefts = () => fields().map((mark) => (mark.quads[0] ?? 0).toFixed(2));
    const widths = () => fields().map((mark) => ((mark.quads[2] ?? 0) - (mark.quads[0] ?? 0)).toFixed(2));
    const was = fields().map((mark) => mark.quads.join());
    const wide = widths().join(" ");
    const ids = fields().map((mark) => mark.id);
    // The checkbox first, so it is the one the others follow: its left edge
    // is the only one of the three that is not already where the others are.
    const order = [ids[1], ids[0], ids[2]].filter((id): id is number => id !== undefined);
    host.run("edit.alignRight");
    await host.idle();
    check("an arrangement with nothing picked does nothing", fields().map((mark) => mark.quads.join()).join("|") === was.join("|"), lefts().join(" "));
    // All three with Shift, so no field's name box opens under the check.
    // A press on the surface beside the page first: an earlier step pressed
    // the text field, which picked it, and a press with Shift on a picked
    // field takes it out.
    for (const type of ["pointerdown", "pointerup"]) {
      root.dispatchEvent(new PointerEvent(type, { button: 0, pointerId: 1, bubbles: true, clientX: box.left + 3, clientY: box.top + 3 }));
    }
    check("a press beside the fields picks none of them", viewer.pickedCount === 0, viewer.pickedMarks().join());
    const trail: string[] = [];
    for (const id of order) {
      const anchor = viewer.markAnchor(id);
      press(id, true);
      trail.push(`${id}@${anchor ? `${anchor.left.toFixed(0)},${anchor.top.toFixed(0)}` : "none"} open ${viewer.markOpen} -> [${viewer.pickedMarks().join()}]`);
    }
    await pause(100);
    check(
      "three presses with Shift pick three fields, in that order",
      viewer.pickedMarks().join() === order.join(),
      `${viewer.pickedMarks().join()} against ${order.join()}; ${trail.join(" | ")}`,
    );
    check("the reader is told how many are picked and which one leads", told().includes("3 picked"), told());
    const rights = () => fields().map((mark) => (mark.quads[2] ?? 0).toFixed(2));
    const lead = () => fields().find((mark) => mark.id === order[0]);
    host.run("edit.alignRight");
    await settle(() => new Set(rights()).size === 1, SETTLE_MS);
    await host.idle();
    check(
      "Arrange: align right puts every right edge where the first picked one's is, and keeps each width",
      new Set(rights()).size === 1 && rights()[0] === (lead()?.quads[2] ?? 0).toFixed(2)
        && lead()?.quads.join() === was[1] && widths().join(" ") === wide,
      `rights ${rights().join(" ")}; widths ${widths().join(" ")}`,
    );
    host.run("edit.undo");
    await host.idle();
    check("one undo puts all three back", fields().map((mark) => mark.quads.join()).join("|") === was.join("|"), rights().join(" "));
    // Nothing picked, and the journal as it was, for the steps below.
    viewer.pick([]);
    check("and nothing is picked afterwards", viewer.pickedCount === 0, String(viewer.pickedCount));
  }

  // Renaming is the mark's note, held to what a field's name may be.
  if (first && lines) {
    await host.apply((edits) => edits.renote(first.id, "Name"));
    check("a field is renamed", fields().some((mark) => mark.note === "Name"), names());
    await host.apply((edits) => edits.renote(lines.id, "Name")).catch(() => undefined);
    await host.idle();
    check(
      "a second field cannot take a name another has, and the reader is told",
      fields().filter((mark) => mark.note === "Name").length === 1 && shown().includes("has this name"),
      `${names()}; shown: ${shown()}`,
    );
  }

  host.run("edit.undo");
  await host.idle();
  host.run("edit.undo");
  await host.idle();
  check("undo takes back the rename and then the last field", fields().length === 2 && names().includes("Text 1"), names());
  host.run("edit.redo");
  await host.idle();
  host.run("edit.redo");
  await host.idle();
  check("redo brings both back", fields().length === 3 && names().includes("Name"), names());

  // A field that is still a mark takes its properties from the same panel.
  {
    const placed = fields().find((mark) => mark.field?.kind === "multiline");
    const sheet = () => document.querySelector<HTMLElement>(`.${PROPERTIES_CLASS}`);
    viewer.pick(placed ? [placed.id] : []);
    host.run("edit.fieldProperties");
    const opened = await settle(() => sheet()?.style.display === "flex", SETTLE_MS);
    const most = sheet()?.querySelector<HTMLInputElement>('[aria-label="Most characters"]') ?? null;
    const tip = sheet()?.querySelector<HTMLInputElement>('[aria-label="Tooltip"]') ?? null;
    check(
      "the properties command opens the panel for a field placed and not yet saved",
      opened && sheet()?.querySelector("h2")?.textContent === `Properties of ${placed?.note}`,
      `${sheet()?.querySelector("h2")?.textContent}`,
    );
    if (most) most.value = "40";
    if (tip) tip.value = "Notes";
    const size = sheet()?.querySelector<HTMLInputElement>('[aria-label="Text size"]') ?? null;
    const start = sheet()?.querySelector<HTMLInputElement>('[aria-label="Default value"]') ?? null;
    if (size) size.value = "10";
    if (start) start.value = "first";
    [...(sheet()?.querySelectorAll("button") ?? [])].at(-1)?.click();
    const now = () => fields().find((mark) => mark.id === placed?.id)?.field;
    await settle(() => now()?.max_length === 40, SETTLE_MS);
    await host.idle();
    check(
      "applying it changes the placed field",
      now()?.max_length === 40 && now()?.tooltip === "Notes" && now()?.kind === "multiline"
        && now()?.text_size === 10 && now()?.default_value === "first",
      JSON.stringify(now()),
    );
    host.run("edit.undo");
    await host.idle();
    check("undo takes the properties back", now()?.max_length === undefined && now()?.tooltip === undefined, JSON.stringify(now()));
    host.run("edit.redo");
    await host.idle();

    // A copy of it, beside it, picked in its place; then moved by a key.
    const before = new Set(fields().map((mark) => mark.id));
    viewer.pick(placed ? [placed.id] : []);
    host.run("edit.duplicate");
    await settle(() => fields().length === 4, SETTLE_MS);
    await host.idle();
    const copy = () => fields().find((mark) => !before.has(mark.id));
    const from = fields().find((mark) => mark.id === placed?.id)?.quads ?? [];
    check(
      "duplicate makes a copy of the picked field with its properties, under a name of its own, a step away",
      copy()?.field?.kind === "multiline" && copy()?.field?.max_length === 40 && copy()?.field?.tooltip === "Notes"
        && copy()?.note !== placed?.note && !names().split(", ").slice(0, 3).includes(copy()?.note ?? "")
        && Math.abs((copy()?.quads[0] ?? 0) - (from[0] ?? 0) - 12) < 0.01
        && Math.abs((copy()?.quads[1] ?? 0) - (from[1] ?? 0) - 12) < 0.01,
      `${JSON.stringify(copy()?.field)}; ${copy()?.note}; ${copy()?.quads.map((v) => v.toFixed(1))} from ${from.map((v) => v.toFixed(1))}`,
    );
    check("and picks the copy", viewer.pickedMarks().join() === String(copy()?.id), viewer.pickedMarks().join());
    const left = copy()?.quads[0] ?? 0;
    root.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", shiftKey: true, bubbles: true }));
    await settle(() => Math.abs((copy()?.quads[0] ?? 0) - left - 10) < 0.01, SETTLE_MS);
    await host.idle();
    check(
      "the right arrow with Shift moves the picked field ten points",
      Math.abs((copy()?.quads[0] ?? 0) - left - 10) < 0.01,
      `${left.toFixed(1)} to ${(copy()?.quads[0] ?? 0).toFixed(1)}`,
    );
    host.run("edit.undo");
    await host.idle();
    host.run("edit.undo");
    await host.idle();
    check("two undos take back the nudge and the copy", fields().length === 3 && copy() === undefined, names());
    viewer.pick([]);
  }

  host.run("file.save");
  await host.idle();
  const saved = await settle(
    () => host.edits()?.state.dirty === false && marks().length === 0,
    SETTLE_MS,
  );
  check("saving leaves nothing unsaved and no field still a mark", saved, `${marks().length} marks; ${shown()}`);
  const edits = host.edits();
  if (!edits) throw new Error("the document did not reopen after the save");
  const form = await call("document_form", { doc: edits.doc });
  const written = form.widgets.map((widget) => `${widget.name}:${widget.control.kind}`).sort().join(", ");
  check(
    "the reopened document's form holds the three fields, by name and kind",
    written === "Checkbox 1:checkbox, Name:text, Text 2:text"
      && form.widgets.find((widget) => widget.name === "Text 2")?.multiline === true
      && form.widgets.every((widget) => widget.reason === null),
    written,
  );
  const lined = form.widgets.find((widget) => widget.name === "Text 2");
  check(
    "and the field placed with properties is saved with them",
    lined?.max_length === 40 && lined.tooltip === "Notes"
      && lined.text_size === 10 && lined.default_value === "first" && lined.value === "first",
    `${lined?.max_length}; ${lined?.tooltip}; ${lined?.text_size}; ${lined?.default_value}; ${JSON.stringify(lined?.value)}`,
  );
  const controls = await settle(
    () => document.querySelectorAll(".form-fields input, .form-fields textarea").length >= 3,
    SETTLE_MS,
  );
  check(
    "and the window offers all three for filling",
    controls,
    `${document.querySelectorAll(".form-fields input, .form-fields textarea").length} controls`,
  );

  // A fourth field on the saved form starts counting past the names it holds.
  await place("edit.addTextField", at(0.3, 0.6), at(0.55, 0.64));
  check("a field placed on the saved form avoids the names the file now has", names() === "Text 1", names());
  await place("edit.addCheckbox", at(0.3, 0.7), at(0.33, 0.73));
  check("and a checkbox there is the second of its kind", names() === "Text 1, Checkbox 2", names());

  // The fields the file now has, changed in place: shown as rectangles, one
  // dragged, and the save writes it where it was dropped.
  {
    const changed = () => host.edits()?.state.fields ?? [];
    const filling = () =>
      [...document.querySelectorAll<HTMLElement>(".form-fields input, .form-fields textarea")]
        .filter((control) => control.style.display !== "none").length;
    const at0 = form.widgets.findIndex((widget) => widget.name === "Name");
    const was = form.widgets[at0]?.display_rect ?? [0, 0, 0, 0];
    // The two fields placed since the save are taken back first: the driver
    // reads the file this block saves and expects the three of the first save.
    host.run("edit.undo");
    await host.idle();
    host.run("edit.undo");
    await host.idle();
    check("the two fields placed since the save are taken back", marks().length === 0, `${marks().length} marks`);
    // The save reopened the document, so the viewer and the surface are new.
    const viewer = host.viewer()!;
    const root = document.querySelector<HTMLElement>(".surface")!;
    const box = root.getBoundingClientRect();
    // A dropdown, placed through the command that asks for its choices.
    const at = (x: number, y: number) => ({ x: box.left + box.width * x, y: box.top + box.height * y });
    host.run("edit.addDropdown", "Yes; No, by post");
    check("the dropdown command arms the field tool once it has its choices", viewer.drawArmed === "field", String(viewer.drawArmed));
    drag(root, at(0.3, 0.8), at(0.55, 0.84));
    await settle(() => fields().length === 1, SETTLE_MS);
    await host.idle();
    const chooser = fields()[0];
    check(
      "a drag places a dropdown that holds the choices typed, under a name of its own",
      chooser?.field?.kind === "dropdown" && chooser.field.options?.join("|") === "Yes|No, by post"
        && chooser.note === "Dropdown 1",
      `${chooser?.field?.kind}; ${chooser?.field?.options?.join("|")}; ${chooser?.note}`,
    );
    // Radio buttons: the command takes the group's name, and the tool stays
    // armed from one button to the next.
    host.run("edit.addRadio", "Pay");
    drag(root, at(0.62, 0.6), at(0.64, 0.62));
    await settle(() => fields().length === 2, SETTLE_MS);
    await host.idle();
    const stillArmed = viewer.drawArmed;
    drag(root, at(0.62, 0.66), at(0.64, 0.68));
    await settle(() => fields().length === 3, SETTLE_MS);
    await host.idle();
    const group = fields().filter((mark) => mark.field?.kind === "radio");
    check(
      "two drags place two radio buttons of the group named, each with a value of its own",
      group.length === 2 && group.every((mark) => mark.note === "Pay")
        && group.map((mark) => mark.field?.options?.[0]).join("|") === "Choice 1|Choice 2",
      group.map((mark) => `${mark.note}:${mark.field?.options?.join()}`).join("; "),
    );
    check("and the tool stays armed between them", stillArmed === "field", String(stillArmed));
    // On the page, which is where the viewer listens and where the pointer
    // has just been.
    root.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await pause(100);
    check("Escape puts the tool down", viewer.drawArmed === null, String(viewer.drawArmed));
    host.run("edit.formEditOn");
    await pause(200);
    const id = SAVED_BASE + at0;
    const anchor = viewer.markAnchor(id);
    check(
      "changing the document's fields shows the saved field as a rectangle",
      anchor !== null,
      `at ${at0}; pages ${JSON.stringify(host.edits()?.state.pages.map((page) => [page.id, page.source]))}; widget pages ${form.widgets.map((w) => w.page).join()}`,
    );
    check("and puts the controls for filling away", filling() === 0, `${filling()} shown`);
    // The line round the field, on the page picture: a strip over its left
    // edge where the file has the field, and the same strip where the drag
    // puts it. The picture and not the overlay, which draws the rectangle a
    // reader drags and would say yes whatever the page showed.
    const strip = (dx: number, dy: number) => anchor && {
      left: anchor.left - 3 + dx, right: anchor.left + 3 + dx, top: anchor.top + 3 + dy, bottom: anchor.bottom - 3 + dy,
    };
    const drawn = async (dx: number, dy: number) => {
      await settle(() => viewer.idle, SETTLE_MS);
      await pause(300);
      const at = strip(dx, dy);
      return at ? dark(viewer, at) : null;
    };
    const oldBefore = await drawn(0, 0);
    const newBefore = await drawn(40, 30);
    check(
      "the page picture shows the saved field's line where the file has it",
      oldBefore !== null && newBefore !== null && oldBefore > 0.05 && oldBefore > newBefore + 0.05,
      `${oldBefore?.toFixed(3)} at the field, ${newBefore?.toFixed(3)} where it will go`,
    );
    if (anchor) {
      const from = { x: box.left + (anchor.left + anchor.right) / 2, y: box.top + (anchor.top + anchor.bottom) / 2 };
      drag(root, from, { x: from.x + 40, y: from.y + 30 });
      await settle(() => changed().length === 1, SETTLE_MS);
      await host.idle();
    }
    const oldMoved = await drawn(0, 0);
    const newMoved = await drawn(40, 30);
    check(
      "a moved field is drawn where it was dropped before it is saved, and no longer where the file has it",
      oldBefore !== null && newBefore !== null && oldMoved !== null && newMoved !== null
        && oldMoved < oldBefore - 0.05 && newMoved > newBefore + 0.05,
      `old place ${oldBefore?.toFixed(3)} to ${oldMoved?.toFixed(3)}, new place ${newBefore?.toFixed(3)} to ${newMoved?.toFixed(3)}`,
    );
    const to = changed()[0]?.rect ?? [0, 0, 0, 0];
    check(
      "dragging it moves the field, at the size it had",
      changed().length === 1 && to[0] > was[0] && to[1] > was[1]
        && Math.abs((to[2] - to[0]) - (was[2] - was[0])) < 0.01,
      `${was.map((v) => v.toFixed(1))} to ${to.map((v) => v.toFixed(1))}`,
    );
    host.run("edit.undo");
    await host.idle();
    check("undo puts it back", changed().length === 0, String(changed().length));
    const oldUndone = await drawn(0, 0);
    const newUndone = await drawn(40, 30);
    check(
      "and the page picture with it",
      oldUndone !== null && newUndone !== null && oldBefore !== null && newBefore !== null
        && Math.abs(oldUndone - oldBefore) < 0.02 && Math.abs(newUndone - newBefore) < 0.02,
      `old place ${oldUndone?.toFixed(3)}, new place ${newUndone?.toFixed(3)}`,
    );
    host.run("edit.redo");
    await host.idle();
    // The moved field's properties, set in the panel the command opens: the
    // join of the pick, the command, the panel and the change it makes.
    viewer.pick([]);
    host.run("edit.fieldProperties");
    await pause(100);
    const panel = () => document.querySelector<HTMLElement>(`.${PROPERTIES_CLASS}`);
    check("with no field picked the properties command opens nothing", panel()?.style.display !== "flex", String(panel()?.style.display));
    viewer.pick([id]);
    host.run("edit.fieldProperties");
    const opened = await settle(() => panel()?.style.display === "flex", SETTLE_MS);
    const heading = panel()?.querySelector("h2")?.textContent ?? "";
    check("with the field picked it opens the panel, under the field's name", opened && heading === "Properties of Name", heading);
    const control = <T extends HTMLElement>(label: string) => panel()?.querySelector<T>(`[aria-label="${label}"]`) ?? null;
    const tooltip = control<HTMLInputElement>("Tooltip");
    const align = control<HTMLSelectElement>("Alignment");
    const required = panel()?.querySelector<HTMLInputElement>('input[type="checkbox"]') ?? null;
    if (tooltip) tooltip.value = "Your full name";
    if (align) align.value = "right";
    if (required) required.checked = true;
    const textSize = control<HTMLInputElement>("Text size");
    const defaultValue = control<HTMLInputElement>("Default value");
    if (textSize) textSize.value = "9";
    if (defaultValue) defaultValue.value = "n/a";
    [...(panel()?.querySelectorAll("button") ?? [])].at(-1)?.click();
    await settle(() => changed()[0]?.props?.tooltip !== undefined, SETTLE_MS);
    await host.idle();
    const set = changed()[0]?.props;
    check(
      "applying it journals the parts that were changed, with the move",
      panel()?.style.display === "none" && changed().length === 1 && changed()[0]?.rect !== undefined
        && JSON.stringify(set) === JSON.stringify({
          tooltip: "Your full name", required: true, align: "right", text_size: 9, default_value: "n/a",
        }),
      JSON.stringify(set),
    );
    host.run("edit.formEditOff");
    await pause(200);
    check("finishing brings the controls back", filling() > 0, `${filling()} shown`);
    const held = document.querySelector<HTMLInputElement>('.form-fields input[aria-label="Name"]')?.value;
    check("and the field that held nothing shows the default it was given", held === "n/a", String(held));
    // The tab order, asked for once and then not on offer until it is saved.
    host.run("edit.tabOrder");
    await settle(() => host.edits()?.state.tab_order === true, SETTLE_MS);
    await host.idle();
    const asked = host.edits()?.state.tab_order === true;
    const journal = JSON.stringify(host.edits()?.state.fields);
    host.run("edit.tabOrder");
    await host.idle();
    check(
      "the tab order command asks for reading order, and asking again changes nothing",
      asked && host.edits()?.state.tab_order === true && JSON.stringify(host.edits()?.state.fields) === journal,
      String(host.edits()?.state.tab_order),
    );
    host.run("file.save");
    await host.idle();
    await settle(() => host.edits()?.state.dirty === false, SETTLE_MS);
    const now = host.edits();
    const after = now ? await call("document_form", { doc: now.doc }) : { widgets: [] };
    const saved = after.widgets.find((widget) => widget.name === "Name")?.display_rect ?? [0, 0, 0, 0];
    const listed = after.widgets.find((widget) => widget.name === "Dropdown 1")?.control;
    check(
      "the save writes the dropdown as a list of its choices",
      listed?.kind === "choice" && listed.combo && listed.options.map((o) => o.label).join("|") === "Yes|No, by post",
      JSON.stringify(listed),
    );
    check("after the save the order is no longer something asked for", now?.state.tab_order === false, String(now?.state.tab_order));
    const buttons = after.widgets.filter((widget) => widget.name === "Pay");
    check(
      "the save writes the two buttons as one group of radio buttons",
      buttons.length === 2 && buttons.every((widget) => widget.control.kind === "radio" && widget.reason === null)
        && buttons[0]?.object.join() === buttons[1]?.object.join(),
      JSON.stringify(buttons.map((widget) => [widget.object, widget.control.kind, widget.reason])),
    );
    const named = after.widgets.find((widget) => widget.name === "Name");
    check(
      "the save writes the field's properties",
      named?.tooltip === "Your full name" && named.required === true && named.align === "right" && named.read_only === false,
      `${named?.tooltip}; ${named?.required}; ${named?.align}; ${named?.read_only}`,
    );
    check(
      "with its text size, and its default value as the answer it had none of",
      named?.text_size === 9 && named.default_value === "n/a" && named.value === "n/a",
      `${named?.text_size}; ${named?.default_value}; ${JSON.stringify(named?.value)}`,
    );
    check(
      "and the save writes the field where it was dropped",
      Math.abs(saved[0] - to[0]) < 0.01 && Math.abs(saved[1] - to[1]) < 0.01,
      `${saved.map((v) => v.toFixed(1))} against ${to.map((v) => v.toFixed(1))}`,
    );
  }
}

/**
 * A field on a page that is turned, by the document and then once more by the
 * reader. The save has to give the field the page's turn, or its answer is
 * drawn lying on its side; `formfields.rs` and the save are tested for that
 * where they live, and this is the drag in the window reaching them.
 *
 * The document's first page is turned a quarter by the file.
 */
export async function turnedFieldCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const [path] = expected.split("|");
  if (!path) throw new Error("a disposable document with a turned first page is required");
  const check = (name: string, ok: boolean, detail = "") => report.check(name, ok, detail);
  await host.open(path);
  if (!(await settle(() => host.viewer()?.idle === true, SETTLE_MS))) {
    throw new Error("the document did not open");
  }
  const fields = () => (host.edits()?.state.marks ?? []).filter((mark) => mark.kind === "field");
  const shown = () => document.querySelector('[data-testid="problem"]')?.textContent ?? "";
  /** The window as it is now: a save reopens the document and a turn lays the page out again. */
  const stage = () => {
    const viewer = host.viewer()!;
    const root = document.querySelector<HTMLElement>(".surface")!;
    return { viewer, root, box: root.getBoundingClientRect() };
  };
  /** A text field dragged out wider than tall, and where the window drew it. */
  const place = async () => {
    const { viewer, root, box } = stage();
    const before = fields().length;
    host.run("edit.addTextField");
    drag(
      root,
      { x: box.left + box.width * 0.3, y: box.top + box.height * 0.2 },
      { x: box.left + box.width * 0.55, y: box.top + box.height * 0.25 },
    );
    await settle(() => fields().length > before, SETTLE_MS);
    await host.idle();
    const mark = fields().at(-1);
    return { mark, anchor: mark ? viewer.markAnchor(mark.id) : null };
  };
  const save = async () => {
    host.run("file.save");
    await host.idle();
    const done = await settle(() => host.edits()?.state.dirty === false && fields().length === 0, SETTLE_MS);
    const edits = host.edits();
    if (!done || !edits) throw new Error(`the save did not finish: ${shown()}`);
    await settle(() => host.viewer()?.idle === true, SETTLE_MS);
    return (await call("document_form", { doc: edits.doc })).widgets;
  };
  /** Where the window draws a saved field, which it does while fields are being changed. */
  const savedAnchor = async (at: number) => {
    host.run("edit.formEditOn");
    await pause(300);
    const { viewer } = stage();
    await settle(() => viewer.idle, SETTLE_MS);
    await pause(300);
    return { viewer, anchor: viewer.markAnchor(SAVED_BASE + at) };
  };
  const size = (rect: readonly number[]) => `${((rect[2] ?? 0) - (rect[0] ?? 0)).toFixed(1)} by ${Math.abs((rect[3] ?? 0) - (rect[1] ?? 0)).toFixed(1)}`;
  const near = (a: Anchor | null, b: Anchor | null) =>
    a !== null && b !== null && Math.abs(a.left - b.left) < 2 && Math.abs(a.top - b.top) < 2
      && Math.abs(a.right - b.right) < 2 && Math.abs(a.bottom - b.bottom) < 2;
  const told = (a: Anchor | null) => (a ? [a.left, a.top, a.right, a.bottom].map((v) => v.toFixed(0)).join() : "none");

  // The quarter turn the file gives the page is not in the journal, which
  // counts the reader's turns; the first save below says it, in the field.
  const byReader = () => host.edits()?.state.pages[0]?.turns ?? 0;
  check("the reader has not turned the page", byReader() % 4 === 0, String(byReader()));

  // On the page the file turns.
  const first = await place();
  check(
    "a drag on a page the document turns places a text field, and no problem is shown",
    first.mark?.field?.kind === "text" && first.anchor !== null && shown() === "",
    `${first.mark?.field?.kind}; shown: ${shown()}`,
  );
  let widgets = await save();
  const one = widgets.findIndex((widget) => widget.name === first.mark?.note);
  check(
    "the save writes it with the page's quarter turn, lying along the page's long side",
    widgets[one]?.turns === 1 && widgets[one]?.reason === null
      && widgets[one].rect[3] - widgets[one].rect[1] > widgets[one].rect[2] - widgets[one].rect[0],
    `turns ${widgets[one]?.turns}; ${size(widgets[one]?.rect ?? [])} in the page; ${widgets[one]?.reason}`,
  );
  {
    const { viewer, anchor } = await savedAnchor(one);
    check("and where it was dragged", near(anchor, first.anchor), `${told(first.anchor)} placed, ${told(anchor)} saved`);
    // The line round it, on the page picture and not the overlay: a strip
    // over its left edge against the same strip beside the field.
    const strip = (dx: number) => anchor && {
      left: anchor.left - 3 + dx, right: anchor.left + 3 + dx, top: anchor.top + 3, bottom: anchor.bottom - 3,
    };
    const on = strip(0);
    const off = strip(-40);
    const line = on ? dark(viewer, on) : null;
    const beside = off ? dark(viewer, off) : null;
    check(
      "the page picture has the field's line there",
      line !== null && beside !== null && line > beside + 0.05,
      `${line?.toFixed(3)} on its left edge, ${beside?.toFixed(3)} beside it`,
    );
    host.run("edit.formEditOff");
    await pause(200);
  }

  // Turned once more by the reader, with the turn not yet saved.
  host.run("edit.rotatePageClockwise");
  await settle(() => byReader() % 4 === 1, SETTLE_MS);
  await host.idle();
  await settle(() => host.viewer()?.idle === true, SETTLE_MS);
  await pause(300);
  check("the reader turns the page a quarter more", byReader() % 4 === 1, String(byReader()));
  const second = await place();
  check(
    "a drag on the page turned in this session places a second field",
    second.mark?.field?.kind === "text" && second.anchor !== null && second.mark.note !== first.mark?.note && shown() === "",
    `${second.mark?.note}; shown: ${shown()}`,
  );
  widgets = await save();
  const two = widgets.findIndex((widget) => widget.name === second.mark?.note);
  const drawn = widgets[two]?.display_rect ?? [0, 0, 0, 0];
  check(
    "the save writes it with both turns, wider than tall as a reader sees the page",
    widgets[two]?.turns === 2 && widgets[two]?.reason === null && drawn[2] - drawn[0] > Math.abs(drawn[3] - drawn[1]),
    `turns ${widgets[two]?.turns}; ${size(drawn)} as displayed; ${widgets[two]?.reason}`,
  );
  {
    const { anchor } = await savedAnchor(two);
    check("and where it was dragged on the turned page", near(anchor, second.anchor), `${told(second.anchor)} placed, ${told(anchor)} saved`);
    host.run("edit.formEditOff");
    await pause(200);
  }
  const controls = document.querySelectorAll(".form-fields input, .form-fields textarea").length;
  check("the window offers both for filling", controls >= 2, `${controls} controls`);
}
