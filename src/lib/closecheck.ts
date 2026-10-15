/**
 * Closing several tabs and reopening the last one closed, in the running
 * application: `tabs_check.py --phase closing`.
 *
 * `tabclose.test.ts` holds which tabs each command means. What only the window
 * can show is the join in `App.svelte`: each tab closed through `closeTab`,
 * one after another, and a closed document opened again by its path.
 */

import type { OpenCheckHost } from "./opencheck";
import { settle, type Report } from "./checkreport";

const SETTLE_MS = 20_000;

export async function closingCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const [first, second, third] = expected.split("|");
  if (!first || !second || !third) throw new Error("three fixture paths required");
  const paths = () => host.tabs().map((tab) => tab.path);
  const names = () => paths().map((path) => [first, second, third].indexOf(path) + 1).join(",");
  const until = (want: string) => settle(() => names() === want, SETTLE_MS);

  report.emit("[closing] opening three documents");
  await host.open(first); await host.open(second); await host.open(third);
  await host.idle();
  report.check("three tabs are open", names() === "1,2,3", names());

  const middle = host.tabs()[1]!;
  await host.activate(middle.id);
  await host.idle();
  host.run("file.closeTabsRight");
  report.check("Close tabs to the right closes the one after the middle tab", await until("1,2"), names());

  host.run("file.reopenClosedTab");
  report.check("Reopen closed tab opens it again", await until("1,2,3"), names());

  await host.activate(host.tabs()[1]!.id);
  await host.idle();
  host.run("file.closeTabsLeft");
  report.check("Close tabs to the left closes the one before the middle tab", await until("2,3"), names());

  await host.activate(host.tabs()[0]!.id);
  await host.idle();
  host.run("file.closeOtherTabs");
  report.check("Close other tabs leaves the tab in front", await until("2"), names());

  // Closed in order: the first (to the left), then the third (the others).
  // The third is the one closed last, and the first comes back after it.
  host.run("file.reopenClosedTab");
  report.check("Reopen closed tab brings back the one closed last", await until("2,3"), names());
  host.run("file.reopenClosedTab");
  report.check("and again, the one closed before it", await until("2,3,1"), names());

  // The three copies are first.pdf, second.pdf and third.pdf in one folder,
  // so from the first the next is the second, which is open already: it is
  // shown, and the tab the reader came from is closed.
  report.emit("[closing] stepping through the folder");
  await host.activate(host.tabs().find((tab) => tab.path === first)!.id);
  await host.idle();
  host.run("file.nextInFolder");
  report.check("the next PDF in the folder moves the tab on", await until("2,3"), names());
  // The tab list changes before the close behind it finishes, and a command
  // run while a tab is closing is not offered.
  await host.openings();
  await host.idle();
  host.run("file.previousInFolder");
  const back = await until("3,1");
  const said = document.querySelector('[data-testid="problem"]')?.textContent?.trim() ?? "";
  report.check("and the previous one moves it back", back,
    `${names()}; in front: ${host.path()}${said ? `; said: ${said}` : ""}`);
}
