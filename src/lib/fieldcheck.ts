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

const SETTLE_MS = 20_000;

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
    if (anchor) {
      const from = { x: box.left + (anchor.left + anchor.right) / 2, y: box.top + (anchor.top + anchor.bottom) / 2 };
      drag(root, from, { x: from.x + 40, y: from.y + 30 });
      await settle(() => changed().length === 1, SETTLE_MS);
      await host.idle();
    }
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
    [...(panel()?.querySelectorAll("button") ?? [])].at(-1)?.click();
    await settle(() => changed()[0]?.props?.tooltip !== undefined, SETTLE_MS);
    await host.idle();
    const set = changed()[0]?.props;
    check(
      "applying it journals the parts that were changed, with the move",
      panel()?.style.display === "none" && changed().length === 1 && changed()[0]?.rect !== undefined
        && JSON.stringify(set) === JSON.stringify({ tooltip: "Your full name", required: true, align: "right" }),
      JSON.stringify(set),
    );
    host.run("edit.formEditOff");
    await pause(200);
    check("finishing brings the controls back", filling() > 0, `${filling()} shown`);
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
    const named = after.widgets.find((widget) => widget.name === "Name");
    check(
      "the save writes the field's properties",
      named?.tooltip === "Your full name" && named.required === true && named.align === "right" && named.read_only === false,
      `${named?.tooltip}; ${named?.required}; ${named?.align}; ${named?.read_only}`,
    );
    check(
      "and the save writes the field where it was dropped",
      Math.abs(saved[0] - to[0]) < 0.01 && Math.abs(saved[1] - to[1]) < 0.01,
      `${saved.map((v) => v.toFixed(1))} against ${to.map((v) => v.toFixed(1))}`,
    );
  }
}
