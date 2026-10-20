/**
 * Mark and crop commands, printing, and adding validation data.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { invoke } from "@tauri-apps/api/core";
import type { CropGeometry } from "../crop";
import { type DocumentInfo, call } from "../ipc";
import type { EditState } from "../edits";
import { baselineOf } from "../pages";
import { check, describeThrown, skip } from "./common";
import { preview } from "./invert";

/** What this phase reports, so a run that cannot start still prints the names. */
const MARK_COMMAND_CHECKS = [
  "the model takes a highlight through the command",
  "the model takes each kind of mark and reports it back",
  "the model takes a note through the command",
  "the model takes a mark off through the command",
  "a note on a mark that is gone is refused in the reader's words",
];

/**
 * One backend command, with a throw turned into a message rather than a stack.
 *
 * **Not a convenience.** An `invoke` for a command that is not registered
 * *rejects*, and an unguarded `await` on it walks out of the phase, past every
 * check below it, and ends the run --- so the finding arrives as
 * `[FAIL] run completed  Command annot_note not found` with no check name on
 * it at all. Measured: the mutation that removes `annot_note` from
 * `generate_handler!` was reported **SURVIVED** by `mutate_viewer.py` for
 * exactly that reason, because the check it names never printed. Caught here,
 * the same mutation reddens the check that names the command, which is what
 * makes it attributable.
 */
export async function attempt(
  command: string,
  args: Record<string, unknown>,
): Promise<{ state: EditState | null; error: string }> {
  try {
    return { state: await invoke<EditState>(command, args), error: "" };
  } catch (e) {
    return { state: null, error: String(e) };
  }
}

/** What this phase reports, so a run that cannot start still prints the names. */
const CROP_CHECKS = [
  "the backend measures a page's content box",
  "the content box is inside the page and encloses something",
  "the model takes a crop through the command and reports it back",
  "a cropped page is smaller than the page the file describes",
  "the crop sits inside the file's own page",
  "clearing the crop puts the page back",
  "a crop that encloses nothing is refused in the reader's words",
  "the backend maps a dragged rectangle into a crop box",
  "a crop placed on screen and dragged back out is the crop it started as",
  "a rectangle dragged past the paper crops to the paper",
];

/**
 * The crop commands, driven against the real backend.
 *
 * Registration is the thing no other layer can say --- see
 * {@link markCommandChecks} --- and there are three commands here rather than
 * one: the measurement, the geometry the layout needs, and the model command.
 * Each is registered separately and any of the three can be the one missing from
 * `generate_handler!`.
 *
 * The last check is the control. Every answer above it could come from a backend
 * that accepts whatever it is handed, and a crop box whose corners are the wrong
 * way round has to come back as the refusal the model names.
 *
 * It runs on **every** corpus, including the two with no extractable text: a
 * content box is measured from pixels, so a page of vector drawing has one and a
 * page of text has one, and the only page that would skip is a blank one.
 */
export async function cropCommandChecks(doc: DocumentInfo): Promise<void> {
  const opened = await attempt("edit_state", { doc: doc.id });
  const first = opened.state?.pages[0];
  if (!first) {
    for (const name of CROP_CHECKS) {
      skip(name, opened.error || "the model reports no pages");
    }
    return;
  }

  const page = baselineOf(first.source);
  let found: [number, number, number, number] | null = null;
  let why = "";
  try {
    found = await invoke<[number, number, number, number] | null>(
      "page_content_box",
      // **`baselineOf`, not `first.source`.** This asks PDFium about the file on
      // disk, so the argument is a page NUMBER; `PageSource` became a union on
      // 2026-08-30 and this line kept passing the whole object, which serialises
      // as a map and is refused by the command with `expected u32`. `invoke` is
      // untyped, so nothing compiled against it and only this harness -- which
      // needs a screen -- could ever say so.
      { doc: doc.id, page },
    );
  } catch (e) {
    why = String(e);
  }
  check(CROP_CHECKS[0] ?? "", found !== null, why || `box ${JSON.stringify(found)}`);
  if (!found) {
    for (const name of CROP_CHECKS.slice(1)) {
      skip(name, why || "the page has no ink to crop to");
    }
    return;
  }

  // `baselineOf` for the reason the call above states, and these two are why
  // that one mattered so little on its own: the `.catch(() => null)` turns a
  // REFUSED ARGUMENT into a plausible `null`, so a wiring bug arrives as "the
  // backend measured nothing" -- and every check below reads `undefined` and
  // blames the geometry. Three call sites, one mistake, and only the one
  // without a catch said anything.
  const whole = await invoke<CropGeometry>("page_geometry", {
    doc: doc.id,
    page,
    crop: null,
  }).catch(() => null);
  const at = await invoke<CropGeometry>("page_geometry", {
    doc: doc.id,
    page,
    crop: found,
  }).catch(() => null);
  check(
    CROP_CHECKS[1] ?? "",
    !!whole && !!at && at.width_pt > 0 && at.height_pt > 0,
    `${JSON.stringify(at)} inside ${JSON.stringify(whole)}`,
  );

  const cropped = await attempt("page_crop", {
    doc: doc.id,
    page: first.id,
    to: found,
  });
  const back = cropped.state?.pages[0]?.crop;
  check(
    CROP_CHECKS[2] ?? "",
    !!back && back.every((value, index) => Math.abs(value - (found?.[index] ?? NaN)) < 0.01),
    cropped.error || `sent ${JSON.stringify(found)}, back ${JSON.stringify(back)}`,
  );
  check(
    CROP_CHECKS[3] ?? "",
    !!whole &&
      !!at &&
      at.width_pt <= whole.width_pt + 0.01 &&
      at.height_pt <= whole.height_pt + 0.01,
    `${at?.width_pt.toFixed(1)}x${at?.height_pt.toFixed(1)} against ` +
      `${whole?.width_pt.toFixed(1)}x${whole?.height_pt.toFixed(1)}`,
  );
  check(
    CROP_CHECKS[4] ?? "",
    !!whole &&
      !!at &&
      at.left >= -0.01 &&
      at.top >= -0.01 &&
      at.left + at.width_pt <= whole.width_pt + 0.01 &&
      at.top + at.height_pt <= whole.height_pt + 0.01,
    `at (${at?.left.toFixed(1)}, ${at?.top.toFixed(1)}) in ` +
      `${whole?.width_pt.toFixed(1)}x${whole?.height_pt.toFixed(1)}`,
  );

  const cleared = await attempt("page_crop", {
    doc: doc.id,
    page: first.id,
    to: null,
  });
  check(
    CROP_CHECKS[5] ?? "",
    cleared.state?.pages[0]?.crop === undefined,
    cleared.error || `crop ${JSON.stringify(cleared.state?.pages[0]?.crop)}`,
  );

  // The new command, and then the differential worth having: `page_geometry`
  // places a crop box on screen and `page_crop_box` takes a rectangle on screen
  // back to a crop box, through two rotation tables (`to_device` and
  // `from_device`) that a unit test can only exercise on turns it invents. Here
  // the turn is the corpus's own --- which on `rotated.pdf` is not zero, and on
  // every other corpus is the case that cannot tell a right table from a wrong
  // one.
  const dragged: [number, number, number, number] | null = at
    ? [at.left, at.top, at.left + at.width_pt, at.top + at.height_pt]
    : null;
  let mapped: [number, number, number, number] | null = null;
  let mappedWhy = "";
  if (dragged) {
    try {
      mapped = await invoke<[number, number, number, number]>("page_crop_box", {
        doc: doc.id,
        page,
        rect: dragged,
      });
    } catch (e) {
      mappedWhy = String(e);
    }
  }
  check(
    CROP_CHECKS[7] ?? "",
    mapped !== null,
    mappedWhy || `sent ${JSON.stringify(dragged)}, got ${JSON.stringify(mapped)}`,
  );
  check(
    CROP_CHECKS[8] ?? "",
    !!mapped && mapped.every((value, index) => Math.abs(value - (found?.[index] ?? NaN)) < 0.05),
    `placed ${JSON.stringify(dragged)}, back ${JSON.stringify(mapped)}, ` +
      `started as ${JSON.stringify(found)}`,
  );

  // The bound, and it is the half a round trip structurally cannot see: a round
  // trip is a composition, so it agrees with itself about a rectangle that never
  // left the page. A reader's drag routinely does leave it --- one flick of the
  // wrist at the edge --- and the answer has to stay inside the file's own box.
  const past = await invoke<[number, number, number, number]>("page_crop_box", {
    doc: doc.id,
    page,
    rect: [-4000, -4000, 8000, 8000],
  }).catch(() => null);
  check(
    CROP_CHECKS[9] ?? "",
    !!past &&
      !!whole &&
      past[2] - past[0] <= Math.max(whole.width_pt, whole.height_pt) + 0.05 &&
      past[3] - past[1] <= Math.max(whole.width_pt, whole.height_pt) + 0.05 &&
      past[2] > past[0] &&
      past[3] > past[1],
    `${JSON.stringify(past)} against a page of ` +
      `${whole?.width_pt.toFixed(1)}x${whole?.height_pt.toFixed(1)}`,
  );

  // The control: corners the wrong way round enclose nothing, and the model has
  // to say so rather than accept a page that renders as nothing.
  const refused = await attempt("page_crop", {
    doc: doc.id,
    page: first.id,
    to: [found[2], found[3], found[0], found[1]],
  });
  check(
    CROP_CHECKS[6] ?? "",
    refused.state === null && refused.error.includes("encloses no area"),
    `state ${refused.state === null ? "refused" : "accepted"}, said "${refused.error}"`,
  );
}

/**
 * The three mark commands, driven against the real backend.
 *
 * The one thing here that no other check can say: **that the commands are
 * registered**. Every layer below is tested somewhere --- the model in
 * `docmodel.rs`, the boundary in `edits.rs`, the file in `annot-probe`, and the
 * frontend's call shape in `edits.test.ts` against a mock --- and all of it
 * passes with a `generate_handler!` list that forgot a name. The reader finds
 * that out by clicking a highlight and being told the command does not exist.
 *
 * The refusal at the end is the control: the three answers above it could come
 * from a backend that accepts anything, and a note on the mark that was just
 * removed has to come back as the diagnosis the model names, in the words a
 * reader would be shown.
 */
export async function markCommandChecks(doc: DocumentInfo): Promise<void> {
  const opened = await attempt("edit_state", { doc: doc.id });
  const page = opened.state?.pages[0]?.id;
  if (page === undefined) {
    for (const name of MARK_COMMAND_CHECKS) {
      skip(name, opened.error || "the model reports no pages");
    }
    return;
  }

  const made = await attempt("annot_mark", {
    doc: doc.id,
    mark: {
      kind: "highlight",
      page,
      quads: [72, 100, 300, 118],
      color: [1, 0.9, 0.2],
      author: "",
      note: "",
      lines: [],
    },
  });
  const mark = made.state?.marks[0]?.id;
  check(
    MARK_COMMAND_CHECKS[0] ?? "",
    made.state?.marks.length === 1 && mark !== undefined && made.state.dirty,
    made.error
      ? preview(made.error)
      : `${made.state?.marks.length} mark(s), dirty=${made.state?.dirty}`,
  );
  if (mark === undefined) {
    for (const name of MARK_COMMAND_CHECKS.slice(1)) {
      skip(name, "the highlight was not taken, so there is no mark to name");
    }
    return;
  }

  // Each kind through the same command, and read back off the state reply. The
  // kind is the one field that crosses the boundary in both directions, so a
  // name the model does not know is a deserialisation failure here rather than
  // a mark quietly written as something else -- and a `MarkKind` that forgot to
  // serialise the variant would report every mark as the same kind, which the
  // set comparison below sees and a per-kind assertion would not.
  // All four beside the highlight the block above already made, so the set
  // comparison covers every variant the model can be sent. The two the reader
  // *places* rather than selects are here for the same reason as the two it
  // selects: `MarkKind` is closed at the boundary, so a name the model does not
  // know is a deserialisation failure rather than a mark written as something
  // else --- and it is exactly the kinds added last that a `#[serde(rename)]`
  // slip would reach.
  const kinds = ["underline", "strikeout", "note", "square"] as const;
  const back: string[] = [];
  let kindsError = "";
  for (const kind of kinds) {
    const one = await attempt("annot_mark", {
      doc: doc.id,
      mark: {
        kind,
        page,
        quads: [72, 140, 300, 158],
        color: [0.85, 0.15, 0.15],
        author: "",
        note: "",
        lines: [],
      },
    });
    kindsError ||= one.error;
    const last = one.state?.marks[one.state.marks.length - 1];
    back.push(String(last?.kind ?? "none"));
    if (last?.id !== undefined) await attempt("annot_remove", { doc: doc.id, mark: last.id, sweep: 0 });
  }
  check(
    MARK_COMMAND_CHECKS[1] ?? "",
    back.join(",") === kinds.join(","),
    kindsError ? preview(kindsError) : `the model reported ${back.join(", ") || "nothing"}`,
  );

  const noted = await attempt("annot_note", {
    doc: doc.id,
    mark,
    note: "typed through the command",
    lines: [],
  });
  check(
    MARK_COMMAND_CHECKS[2] ?? "",
    noted.state?.marks[0]?.note === "typed through the command",
    noted.error
      ? preview(noted.error)
      : `the model says ${JSON.stringify(noted.state?.marks[0]?.note ?? null)}`,
  );

  const gone = await attempt("annot_remove", { doc: doc.id, mark, sweep: 0 });
  check(
    MARK_COMMAND_CHECKS[3] ?? "",
    gone.state?.marks.length === 0,
    gone.error ? preview(gone.error) : `${gone.state?.marks.length} mark(s) left`,
  );

  const after = await attempt("annot_note", {
    doc: doc.id,
    mark,
    note: "after it went",
    lines: [],
  });
  check(
    MARK_COMMAND_CHECKS[4] ?? "",
    after.error.includes("already been removed"),
    after.error ? preview(after.error) : "it accepted a note on a mark that is gone",
  );
}

/**
 * The print command, up to but not including the panel.
 *
 * Everything about printing that a test can reach is in Rust and covered there.
 * What is **not** covered anywhere else is the wiring: a mistyped command name
 * or a parameter Tauri cannot deserialise compiles perfectly and fails the first
 * time somebody presses Cmd-P. Nothing in the gate would notice, because the
 * two sides never meet until then.
 *
 * So these ask for jobs the backend must *refuse*, and assert on the reason it
 * gives. A refusal proves the command exists, the arguments arrived, and
 * `print::build` ran --- and stops before anything platform-specific, which is
 * the half that would need a person and a sheet of paper.
 */
export async function printChecks(path: string, doc: DocumentInfo): Promise<void> {
  const print = async (
    pages: number[] | null,
    turns: number,
  ): Promise<string> => {
    try {
      await invoke("print_document", { path, pages, turns });
      return "";
    } catch (e) {
      // `describeThrown`, not `String(e)`, and that one word was the whole
      // defect. `print_document` returns `Result<(), save::Refusal>`, so a
      // refusal crosses as an **object** carrying `message` and `changed` ---
      // and `String` renders it `[object Object]`, which contains none of the
      // text the three checks below look for. Both of the assertions went red
      // the day the print refusal grew a shape, and the harness is a step of
      // `BUILD.md` rather than a gate, so nothing said so until the release
      // checklist ran it.
      //
      // The third check passed throughout, and it is the one to look at: it
      // asks only that the answer is non-empty and does *not* mention a missing
      // page, and `[object Object]` satisfies both. One defect, two checks red
      // and one green for the wrong reason.
      return describeThrown(e);
    }
  };

  // A page past the end. Refused by `resolve`, before any platform code.
  const beyond = await print([doc.page_count + 1], 0);
  check(
    "the print command refuses a page the document does not have",
    beyond.includes("is not in this document"),
    beyond ? preview(beyond) : "it accepted the job",
  );

  // The control, and the half that actually pins the wiring: the message above
  // could be produced by a backend that refuses everything. An empty selection
  // is refused by a *different* branch, so the two together show the argument
  // was read rather than that some error was reached.
  const empty = await print([], 0);
  check(
    "the print command refuses an empty selection",
    empty.includes("no pages selected"),
    empty ? preview(empty) : "it accepted the job",
  );

  // And a turn count is a `u8` in Rust: sending a number outside it proves the
  // parameter is typed rather than ignored. `resolve` never runs here.
  const turned = await print(null, 4096);
  check(
    "the print command's rotation is a typed parameter",
    turned.length > 0 && !turned.includes("is not in this document"),
    turned ? preview(turned) : "it accepted 4096 quarter-turns",
  );

  await printPreviewChecks(path, doc);
}

/**
 * The print preview's commands, short of the dialog: a job of page 1 turned a
 * quarter, one sheet drawn, then closed. The turn is what makes the sizes a
 * reading of the *job* rather than of the open document: the sheet comes back
 * with page 1's width and height exchanged, and its picture with them.
 */
async function printPreviewChecks(path: string, doc: DocumentInfo): Promise<void> {
  const page = doc.pages[0];
  let job: number | null = null;
  try {
    const prepared = await call("print_prepare", { path, pages: [1], turns: 1 });
    job = prepared.job;
    const sheet = prepared.pages[0];
    const turned =
      prepared.pages.length === 1 &&
      page !== undefined &&
      sheet !== undefined &&
      Math.abs(sheet.width_pt - page.height_pt) < 0.5 &&
      Math.abs(sheet.height_pt - page.width_pt) < 0.5;
    check(
      "the print preview reads the job's sheets, turned as they print",
      turned,
      `${prepared.pages.length} sheet(s), first ${sheet?.width_pt}x${sheet?.height_pt} for page ${page?.width_pt}x${page?.height_pt}`,
    );

    const url = await call("print_preview_page", { job, index: 0, width: 200 });
    const size = pictureSize(url);
    const wanted = sheet ? Math.round((200 * sheet.height_pt) / sheet.width_pt) : -1;
    check(
      "the print preview draws a sheet at the width asked for",
      size !== null && size.width === 200 && Math.abs(size.height - wanted) <= 1,
      size ? `${size.width}x${size.height}, wanted 200x${wanted}` : preview(url),
    );
  } catch (e) {
    check("the print preview reads the job's sheets, turned as they print", false, preview(describeThrown(e)));
  }
  if (job === null) return;
  await call("print_discard", { job });
  let after = "";
  try {
    await call("print_preview_page", { job, index: 0, width: 200 });
  } catch (e) {
    after = describeThrown(e);
  }
  check(
    "a closed print preview draws nothing more",
    after.includes("has been closed"),
    after ? preview(after) : "it drew a sheet of a closed preview",
  );
}

/**
 * The pixel size of a PNG `data:` URL, read from its header, or `null` when it
 * is not one. Read rather than loaded into an image, which would be a sink.
 */
function pictureSize(url: string): { width: number; height: number } | null {
  const prefix = "data:image/png;base64,";
  if (!url.startsWith(prefix)) return null;
  let head: string;
  try {
    // The signature and the IHDR chunk's width and height: 24 bytes.
    head = atob(url.slice(prefix.length, prefix.length + 32));
  } catch {
    return null;
  }
  if (head.length < 24 || head.slice(1, 4) !== "PNG" || head.slice(12, 16) !== "IHDR") return null;
  const word = (at: number) =>
    ((head.charCodeAt(at) << 24) | (head.charCodeAt(at + 1) << 16) | (head.charCodeAt(at + 2) << 8) | head.charCodeAt(at + 3)) >>> 0;
  return { width: word(16), height: word(20) };
}

/** What this phase reports, so a run that cannot start still prints the names. */
const VALIDATION_CHECK = {
  unsigned: "validation data is refused for a document with no signature",
  sameFile: "validation data is refused under the original's own name",
  address: "validation data is refused a timestamp authority that is not http",
  // `viewer_check.py` reads this one by name: with `--signed` it must have run.
  untrusted: "nothing is fetched for a signer this computer does not trust",
} as const;

/**
 * A timestamp authority nobody is listening at: the discard port, on this
 * machine. Never an outside host. Every refusal below is made before anybody
 * is asked, so nothing is sent here either; it is what a regression that went
 * as far as the archive timestamp would reach, and not the internet.
 *
 * **It is not how a skipped trust rule shows.** The signed fixture's signer
 * names no address for revocation data, so a build that vouched for everybody
 * stops one step later, at *does not say where its revocation data is
 * published*, having asked nobody (measured 2026-10-10, `BUILD.md`). That no
 * request is made for an untrusted signer whose certificate *does* name an
 * address is `longterm::existing`'s tests', whose authorities record what
 * they were asked.
 */
const NOBODY_LISTENING = "http://127.0.0.1:9/";

/**
 * Adding long-term validation data, up to but not including anybody's answer.
 *
 * {@link printChecks}' arrangement, for its reason. The steps are Rust's and
 * tested there, and the sequence before the command is `validationdata.ts`'s
 * and tested there; what neither reaches is whether `add_validation_data`
 * exists under that name, takes these four arguments, and hands the document
 * to a worker that answers. So these ask for what the backend must *refuse*,
 * and assert on the reason it gives. The arguments go through `call`, which
 * types them against the table `App.svelte`'s own call is typed against.
 *
 * Four refusals from four branches, because one could come from a backend that
 * refuses everything: the worker's reading of a document with no signature,
 * the output named as the original, an address judged in the command, and a
 * signer the operating system does not vouch for.
 *
 * **The sentences are Rust's, and nothing here reads them from it**: each is
 * matched by a fragment, as the print refusals are. `longterm::existing`'s
 * `Refusal::sentence` has the first and the last, `commands::validation` the
 * second and the *nothing was written* every `Stopped::Refused` ends in, and
 * `tsa::authority` the third.
 *
 * **Not reached: the data gathered and the copy written.** That needs a signer
 * the operating system trusts, and no check adds a root to a key store. What a
 * worker builds and reads back is `commands::validation`'s own tests, over
 * authorities of their own.
 *
 * Whether anything was written is not something a page can see.
 * `viewer_check.py` looks beside the scratch path after the run, and holds the
 * signed document's bytes against the fixture's.
 */
export async function validationDataChecks(path: string): Promise<void> {
  const scratch = await invoke<string | null>("viewercheck_scratch");
  if (!scratch) {
    for (const name of Object.values(VALIDATION_CHECK)) {
      skip(
        name,
        "no scratch path was bound, so there is no name to refuse a copy at",
      );
    }
    return;
  }
  // Names of this run's own, which `viewer_check.py` finds by the same stem.
  const beside = (what: string): string =>
    scratch.replace(/\.pdf$/, `-${what}.pdf`);

  /** The refusal, or nothing when the data was added. */
  const add = async (
    doc: number,
    source: string,
    out: string,
    timestamp: string,
  ): Promise<string> => {
    try {
      await call("add_validation_data", { doc, source, path: out, timestamp });
      return "";
    } catch (e) {
      return describeThrown(e);
    }
  };
  // A refusal that is the wrong one is printed whole: which sentence came
  // instead is the finding, and forty characters of it do not say.
  const said = (ok: boolean, refusal: string): string =>
    !refusal
      ? "it added the data"
      : ok
        ? preview(refusal)
        : refusal.replace(/\s+/g, " ").trim();
  const opened = (file: string): Promise<DocumentInfo | null> =>
    invoke<DocumentInfo>("open_document", { path: file }).catch(() => null);
  const closed = (doc: number): Promise<void> =>
    invoke<void>("close_document", { doc }).catch(() => {});

  // Opened here rather than reusing the document under test: the command
  // refuses a document with unsaved edits, and whether the phases above left
  // that one with any is not something this phase should depend on.
  const plain = await opened(path);
  if (!plain) {
    const why = "the document did not open a second time";
    check(VALIDATION_CHECK.unsigned, false, why);
    skip(VALIDATION_CHECK.sameFile, why);
    skip(VALIDATION_CHECK.address, why);
  } else {
    const unsigned = await add(
      plain.id,
      path,
      beside("validation-unsigned"),
      NOBODY_LISTENING,
    );
    const refused =
      unsigned.includes("this document has no signature") &&
      unsigned.includes("nothing was written");
    check(VALIDATION_CHECK.unsigned, refused, said(refused, unsigned));

    // Refused by the command itself, before a worker is asked: with the check
    // above, `source` and `path` were both read.
    const same = await add(plain.id, path, path, NOBODY_LISTENING);
    const named = same.includes("a name other than the original's");
    check(VALIDATION_CHECK.sameFile, named, said(named, same));

    // And `timestamp` is judged here, not taken from the page as it comes.
    const address = await add(
      plain.id,
      path,
      beside("validation-address"),
      "ftp://127.0.0.1/",
    );
    const judged = address.includes("only http and https");
    check(VALIDATION_CHECK.address, judged, said(judged, address));
    await closed(plain.id);
  }

  // A document somebody signed with a certificate they issued to themselves,
  // which `viewer_check.py --signed` copies here. The refusal is the trust
  // rule's: reached only once a worker has read the signature and called it
  // intact, and before any address a certificate names is asked.
  const signedPath = beside("signed");
  const signed = await opened(signedPath);
  if (!signed) {
    skip(
      VALIDATION_CHECK.untrusted,
      "no signed document was put beside the scratch path",
    );
    return;
  }
  const untrusted = await add(
    signed.id,
    signedPath,
    beside("validation-untrusted"),
    NOBODY_LISTENING,
  );
  const unvouched =
    untrusted.includes("is not trusted by this computer") &&
    untrusted.includes("will not fetch revocation data") &&
    untrusted.includes("nothing was written");
  check(VALIDATION_CHECK.untrusted, unvouched, said(unvouched, untrusted));
  await closed(signed.id);
}
