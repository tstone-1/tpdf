/**
 * Everything the application can do, as data, plus the ranking that finds one.
 *
 * `docs/PLAN.md` §8 puts the command palette in Phase 1 and calls it "the
 * thesis, not a garnish": the stated pain with Acrobat is that its capability is
 * real and unreachable. A palette only helps if *every* command is in it, which
 * means commands have to be registered somewhere rather than living as branches
 * of a `keydown` handler --- that handler is already fifteen branches long, and
 * a feature added to it is a feature the palette cannot see.
 *
 * So this is the registry, and the keyboard handler becomes one of its callers
 * rather than the place commands are defined.
 *
 * ## Ranking
 *
 * Subsequence matching, the way a code editor does it: every character of the
 * query must appear in the title in order, but not adjacently, so `zi` finds
 * "Zoom in". Scoring prefers, in order, matches at the start of a word, matches
 * that run consecutively, and matches early in the title --- because "fw" should
 * find "Fit width" ahead of "Find, forward".
 *
 * Positions are returned, not just a score. The palette highlights the matched
 * characters, and a highlight that disagreed with the ranking would be worse
 * than none.
 */

/** What every command has, whether or not it takes an argument. */
interface CommandBase {
  /** Stable identity, e.g. `view.zoomIn`. Used for recents, never displayed. */
  id: string;
  /** What the palette shows. */
  title: string;
  /**
   * The keybinding, as a reader would type it.
   *
   * Displayed by the palette so it teaches shortcuts as a side effect --- which
   * is the reason a palette makes a keyboard-first application *more* keyboard
   * driven rather than less. Derived from the binding by `keys.ts`'s `label`,
   * so it cannot come to disagree with the handler.
   */
  keys?: string;
  /** Whether it can run right now. A command with no document is not offered. */
  enabled?: () => boolean;
}

/**
 * A value the reader types before the command can run.
 *
 * "Go to page" is the case that forced this: a 775-page document has no other
 * way to reach page 400, and a command with no argument cannot express it. The
 * palette handles the typing, so a command declares what it needs rather than
 * building a dialog.
 *
 * {@link problem} and {@link preview} are separate on purpose. A palette that
 * only said "invalid" would be no better than an input that rejects; saying
 * *what* is wrong, and showing what will happen when it is right, is the same
 * argument that puts the keybinding next to every command.
 */
export interface CommandArgument {
  /** Shown in the palette's input while the value is being typed. */
  placeholder: string;
  /**
   * Why this value cannot be used, or null if it can.
   *
   * Called on every keystroke, so it must be cheap and must not act. An empty
   * string is a value like any other and must be handled --- the palette asks
   * before anything is typed.
   */
  problem: (raw: string) => string | null;
  /** What running it will do, shown once {@link problem} is happy. */
  preview: (raw: string) => string;
  run: (raw: string) => void | Promise<void>;
}

/**
 * One thing the application can do.
 *
 * A union rather than one shape with two optional halves, so a command cannot
 * be declared with neither `run` nor `argument` --- which would type-check,
 * list in the palette, and do nothing when chosen.
 */
export type Command =
  | (CommandBase & { run: () => void | Promise<void>; argument?: undefined })
  | (CommandBase & { argument: CommandArgument; run?: undefined });

/** A command matched against a query, with where it matched. */
export interface Ranked {
  command: Command;
  score: number;
  /** Indices into `command.title` that the query matched, ascending. */
  positions: number[];
}

/**
 * Weights. Only their *order* is meaningful --- a score is never displayed and
 * can go negative on a match that starts late in a long title, which is fine
 * because nothing compares one against a threshold.
 */
/** Score for a match at the start of a word, which is what people type. */
const WORD_START = 12;
/** Score for a match immediately after the previous one. */
const CONSECUTIVE = 8;
/** Score for any match at all, so a subsequence match never scores zero. */
const BASE = 1;
/** Penalty per character skipped before the first match. */
const LEADING_SKIP = 1;

/**
 * Whether the character at `index` starts a word in `title`.
 *
 * The `index === 0` case cannot be isolated by a score comparison --- there is
 * no title where position 0 is *not* a word start to compare against --- so no
 * test asserts it directly. It is pinned instead by the ordering test that
 * recency must not beat a better match, which fails without it. Recorded
 * because "no test names this line" and "this line is untested" are different
 * things, and only the second is a problem.
 */
function startsWord(title: string, index: number): boolean {
  if (index === 0) return true;
  return !/[A-Za-z0-9]/.test(title[index - 1] ?? "");
  // A camel-case clause was here too --- a capital after a lower-case letter,
  // for titles like "zoomIn". Every title is a human-readable label in sentence
  // case, so no mutation of that clause could fail a test, which is the
  // signature of defence nothing pins. Deleted, as `Selection.isEmpty` and
  // `queue.rs`'s zero guard were before it.
}

/**
 * Matches `query` against `title` as a subsequence, greedily left to right.
 *
 * Greedy rather than optimal: finding the best-scoring set of positions is a
 * search, and for titles of a few words the greedy pass differs only on inputs
 * nobody types. What it must not do is *miss* a match that exists, and it does
 * not --- greedy subsequence matching is complete, only its scoring is
 * approximate.
 */
export function fuzzyMatch(
  query: string,
  title: string,
): { score: number; positions: number[] } | null {
  const needle = query.toLowerCase();
  const hay = title.toLowerCase();
  if (needle.length === 0) return { score: 0, positions: [] };

  const positions: number[] = [];
  let score = 0;
  let at = 0;

  for (const wanted of needle) {
    const found = hay.indexOf(wanted, at);
    if (found < 0) return null;

    score += BASE;
    if (startsWord(title, found)) score += WORD_START;
    if (positions.length > 0 && found === positions[positions.length - 1]! + 1) {
      score += CONSECUTIVE;
    }
    if (positions.length === 0) score -= found * LEADING_SKIP;

    positions.push(found);
    at = found + 1;
  }

  return { score, positions };
}

/**
 * The commands matching `query`, best first.
 *
 * With an empty query this is the whole enabled list in registration order,
 * with recents lifted to the front --- a palette that opens on nothing is a
 * palette that has to be searched before it can be used.
 */
export function rank(
  query: string,
  commands: readonly Command[],
  recents: readonly string[] = [],
): Ranked[] {
  const enabled = commands.filter((command) => command.enabled?.() ?? true);
  const trimmed = query.trim();

  if (trimmed.length === 0) {
    const recent = recents
      .map((id) => enabled.find((command) => command.id === id))
      .filter((command): command is Command => command !== undefined);
    const rest = enabled.filter((command) => !recents.includes(command.id));
    return [...recent, ...rest].map((command) => ({
      command,
      score: 0,
      positions: [],
    }));
  }

  const matched: Ranked[] = [];
  for (const command of enabled) {
    const hit = fuzzyMatch(trimmed, command.title);
    if (!hit) continue;
    // A recent command wins ties, and only ties: the bonus is smaller than one
    // word-start match, so typing something specific always beats history.
    const recency = recents.indexOf(command.id);
    const bonus = recency < 0 ? 0 : Math.max(1, WORD_START / 2 - recency);
    matched.push({
      command,
      score: hit.score + bonus,
      positions: hit.positions,
    });
  }

  // Stable by construction: equal scores keep registration order, which is
  // grouped by area, so the list does not reshuffle as someone types.
  return matched.sort((a, b) => b.score - a.score);
}

/** The commands, and which ones were used recently. */
export class CommandRegistry {
  private readonly commands: Command[] = [];
  private readonly recent: string[] = [];

  /** How many commands are remembered as recent. */
  private static readonly RECENTS = 5;

  /** Adds commands, in the order they should appear with an empty query. */
  register(...commands: Command[]): void {
    this.commands.push(...commands);
  }

  /**
   * Swaps every command whose id starts with `prefix` for `commands`.
   *
   * The registry is otherwise append-only, which suits a command list that is
   * decided at startup. Recent documents are not: the list changes whenever one
   * is opened, and re-registering without removing would leave the palette
   * offering yesterday's ordering alongside today's.
   *
   * By prefix rather than by a list of ids, because the *old* ids are exactly
   * what a caller replacing a whole group does not have --- it knows what the
   * group is now, not what it was.
   */
  replace(prefix: string, commands: Command[]): void {
    for (let i = this.commands.length - 1; i >= 0; i--) {
      if (this.commands[i]?.id.startsWith(prefix)) this.commands.splice(i, 1);
    }
    this.commands.push(...commands);
    // A removed command's id can still be sitting in the recents, where it would
    // rank a command that no longer exists --- harmless today, since `rank`
    // looks the id up and finds nothing, and wrong the moment an id is reused
    // for a different document, which is precisely what these ids do.
    for (let i = this.recent.length - 1; i >= 0; i--) {
      const id = this.recent[i];
      if (id?.startsWith(prefix)) this.recent.splice(i, 1);
    }
  }

  /** Every registered command, enabled or not. */
  all(): readonly Command[] {
    return this.commands;
  }

  /** Ids of recently run commands, most recent first. */
  recents(): readonly string[] {
    return this.recent;
  }

  /** The commands matching `query`, best first. */
  search(query: string): Ranked[] {
    return rank(query, this.commands, this.recent);
  }

  /** A command by id, or undefined. */
  find(id: string): Command | undefined {
    return this.commands.find((c) => c.id === id);
  }

  /**
   * Runs a command and records it as recent.
   *
   * A disabled command is not run. The palette does not offer one, but a
   * keybinding can still reach a command whose document has just been closed,
   * and "the shortcut did nothing" is a better outcome than a stack trace.
   *
   * A command that takes an argument is refused without one, and refused again
   * if the value it is given is one its own {@link CommandArgument.problem}
   * rejects. The palette will not offer a bad value, but this is what makes the
   * registry safe to call from anywhere --- and it is checked here rather than
   * trusted to the caller, because the caller that skips the check is the one
   * that will exist later.
   */
  run(id: string, argument?: string): boolean {
    const command = this.find(id);
    if (!command || !(command.enabled?.() ?? true)) return false;

    if (command.argument) {
      if (argument === undefined) return false;
      if (command.argument.problem(argument) !== null) return false;
    } else if (argument !== undefined) {
      // Refused rather than ignored: a caller passing a value to a command that
      // takes none has misunderstood something, and silently dropping it hides
      // that until someone wonders why the value had no effect.
      return false;
    }

    const already = this.recent.indexOf(id);
    if (already >= 0) this.recent.splice(already, 1);
    this.recent.unshift(id);
    this.recent.length = Math.min(this.recent.length, CommandRegistry.RECENTS);

    if (command.argument) void command.argument.run(argument as string);
    else void command.run();
    return true;
  }
}
