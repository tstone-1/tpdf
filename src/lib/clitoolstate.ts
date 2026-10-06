/**
 * Whether *Install command-line tool…* and *Uninstall command-line tool…* have
 * anything to do: the last answer the backend gave, and when to ask again.
 *
 * `App.svelte` keeps the two `invoke`s and the listener; what is known, what
 * that greys and every moment it is asked again are here, so they have tests.
 *
 * **This is a hint and never the decision.** `command_line_tool` reads the
 * filesystem (macOS) or the user's `PATH` (Windows) each time it runs and
 * answers from that; nothing here is passed to it. What this holds only decides
 * whether a command is offered, and it is wrong in the safe direction whenever
 * it can be:
 *
 * - **Not known means offered.** Before the first answer, on a platform with
 *   nothing to read (`null`), and after any read that failed, both commands are
 *   live, and running one says what is there.
 * - **A failed read forgets the answer before it** rather than keeping it, so a
 *   command greyed by an old answer does not stay greyed behind a read that no
 *   longer works.
 * - **A slow read greys nothing new.** The answer before it stands until the
 *   new one arrives, and only the newest question's answer is taken, so two
 *   reads that cross cannot leave the older one in place.
 *
 * It is asked once after the first document is on its way, after either
 * command finishes --- done or refused --- and whenever the window comes back
 * to the front, because the link or the `PATH` can be changed from a terminal
 * while tpdf is open. A greyed command cannot be run to find out it should not
 * be grey, which is why coming back to the window is one of the three.
 */

/** `clitool::ToolState` in `src-tauri/src/clitool.rs`. */
export interface ToolState {
  /** This copy's tool is what a terminal gets: installing has nothing to do. */
  installed: boolean;
  /**
   * Something is at one of the tool's paths --- tpdf's, which removing takes
   * away, or somebody else's, which removing leaves alone and says so. On
   * Windows, the folder is on the `PATH`.
   */
  occupied: boolean;
}

/** What the module needs from the application. */
export interface ToolStateDeps {
  /** `command_line_tool_state`: `null` where there is nothing to read. */
  read(): Promise<ToolState | null>;
  /** `command_line_tool`: the sentence to show; a refusal rejects. */
  apply(install: boolean): Promise<string>;
  /** Show a sentence to the reader. */
  say(text: string): void;
  /** What is offered may have moved: re-read every command's guard. */
  changed(): void;
}

export class CommandLineTool {
  /** The last answer, or `null` for "not known". */
  private known: ToolState | null = null;
  /** Counts the questions, so an answer can tell whether it is the newest. */
  private asked = 0;

  constructor(private readonly deps: ToolStateDeps) {}

  /**
   * Whether the install (`install`) or the uninstall command is offered.
   *
   * Install is withheld only when this copy is installed: a link to another
   * copy of tpdf is repointed by installing, and somebody else's file is
   * explained by it. Uninstall is withheld only when nothing is at either
   * path, for the same reason --- over somebody else's file it removes nothing
   * and says why, which a greyed item could not.
   */
  offered(install: boolean): boolean {
    if (!this.known) return true;
    return install ? !this.known.installed : this.known.occupied;
  }

  /** Asks the backend again. Never rejects. */
  async refresh(): Promise<void> {
    const mine = ++this.asked;
    let next: ToolState | null;
    try {
      next = await this.deps.read();
    } catch {
      next = null;
    }
    if (mine !== this.asked) return;
    this.known = next;
    this.deps.changed();
  }

  /**
   * Runs one of the two commands, shows what the backend said --- a refusal the
   * same way as a success --- and asks again afterwards either way: a cancelled
   * administrator prompt changed nothing, and that is worth knowing too.
   */
  async run(install: boolean): Promise<void> {
    try {
      this.deps.say(await this.deps.apply(install));
    } catch (why) {
      this.deps.say(String(why));
    }
    await this.refresh();
  }
}
