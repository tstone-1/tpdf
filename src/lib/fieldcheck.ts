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
    fields().length === 1 && first?.field === "text" && first.note === "Text 1",
    `${fields().length} fields: ${names()}; kind ${first?.field}`,
  );
  check("the tool is spent by the drag", viewer.drawArmed === null, String(viewer.drawArmed));

  await place("edit.addCheckbox", at(0.3, 0.3), at(0.33, 0.33));
  const box1 = fields().find((mark) => mark.field === "checkbox");
  check(
    "the checkbox command places a checkbox, named apart from the text field",
    fields().length === 2 && box1?.note === "Checkbox 1",
    names(),
  );

  await place("edit.addMultilineField", at(0.3, 0.4), at(0.6, 0.5));
  const lines = fields().find((mark) => mark.field === "multiline");
  check(
    "the several-lines command places one, and it takes the next free name",
    fields().length === 3 && lines?.note === "Text 2",
    names(),
  );

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
}
