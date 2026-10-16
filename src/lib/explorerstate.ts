/**
 * *Show PDFs in Explorer with tpdf* and *Stop showing PDFs in Explorer with
 * tpdf*: whether each has anything to do, the question asked before another
 * program's handler is replaced, and when to ask the backend again.
 *
 * `App.svelte` keeps the two `invoke`s; the rest is here, so it has tests.
 *
 * As in `clitoolstate.ts`, what is held is a hint and never the decision:
 * `explorer_set` reads the registry again when it runs. It differs in one way.
 * `null` from `explorer_state` means "not Windows", where both commands are
 * greyed for good, while a read that *failed* is "not known" and leaves both
 * offered, so running one says what is there.
 */

/** `explorer::Owner` in `src-tauri/src/explorer.rs`. */
export type ExplorerOwner = { kind: "nobody" } | { kind: "tpdf" } | { kind: "other"; name: string };

/** `explorer::Handlers` in `src-tauri/src/explorer.rs`. */
export interface ExplorerState {
  thumbnail: ExplorerOwner;
  preview: ExplorerOwner;
}

/** What the module needs from the application. */
export interface ExplorerDeps {
  /** `explorer_state`: `null` off Windows. */
  read(): Promise<ExplorerState | null>;
  /** `explorer_set`: the sentence to show; a refusal rejects. */
  apply(on: boolean, replace: boolean): Promise<string>;
  /** Asks the reader a yes-or-no question. */
  confirm(question: string): Promise<boolean>;
  /** Show a sentence to the reader. */
  say(text: string): void;
  /** What is offered may have moved: re-read every command's guard. */
  changed(): void;
}

/**
 * The question to ask before turning tpdf's handlers on, or `null` when
 * nothing of another program's would be replaced.
 */
export function replaceQuestion(state: ExplorerState): string | null {
  const others: string[] = [];
  if (state.thumbnail.kind === "other") others.push(`PDF thumbnails come from ${state.thumbnail.name}`);
  if (state.preview.kind === "other") others.push(`the preview pane comes from ${state.preview.name}`);
  if (!others.length) return null;
  const said = others.join(", and ");
  return `In Explorer, ${said}. Replace ${others.length > 1 ? "them" : "it"} with tpdf? ` +
    "Turning tpdf's off later puts back what it replaced.";
}

export class ExplorerIntegration {
  /** `undefined` not known, `null` not Windows, else the last answer. */
  private known: ExplorerState | null | undefined = undefined;
  /** Counts the questions, so an answer can tell whether it is the newest. */
  private asked = 0;

  constructor(private readonly deps: ExplorerDeps) {}

  /** Whether turning on (`on`) or off is offered. */
  offered(on: boolean): boolean {
    const state = this.known;
    if (state === null) return false;
    if (state === undefined) return true;
    const slots = [state.thumbnail, state.preview];
    return on ? slots.some((s) => s.kind !== "tpdf") : slots.some((s) => s.kind === "tpdf");
  }

  /** Asks the backend again. Never rejects. */
  async refresh(): Promise<void> {
    const mine = ++this.asked;
    let next: ExplorerState | null | undefined;
    try {
      next = await this.deps.read();
    } catch {
      next = undefined;
    }
    if (mine !== this.asked) return;
    this.known = next;
    this.deps.changed();
  }

  /**
   * Turns tpdf's handlers on or off and shows what the backend said. Before
   * turning on, a handler of another program's is replaced only when the
   * reader answers yes; the free slots are taken either way. The state is
   * read fresh for the question rather than taken from the hint.
   */
  async run(on: boolean): Promise<void> {
    try {
      let replace = false;
      if (on) {
        const state = await this.deps.read();
        const question = state ? replaceQuestion(state) : null;
        if (question) replace = await this.deps.confirm(question);
      }
      this.deps.say(await this.deps.apply(on, replace));
    } catch (why) {
      this.deps.say(String(why));
    }
    await this.refresh();
  }
}
