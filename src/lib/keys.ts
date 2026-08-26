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
  /**
   * The physical key, as a `KeyboardEvent.code`, matched *as well as* {@link keys}.
   *
   * For a chord whose character a keyboard cannot produce without extra
   * modifiers. ⌘\ is the case that forced it: on the German layout `\` is
   * **⌥⇧7**, so the event arrives with Option and Shift held, {@link matches}
   * refuses it for that reason, and the shortcut this application has always
   * advertised has never once worked on that keyboard. Measured on the running
   * application rather than deduced --- ⌘ with the `\` character did nothing,
   * ⌘ with physical key 42 toggled the sidebar.
   *
   * **As well as, never instead of**, and that is not belt and braces. A code
   * names a *position*: `Backslash` is `\` on a US keyboard and `#` on a German
   * one, so matching only the code would move a chord for every reader outside
   * the layout it was written on. Matching both means the chord is *either* the
   * character or the key in that position, whichever the keyboard can offer.
   *
   * **Which path actually delivers the chord differs by platform**, and it is
   * worth saying rather than leaving to be discovered. On macOS the menu bar
   * claims the accelerator, so the chord never reaches the page and
   * {@link matches} is not what fixes it there --- `menubar.ts` derives the
   * accelerator from this same field, which is the point: the two agree by
   * construction instead of by two tables happening to say the same thing. On
   * Windows there is no menu bar, and this field is the only thing standing
   * between a German keyboard and a dead shortcut.
   *
   * Only one binding carries one, and the absences are deliberate.
   * `view.zoomIn` and `view.zoomOut` are punctuation too and are left alone
   * because they already work: German puts `+` and `-` on unshifted keys, so
   * the character path matches, and adding the position would claim two further
   * chords for nothing. And see the collision test in `keys.test.ts` for why
   * `nav.forward` cannot have one at all.
   */
  code?: string;
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
  // The Option chords list what Option *produces* as well as the plain letter,
  // and that is not belt and braces. macOS translates Option+letter to another
  // character on a US layout --- ⌥C is `ç`, ⌥W is `∑`, ⌥G is `©` --- and whether
  // that translation survives Command being held as well is a WebKit detail
  // nothing here can assert: a synthetic event carries whatever `key` the
  // harness put in it, so the check harness cannot see this either way. Listing
  // both spellings is correct under both answers, which is what `keys` being a
  // list is for.
  "find.matchCase": { keys: ["c", "ç"], accel: true, alt: true },
  "find.wholeWord": { keys: ["w", "∑"], accel: true, alt: true },
  // ⌥⌘R rather than ⌘R, which is the rotate chord, and beside the other two
  // find toggles on the same modifier.
  "find.regex": { keys: ["r", "®"], accel: true, alt: true },
  "find.inSelection": { keys: ["s", "ß"], accel: true, alt: true },
  // Not a command --- the palette is what *lists* commands, so it has no row of
  // its own --- and in this table anyway, which it was not until the toolbar
  // grew a button for it. The button has to show the chord it stands for, and
  // the only honest way to show a chord is to render it from the same entry the
  // handler matches. Written by hand it would be the drift this table exists to
  // prevent, one increment after the file's own comment said ⌘K was the one
  // chord that could not drift because nothing rendered it.
  //
  // It also fixes a smaller thing on the way in. The hand-written test it
  // replaces was `(metaKey || ctrlKey) && key === "k"`, which tests neither
  // Shift nor Option, so ⇧⌘K and ⌥⌘K opened the palette too --- the same
  // both-directions bug the note on `Binding.alt` describes.
  "app.palette": { keys: ["k", "K"], accel: true },
  "view.zoomIn": { keys: ["+", "="], accel: true, shown: "+" },
  "view.zoomOut": { keys: ["-"], accel: true, shown: "−" },
  // ⌘0 for fit-width was here first and stays, which is why the other two are
  // not Acrobat's or Preview's: both of those give ⌘0 to a different fit, and
  // moving a binding a reader already has is worse than not matching an
  // application they may not use. ⌘9 sits next to it and reads as the wider
  // fit; ⌘1 for actual size is the one Acrobat spelling that does not collide.
  "view.fitWidth": { keys: ["0"], accel: true },
  "view.fitPage": { keys: ["9"], accel: true },
  "view.actualSize": { keys: ["1"], accel: true },
  "view.zoomTo": { keys: ["z", "Ω"], accel: true, alt: true },
  "view.rotateClockwise": { keys: ["r", "R"], accel: true },
  "view.rotateCounterClockwise": { keys: ["l", "L"], accel: true },
  // The one binding with a physical key beside its character, and the reason is
  // in `Binding.code`: `\` is ⌥⇧7 on a German keyboard, so this chord could not
  // be typed there at all. `Backslash` is the `#` key on that layout, which is
  // where a Mac reader's finger goes for ⌘\ anyway, because that is the key the
  // menu bar highlights.
  "view.toggleSidebar": { keys: ["\\"], code: "Backslash", accel: true },
  "view.invertPages": { keys: ["i", "I"], accel: true, shift: true },
  "nav.nextPage": { keys: ["n"] },
  "nav.previousPage": { keys: ["p"] },
  "nav.firstPage": { keys: ["Home"] },
  "nav.lastPage": { keys: ["End"] },
  "nav.goToPage": { keys: ["g", "G", "©"], accel: true, alt: true },
  // Browser spelling, because that is where a reader has met Back and Forward
  // before. Deliberately not ⌘← / ⌘→, which macOS gives to line-start and
  // line-end in every text field and which the find bar is a text field.
  "nav.back": { keys: ["[", "\u201c"], accel: true },
  // ⌥⌘L and ⇧⌥⌘L. The macOS alternates are what the key *produces* with Option
  // held --- ¬ and Ò on a US layout --- which is why they are listed rather than
  // inferred: `matches` compares `event.key`, and Option has already rewritten
  // it by the time the handler sees it.
  "nav.nextLink": { keys: ["l", "\u00ac"], accel: true, alt: true },
  "nav.previousLink": {
    keys: ["L", "\u00d2"],
    accel: true,
    alt: true,
    shift: true,
  },
  "nav.forward": { keys: ["]", "\u2018"], accel: true },
  // ⌥⌘M and ⇧⌥⌘M, beside the link walk on the same modifier because they are
  // the same gesture over a different set of things on the page. The macOS
  // alternates are what Option makes of the letter --- µ and Â on a US layout.
  "nav.nextMark": { keys: ["m", "\u00b5"], accel: true, alt: true },
  "nav.previousMark": {
    keys: ["M", "\u00c2"],
    accel: true,
    alt: true,
    shift: true,
  },
  // The page operations, on Shift plus the view-rotation chords. Same gesture,
  // different subject: ⌘R turns the view, ⇧⌘R turns the page in the document.
  "edit.rotatePageClockwise": { keys: ["r", "R"], accel: true, shift: true },
  "edit.rotatePageCounterClockwise": {
    keys: ["l", "L"],
    accel: true,
    shift: true,
  },
  // ⌘Z and ⇧⌘Z, which is what every application on both platforms uses and the
  // one place here where matching the convention beats any other argument.
  "edit.undo": { keys: ["z", "Z"], accel: true },
  "edit.redo": { keys: ["z", "Z"], accel: true, shift: true },
  // ⌘S and ⇧⌘S, the pair every application on both platforms uses, and they
  // mean here what they mean everywhere: the first replaces the open file, the
  // second writes a second one. The name "Save a copy" rather than "Save as"
  // is the one departure --- tpdf goes on reading the file it opened, so a
  // reader who picked "Save as" and then kept editing would be editing the
  // document they thought they had left behind.
  "file.save": { keys: ["s"], accel: true },
  "file.saveCopy": { keys: ["s", "S"], accel: true, shift: true },
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
 * come to mean something the handler does not accept.
 *
 * Takes a binding rather than an id so that the ordering below can be tested
 * against combinations no command currently uses. It had one --- the comment
 * inside said "Option, Shift, Command" while the code emitted Shift first ---
 * and nothing could go red, because no binding held both and `label` accepted
 * only ids that exist.
 */
export function render(binding: Binding): string {
  const modified = Boolean(binding.accel || binding.shift || binding.alt);
  // What this keyboard prints in that position, when the binding names one and
  // the platform has answered. Before the answer arrives, and on a platform that
  // cannot give one, this is the character the binding declares --- which is the
  // label the palette showed before any of this existed.
  const printed = binding.code === undefined ? undefined : PRINTED[binding.code];
  const first = printed ?? binding.keys[0] ?? "";
  // A letter in a chord is capitalised and a bare one is not, which is what
  // every menu on this platform does: ⌘O, but `n` for the unmodified key.
  const key =
    binding.shown ?? (modified && first.length === 1 ? first.toUpperCase() : first);
  if (!MAC) {
    // Windows order and Windows spelling: Ctrl, Alt, Shift, joined by `+`. Not
    // a cosmetic difference --- this function renders every shortcut a reader
    // is ever shown, in the palette, in both right-click menus and in the
    // toolbar's tooltips, and until this branch existed all of them read
    // ⌘ ⌥ ⇧ on Windows. A reader there was being taught a chord their
    // keyboard does not have, which is worse than being taught none: reported
    // from use as ⌘C and ⌘A in a Windows context menu.
    //
    // `Binding.accel` has said "⌘ on macOS, Ctrl elsewhere" since it was
    // written. The data model knew; only the renderer did not.
    const parts = [];
    if (binding.accel) parts.push("Ctrl");
    if (binding.alt) parts.push("Alt");
    if (binding.shift) parts.push("Shift");
    parts.push(key);
    return parts.join("+");
  }
  // macOS order, which is Control, Option, Shift, Command, then the key.
  return `${binding.alt ? "⌥" : ""}${binding.shift ? "⇧" : ""}${binding.accel ? "⌘" : ""}${key}`;
}

/**
 * Whether to spell chords the way macOS does.
 *
 * Read from the web view rather than asked of Rust, unlike {@link PRINTED} next
 * door, and the difference is what each answer is *for*. What a key prints is a
 * property of the reader's keyboard layout that no web API reports reliably ---
 * `keylayout.rs` has the account. Which glyphs a platform spells its modifiers
 * with is not a question about this machine's configuration, and asking across
 * the boundary would make it asynchronous: every label rendered before the reply
 * lands would be wrong and would need re-rendering, which is exactly the flicker
 * `relabelCommands` exists to absorb for the one answer that genuinely has to be
 * awaited.
 *
 * Mutable so both spellings can be tested from either machine. A rule with one
 * branch reachable is a rule with an untestable half, and this file already
 * carries the lesson --- see the note on {@link render} about an ordering that
 * could not go red because no binding exercised it.
 */
let MAC = detectMac();

/** What the web view says it is running on. */
function detectMac(): boolean {
  const agent = globalThis.navigator?.userAgent ?? "";
  return /Mac|iPhone|iPad/.test(agent);
}

/**
 * Forces the modifier spelling, for tests.
 *
 * Pass nothing to go back to what the web view says, so a test that sets it
 * cannot decide what the rest of the file sees.
 */
export function setMacSpelling(mac?: boolean): void {
  MAC = mac ?? detectMac();
}

/**
 * Whether the web view is running on macOS.
 *
 * The same answer {@link render} spells modifiers from, and deliberately so: a
 * second detector is a second thing to drift, which `docs/TRAPS.md` records
 * under two resolvers agreeing with themselves. The only caller outside this
 * file is the viewer's press guard, where the platform decides a *gesture*
 * rather than a spelling --- a Control+click is a right-click here and an
 * ordinary modified press everywhere else.
 *
 * So {@link setMacSpelling} drives this too, and its name is now narrower than
 * what it moves. That is the intended shape rather than an oversight: there is
 * one answer to what platform this is, and a test that forces it forces every
 * rule that reads it.
 */
export function isMac(): boolean {
  return MAC;
}

/**
 * What the reader's keyboard prints, by `KeyboardEvent.code`.
 *
 * Empty until {@link setPrintedKeys} is told otherwise, which is deliberate: a
 * label rendered before the platform answers is the character the binding
 * declares, and that is exactly what was shown before this existed. There is no
 * state in which a chord renders blank.
 */
const PRINTED: Record<string, string> = {};

/**
 * Records what this keyboard prints, so labels can name the key a reader sees.
 *
 * Called once at startup with the platform's answer --- see `keylayout.rs` for
 * why the platform has to be asked at all: `navigator.keyboard.getLayoutMap()`
 * is the browser API for this and WebKit does not implement it.
 *
 * Mutates a module-level map rather than being threaded through every caller,
 * because the alternative is making {@link label} async and every command
 * registration wait on a keyboard read. Labels are already re-derived from this
 * by `relabel`, so the ordering is: ask, record, relabel.
 */
export function setPrintedKeys(printed: Record<string, string>): void {
  for (const key of Object.keys(PRINTED)) delete PRINTED[key];
  Object.assign(PRINTED, printed);
}

/** The string the palette shows for a bound command. */
export function label(id: BoundCommand): string {
  return render(BINDINGS[id]);
}

/**
 * The accelerator string a native menu item wants, or null for no accelerator.
 *
 * A third rendering of the same {@link Binding} the palette's label and the key
 * handler read, for the same reason the second one exists: three hand-written
 * spellings of one chord drift, and the one that drifts is whichever nothing
 * presses.
 *
 * **Null for any binding that does not hold the accelerator key**, and that is
 * the load-bearing half. A menu accelerator is claimed by the menu bar *before*
 * the web view sees the key, so registering a bare `n` --- what
 * `nav.nextPage` is bound to --- would take the letter out of the find field
 * and out of every text input the application ever grows. The unmodified
 * bindings keep working exactly as they do now, through handlers that can see
 * what has focus; they simply appear in the menu without a shortcut beside
 * them. `menubar.ts` withholds a further four for the same reason at one
 * remove: ⌘Z, ⇧⌘Z, ⌘C and ⌘A are chords a text field claims even though they
 * carry the modifier.
 *
 * **Null for anything but a letter or a digit, and that one is measured rather
 * than cautious.** A menu accelerator names a *physical key*; {@link matches}
 * reads `event.key`, which is the *character* that key produced. On a US layout
 * the two agree and the difference is invisible. On the German layout this was
 * developed against they do not: `Backslash` is the `#` key, `BracketLeft` is
 * `ö`, and a menu built from the character table advertised ⌘#, ⌘Ö and ⌘Ä for
 * commands whose palette entry says ⌘\, ⌘[ and ⌘]. Read out of the running
 * application's own menu bar, and then confirmed by pressing both: ⌘ with the
 * `\` character did nothing, ⌘ with physical key 42 toggled the sidebar.
 *
 * So the menu claims only the keys whose character and position agree on the
 * layouts this ships to. The punctuation chords keep working exactly as they
 * did --- which on a German keyboard means three of them do not work at all,
 * a defect that predates the menu and is `keys.ts`'s to fix, not the menu's to
 * paper over by claiming a different chord than the one it advertises.
 *
 * The residual, stated rather than implied: a layout that moves *letters* ---
 * AZERTY swaps A and Q --- breaks the same way for letter chords. The durable
 * fix for both is matching on `event.code` instead of `event.key`, which would
 * make the handler and the accelerator one vocabulary rather than two.
 */
export function accelerator(binding: Binding): string | null {
  if (!binding.accel) return null;
  // A binding that names its physical key can be claimed as that key, whatever
  // character the layout prints on it --- which is the whole reason `code`
  // exists, and it makes the menu and the handler agree by construction rather
  // than by two tables happening to say the same thing.
  const key = binding.code ?? plainKey(binding.keys[0] ?? "");
  if (key === null) return null;
  // The parser is order-insensitive; this is macOS reading order so that a
  // string read in a diff matches the glyphs `render` produces beside it.
  const parts = [];
  if (binding.alt) parts.push("Alt");
  if (binding.shift) parts.push("Shift");
  parts.push("CmdOrCtrl", key);
  return parts.join("+");
}

/**
 * A single letter or digit, upper-cased, or null for anything else.
 *
 * The refusal is the point --- see {@link accelerator}. A punctuation key is
 * spelled by position in an accelerator and by character in a binding, and the
 * parser accepts the position happily, so a guess here claims a chord nobody
 * chose rather than failing.
 */
function plainKey(key: string): string | null {
  if (key.length !== 1) return null;
  const upper = key.toUpperCase();
  return /^[A-Z0-9]$/.test(upper) ? upper : null;
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
  // Position *or* character --- see `Binding.code`. The character alone leaves
  // ⌘\ untypable on a German keyboard; the position alone would move the chord
  // for everyone whose layout is not the one it was written on.
  if (binding.code !== undefined && event.code === binding.code) return true;
  return binding.keys.includes(event.key);
}

/**
 * Whether the key went to something a reader is typing into.
 *
 * Here rather than in one handler because **both** key handlers need it and
 * they need it for opposite reasons. `appcommands.ts` guards ⌘Z and ⌘⇧Z alone,
 * since every other binding it holds is a chord no text field claims and taking
 * those from the find bar is what a reader wants. `viewer.ts` guards
 * *everything*: its keys are `n`, `p`, Space, Home, End and the arrows, which a
 * text field claims all of, and since 2026-08-18 there is a text field inside
 * the viewer's own root --- the note on a mark. Two copies of this test would be
 * two chances to disagree about what a text field is.
 *
 * Duck-typed rather than `instanceof HTMLElement`, so that the guard is
 * exercised by the same tests that exercise everything else.
 *
 * Measured rather than assumed, because the guess was wrong: the test runner has
 * no DOM at all, `globalThis.HTMLElement` is `undefined`, and
 * `target instanceof HTMLElement` there **throws** ---
 * `TypeError: Right-hand side of 'instanceof' is not an object`. So it does not
 * quietly answer "not a text field"; it takes the whole handler down, and the
 * only way to test the guard would be to stand up a DOM for it. Duck-typing
 * costs three field reads and needs neither.
 */
export function inTextField(event: KeyboardEvent): boolean {
  const target = event.target as {
    tagName?: string;
    isContentEditable?: boolean;
  } | null;
  if (!target) return false;
  if (target.isContentEditable === true) return true;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}
