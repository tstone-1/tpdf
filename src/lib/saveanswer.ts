/**
 * The save panel a signing opens, answered by the checks build.
 *
 * A phase cannot drive a native panel --- `file.redactCopy` is left out of the
 * redaction phase for exactly that reason --- and *Sign document…* asks for its
 * file name with one. Everything before and after the panel is the window's
 * own: the palette, the chooser, the command, the question after a timestamp
 * or long-term data did not come, the message area. So the panel alone is
 * answered, by a path the phase queues, and every other step stays the reader's
 * path (`signingcheck.ts`).
 *
 * `App.svelte` constructs one only when `__TPDF_CHECKS__` is set, so a normal
 * build compiles its panel call to the panel alone and this module out.
 */
export class SaveAnswers {
  private queued: string | null = null;
  private readonly suggestions: string[] = [];

  /** The path the next panel answers with, instead of showing. */
  queue(path: string): void {
    this.queued = path;
  }

  /** Every name a panel was asked to suggest, in order: the phase checks the last. */
  get asked(): readonly string[] {
    return this.suggestions;
  }

  /**
   * The queued path, once, or `panel()` when nothing is queued. The
   * suggestion is recorded either way, because what the window would have
   * offered the reader is part of what the phase checks.
   */
  async ask(suggested: string, panel: () => Promise<string | null>): Promise<string | null> {
    this.suggestions.push(suggested);
    const path = this.queued;
    if (path === null) return panel();
    this.queued = null;
    return path;
  }
}
