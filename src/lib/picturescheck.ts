/**
 * *New document from pictures*, through the window a reader uses it in.
 *
 * `tests/cli/images.rs` holds the pages against the pictures; what no gate
 * reaches is `App.svelte`: the command being offered with nothing open, the
 * name the save panel suggests, the sentence, the document being opened, and
 * a refusal leaving the window as it was. Both panels are answered here, since
 * no phase can drive a native one.
 *
 * `expected` is `first|second|directory`: two pictures and an empty directory.
 * `scripts/tabs_check.py --phase pictures` makes them and reads the directory
 * afterwards.
 */

import type { OpenCheckHost } from "./opencheck";
import { pause, settle, type Report } from "./checkreport";
import { suggestedName } from "./pictures";

const SETTLE_MS = 20_000;
const WRITE_MS = 30_000;

export async function picturesCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const [first, second, directory] = expected.split("|");
  if (!first || !second || !directory) throw new Error("two pictures and a directory are required");
  const at = (name: string) => `${directory}/${name}`;
  const shown = () => document.querySelector('[data-testid="problem"]')?.textContent ?? "";
  const quiet = async () => {
    if (!(await settle(() => host.viewer()?.idle === true, SETTLE_MS))) throw new Error("the viewer did not settle");
    await pause(100);
  };

  // ---- 1. With nothing open: two pictures, two pages, and the document shown.
  report.check("the control: no document is open", host.path() === "", host.path());
  host.answerPictures([first, second]);
  host.answerSave(at("album.pdf"));
  host.run("file.fromPictures");
  const said = await settle(() => shown().includes("Saved album.pdf"), WRITE_MS);
  report.check("the command runs with no document open and ends on its sentence",
    said && shown().trim() === "Saved album.pdf: 2 pages, one for each picture.", shown().slice(0, 200));
  const suggestions = host.saveSuggestions();
  report.check("the save panel suggests the first picture's name as a PDF",
    suggestions[suggestions.length - 1] === suggestedName(first), suggestions.join(", "));
  await host.idle(); await quiet();
  report.check("the new document is the open one", host.path() === at("album.pdf"), host.path());
  const pages = host.edits()?.state.pages.length ?? 0;
  report.check("and it has a page for each picture", pages === 2, String(pages));

  // ---- 2. A file that is not a picture: refused by name, and nothing changes.
  host.answerPictures([first, at("album.pdf")]);
  host.answerSave(at("refused.pdf"));
  host.run("file.fromPictures");
  const refused = await settle(() => shown().includes("cannot be used"), WRITE_MS);
  report.check("a file that is not a picture is refused by its name",
    refused && shown().includes("album.pdf cannot be used: it is not a PNG or JPEG"), shown().slice(0, 200));
  await host.idle();
  report.check("and the open document is still the one before", host.path() === at("album.pdf"), host.path());

  // ---- 3. No picture chosen: no name is asked for.
  const asked = host.saveSuggestions().length;
  host.answerPictures([]);
  host.run("file.fromPictures");
  await host.idle(); await pause(300);
  report.check("choosing no picture asks for no name", host.saveSuggestions().length === asked,
    `${host.saveSuggestions().length} against ${asked}`);
}
