/** Real-window signature workflow on disposable files; driven by tabs_check.py. */
import type { OpenCheckHost } from "./opencheck";
import { settle, pause, type Report } from "./checkreport";
import { loadSignature, rememberSignature, forgetSignature } from "./signaturestore";
import { call } from "./ipc";

export async function signatureCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const check = (name: string, ok: boolean) => report.check(name, ok, "signature workflow");
  const [first, second] = expected.split("|");
  if (!first || !second) throw new Error("two disposable paths required");
  await host.open(first); await host.idle();
  const a = host.tabs().find((t) => t.path === first)!;
  const savedBefore = await loadSignature();
  const dialog = () => document.querySelector<HTMLDialogElement>(".signature-dialog[open]");
  const button = (label: string, inDialog = true) => [...(inDialog ? dialog()! : document).querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label)!;
  const pointer = (target: HTMLElement, type: string, x: number, y: number) => target.dispatchEvent(new PointerEvent(type, {
    bubbles: true, cancelable: true, pointerId: 31, pointerType: "mouse", button: 0,
    buttons: type === "pointerup" ? 0 : 1, clientX: x, clientY: y,
  }));
  const place = async () => {
    button("Place signature image").click(); await host.idle();
    if (!await settle(() => host.viewer()?.drawArmed === "signature", 3000)) throw new Error("signature tool did not arm");
    const root = document.querySelector<HTMLElement>(".surface")!, bounds = root.getBoundingClientRect();
    const start = host.viewer()!.screenPoint(0, 40, 40), end = host.viewer()!.screenPoint(0, 180, 100);
    pointer(root, "pointerdown", bounds.left+start.x, bounds.top+start.y);
    pointer(root, "pointermove", bounds.left+end.x, bounds.top+end.y);
    pointer(root, "pointerup", bounds.left+end.x, bounds.top+end.y);
    await host.idle();
  };
  host.run("edit.addSignature");
  if (!await settle(() => !!dialog(), 3000)) throw new Error("signature dialog did not open");
  check("the dialog states the visual signature limitation", dialog()!.textContent!.includes("does not verify your identity or create a certificate-based digital signature"));
  check("an empty signature cannot be placed", button("Place signature image").disabled);
  const canvas = dialog()!.querySelector("canvas")!, bounds = canvas.getBoundingClientRect();
  pointer(canvas, "pointerdown", bounds.left+30, bounds.top+60);
  pointer(canvas, "pointermove", bounds.left+80, bounds.top+100);
  pointer(canvas, "pointermove", bounds.left+160, bounds.top+50);
  pointer(canvas, "pointerup", bounds.left+160, bounds.top+50);
  check("drawing enables signature placement", !button("Place signature image").disabled);
  await place();
  let mark = host.edits()!.state.marks[0]!;
  check("drawing creates a signature with visible pixels", mark?.kind === "signature" && !!mark.image?.rgba.some((v,i) => i%4===3 && v>0));
  check("placement keeps the signature proportions", !!mark.image && Math.abs((mark.quads[2]!-mark.quads[0]!)/(mark.quads[3]!-mark.quads[1]!) - mark.image.width/mark.image.height) < 0.01);
  check("colour commands preserve signature pixels", !host.viewer()!.recolorOpenMark([1,0,0]));
  const initialWidth = mark.quads[2]!-mark.quads[0]!;
  button("Larger", false).click(); await host.idle();
  mark = host.edits()!.state.marks[0]!;
  check("the signature size control changes its width", Math.abs(mark.quads[2]!-mark.quads[0]!-initialWidth*1.25) < 0.01);
  button("Larger", false).click(); await host.idle();
  mark = host.edits()!.state.marks[0]!;
  check("repeated sizing uses the current width", Math.abs(mark.quads[2]!-mark.quads[0]!-initialWidth*1.25*1.25) < 0.01);
  host.run("edit.undo"); await host.idle();
  host.run("edit.undo"); await host.idle();
  mark = host.edits()!.state.marks[0]!;
  check("undo restores the signature size", Math.abs(mark.quads[2]!-mark.quads[0]!-initialWidth) < 0.01);
  host.run("edit.redo"); await host.idle();
  const beforeMove = host.edits()!.state.marks[0]!;
  host.viewer()!.closeMark();
  const root = document.querySelector<HTMLElement>(".surface")!, rootBounds = root.getBoundingClientRect();
  const from = host.viewer()!.screenPoint(0, 50, 50), to = host.viewer()!.screenPoint(0, 62, 64);
  pointer(root, "pointerdown", rootBounds.left+from.x, rootBounds.top+from.y);
  pointer(root, "pointermove", rootBounds.left+to.x, rootBounds.top+to.y);
  pointer(root, "pointerup", rootBounds.left+to.x, rootBounds.top+to.y);
  await host.idle();
  check("the signature moves without changing pixels", Math.abs(host.edits()!.state.marks[0]!.quads[0]!-beforeMove.quads[0]!-12) < 0.01
    && JSON.stringify(host.edits()!.state.marks[0]!.image) === JSON.stringify(beforeMove.image));
  await host.open(second); await host.idle();
  check("the other tab has no signature", host.edits()!.state.marks.length === 0);
  await host.activate(a.id); await host.idle();
  check("the signature survives tab switching", host.edits()!.state.marks[0]?.kind === "signature");
  host.viewer()!.showMark(host.edits()!.state.marks[0]!.id);
  pointer(button("Remove signature", false), "pointerdown", 0, 0); await host.idle();
  check("the signature can be removed", host.edits()!.state.marks.length === 0);
  host.run("edit.undo"); await host.idle();
  check("undo restores a removed signature", host.edits()!.state.marks[0]?.kind === "signature");
  host.run("edit.addSignature");
  if (!await settle(() => !!dialog(), 3000)) throw new Error("second signature dialog did not open");
  const source = document.createElement("canvas"); source.width = 40; source.height = 20;
  const ctx = source.getContext("2d")!; ctx.fillStyle = "#0033cc"; ctx.fillRect(2,2,30,4); ctx.fillRect(2,2,4,16);
  const blob = await new Promise<Blob>((resolve) => source.toBlob((b) => resolve(b!), "image/png"));
  const transfer = new DataTransfer(); transfer.items.add(new File([blob], "synthetic-signature.png", { type: "image/png" }));
  const input = dialog()!.querySelector<HTMLInputElement>('input[type="file"]')!;
  input.files = transfer.files; input.dispatchEvent(new Event("change"));
  if (!await settle(() => !button("Place signature image").disabled, 3000)) throw new Error("import did not finish");
  await place();
  check("imported pixels create a second signature", host.edits()!.state.marks.length === 2 && host.edits()!.state.marks.every((m) => !!m.image));
  check("local reuse is opt-in", JSON.stringify(await loadSignature()) === JSON.stringify(savedBefore));
  // The app uses an isolated test identifier. Restore even an existing saved value.
  try {
    host.run("edit.addSignature");
    if (!await settle(() => !!dialog(), 3000)) throw new Error("reuse dialog did not open");
    const sample = host.edits()!.state.marks[1]!.image!;
    await rememberSignature(sample);
    button("Use saved signature").click();
    check("a saved signature is ready to place", await settle(() => !button("Place signature image").disabled, 3000));
    const remember = [...dialog()!.querySelectorAll("label")].find((l) => l.textContent?.includes("Remember on this device"))!.querySelector("input")!;
    remember.checked = true;
    await place();
    check("remembering stores the accepted pixels", (await loadSignature())?.rgba.length === sample.rgba.length);
    host.run("edit.undo"); await host.idle();
    host.run("edit.addSignature");
    if (!await settle(() => !!dialog(), 3000)) throw new Error("forget dialog did not open");
    button("Forget saved signature").click();
    if (!await settle(() => dialog()!.querySelector('[role="status"]')?.textContent === "Saved signature removed from this device.", 3000)) throw new Error("forget did not finish");
    check("the saved signature can be forgotten", await loadSignature() === null);
    button("Cancel").click();
  } finally {
    if (savedBefore === null) await forgetSignature();
    else await rememberSignature(savedBefore);
  }
  host.run("file.save"); await host.idle();
  if (!await settle(() => !host.edits()!.state.dirty, 10000)) throw new Error("signature save did not finish");
  const comments = await call("document_comments", { doc: host.edits()!.doc });
  check("saved signatures reopen as PDF stamps", comments.items.filter((c) => c.kind === "stamp").length === 2);
  check("saving resets the signature journal", host.edits()!.state.marks.length === 0);
  await pause(100);
}
