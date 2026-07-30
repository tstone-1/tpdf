/**
 * Keyboard bindings, as data.
 *
 * The palette advertises a shortcut next to every command it lists, and until
 * this existed those labels were hand-written strings sitting twenty lines away
 * from the handlers that implement them, with nothing checking the two agreed.
 * `App.svelte` said so in a comment and called it "a real gap and a small one".
 * It is small in consequence — a wrong label teaches a wrong shortcut, it does
 * not break a command — and it is exactly the kind of thing that rots, because
 * nothing goes red when it does.
 *
 * The fix is not a test that compares two lists. It is having one list: a
 * binding declares its modifiers and its keys, {@link label} renders the string
 * the palette shows *from those modifiers*, and {@link matches} decides whether
 * an event is that binding. A label can no longer disagree with its handler,
 * because it is derived from it.
 *
 * Only the bindings the palette advertises live here. Scrolling keys — the
 * arrows, Page Up/Down, space — are not commands, are not listed, and stay where
 * they are handled.
 */

/** One advertised binding. */
export interface Binding {
  /**
   * Keys that trigger it, as `KeyboardEvent.key` values.
   *
   * A list rather than one value because `key` carries the *shifted* form and
   * varies by layout: ⌘+ arrives as `+` on one keyboard and `=` on another, and
   * a chord held with Shift reports `I` on a US layout and `i` on some others.
   * Matching one spelling makes a shortcut that works on one keyboard.
   */
  keys: string[];
  /** Whether the platform accelerator (⌘ on macOS, Ctrl elsewhere) is held. */
  accel?: boolean;
  /** Whether Shift is held. Absent means "must not be". */
  shift?: boolean;
  /**
   * Whether Option/Alt is held. Absent means "must not be".
   *
   * Added for ⌥⌘G, which is what this platform's own viewer uses for "Go to
   * page". Note {@link matches} previously did not look at `altKey` at all, so
   * every binding here matched with Option held as well as without --- ⌥⌘F
   * opened find, and ⌥⌘G ran find-next. That is the same both-directions bug
   * the Shift check exists to prevent, and it had to be fixed before this
   * binding could exist at all: ⌥⌘G would otherwise have been find-next too,
   * and whichever branch came first would have won.
   */
  alt?: boolean;
  /**
   * What the palette shows, when the modifiers do not say it.
   *
   * Only for bindings whose key is not its own name: `Escape` reads as `Esc`,
   * and `\` needs no dressing up. Everything else is rendered by {@link label}.
   */
  shown?: string;
}

/**
 * Every binding the palette advertises, by command id.
 *
 * The ids match `CommandRegistry` registrations in `App.svelte`; a command with
 * no entry here simply shows no shortcut, which is what "Show outline" and
 * "Show page thumbnails" do.
 */
export const BINDINGS = {
  "file.open": { keys: ["o"], accel: true },
  "file.print": { keys: ["p"], accel: true },
  "find.open": { keys: ["f"], accel: true },
  "find.next": { keys: ["g", "G"], accel: true },
  "find.previous": { keys: ["g", "G"], accel: true, shift: true },
  "view.zoomIn": { keys: ["+", "="], accel: true, shown: "+" },
  "view.zoomOut": { keys: ["-"], accel: true, shown: "−" },
  "view.fitWidth": { keys: ["0"], accel: true },
  "view.rotateClockwise": { keys: ["r", "R"], accel: true },
  "view.rotateCounterClockwise": { keys: ["l", "L"], accel: true },
  "view.toggleSidebar": { keys: ["\\"], accel: true },
  "view.invertPages": { keys: ["i", "I"], accel: true, shift: true },
  "nav.nextPage": { keys: ["n"] },
  "nav.previousPage": { keys: ["p"] },
  "nav.firstPage": { keys: ["Home"] },
  "nav.lastPage": { keys: ["End"] },
  "nav.goToPage": { keys: ["g", "G"], accel: true, alt: true },
  "edit.selectAll": { keys: ["a"], accel: true },
  "edit.copy": { keys: ["c"], accel: true },
  "edit.clearSelection": { keys: ["Escape"], shown: "Esc" },
} as const satisfies Record<string, Binding>;

/** A command id that has an advertised binding. */
export type BoundCommand = keyof typeof BINDINGS;

/**
 * The string the palette shows for a binding.
 *
 * Rendered from the modifiers rather than written beside them, so ⇧⌘G cannot
 * come to mean something the handler does not accept. Order follows the macOS
 * convention, which puts Shift before Command.
 */
export function label(id: BoundCommand): string {
  const binding: Binding = BINDINGS[id];
  const modified = Boolean(binding.accel || binding.shift || binding.alt);
  const first = binding.keys[0] ?? "";
  // A letter in a chord is capitalised and a bare one is not, which is what
  // every menu on this platform does: ⌘O, but `n` for the unmodified key.
  const key =
    binding.shown ?? (modified && first.length === 1 ? first.toUpperCase() : first);
  // macOS order: Control, Option, Shift, Command, then the key.
  return `${binding.shift ? "⇧" : ""}${binding.alt ? "⌥" : ""}${binding.accel ? "⌘" : ""}${key}`;
}

/**
 * Whether an event is this binding.
 *
 * `metaKey || ctrlKey` for the accelerator, so the same table serves both
 * platforms. Shift is checked in **both** directions: a binding that does not
 * ask for it must not match an event that holds it, or ⇧⌘G would also fire
 * find-next.
 */
export function matches(id: BoundCommand, event: KeyboardEvent): boolean {
  const binding: Binding = BINDINGS[id];
  const accel = event.metaKey || event.ctrlKey;
  if (accel !== (binding.accel ?? false)) return false;
  if (event.shiftKey !== (binding.shift ?? false)) return false;
  if (event.altKey !== (binding.alt ?? false)) return false;
  return binding.keys.includes(event.key);
}
