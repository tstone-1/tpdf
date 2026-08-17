/**
 * Just enough DOM to construct a {@link Scroller} or a {@link Viewer} in a test.
 *
 * Imported only by `*.test.ts` files and never by the application --- nothing in
 * `main.ts`'s import graph reaches it, so it is not in the bundle.
 *
 * There is no jsdom here, deliberately: adding one is a dependency decision, and
 * what these tests need is a handful of methods plus **control of the frame
 * clock**, which a real DOM would not give. That control is the point. The
 * defect this harness was written for is a continuation that restarts a frame
 * loop after the viewer that owns it has been destroyed, and the only way to see
 * that is to count the callbacks `requestAnimationFrame` is handed rather than
 * to wait and see whether anything happens.
 *
 * What it deliberately does **not** do: layout, style resolution, or painting.
 * `getContext` returns `null`, which every canvas user in this codebase already
 * handles, so a test here can say what was requested and what was cached but
 * never what a pixel looks like. Pixels are the check harness's job (`bin/` and
 * `viewercheck.ts`), against a real webview.
 */

/** A node in the fake tree. Only what the code under test actually calls. */
export class FakeElement {
  readonly tagName: string;
  readonly children: FakeElement[] = [];
  parent: FakeElement | null = null;
  /** Assignments only --- `cssText` and individual properties both land here. */
  readonly style: Record<string, string> = {};
  readonly dataset: Record<string, string> = {};
  readonly attributes = new Map<string, string>();
  readonly listeners = new Map<string, Set<(event: unknown) => void>>();
  textContent = "";
  tabIndex = 0;
  width = 0;
  height = 0;
  clientWidth = 0;
  clientHeight = 0;
  scrollTop = 0;
  /** Set by {@link focus}, so a test can assert where the keyboard went. */
  focused = false;

  constructor(tagName: string) {
    this.tagName = tagName;
  }

  get childElementCount(): number {
    return this.children.length;
  }

  appendChild(child: FakeElement): FakeElement {
    child.parent?.removeChild(child);
    child.parent = this;
    this.children.push(child);
    return child;
  }

  append(...children: FakeElement[]): void {
    for (const child of children) this.appendChild(child);
  }

  insertBefore(child: FakeElement, before: FakeElement | null): FakeElement {
    child.parent?.removeChild(child);
    child.parent = this;
    const at = before ? this.children.indexOf(before) : -1;
    if (at < 0) this.children.push(child);
    else this.children.splice(at, 0, child);
    return child;
  }

  removeChild(child: FakeElement): void {
    const at = this.children.indexOf(child);
    if (at >= 0) this.children.splice(at, 1);
    child.parent = null;
  }

  remove(): void {
    this.parent?.removeChild(this);
  }

  replaceChildren(...children: FakeElement[]): void {
    for (const child of [...this.children]) this.removeChild(child);
    for (const child of children) this.appendChild(child);
  }

  setAttribute(name: string, value: string): void {
    this.attributes.set(name, value);
  }

  getAttribute(name: string): string | null {
    return this.attributes.get(name) ?? null;
  }

  addEventListener(type: string, listener: (event: unknown) => void): void {
    const set = this.listeners.get(type) ?? new Set();
    set.add(listener);
    this.listeners.set(type, set);
  }

  removeEventListener(type: string, listener: (event: unknown) => void): void {
    this.listeners.get(type)?.delete(listener);
  }

  /** Calls every listener for `type`. There is no bubbling and none is needed. */
  dispatch(type: string, event: Record<string, unknown>): void {
    const target = { preventDefault: () => {}, stopPropagation: () => {}, ...event };
    for (const listener of [...(this.listeners.get(type) ?? [])]) listener(target);
  }

  contains(node: unknown): boolean {
    if (node === this) return true;
    return this.children.some((child) => child.contains(node));
  }

  focus(): void {
    this.focused = true;
  }

  /**
   * Pointers this element has captured.
   *
   * Tracked rather than stubbed away, so that `hasPointerCapture` can answer
   * truthfully: a double that always says `false` would let a release that
   * never happens look exactly like one that did, and releasing a capture is
   * how a strip stops swallowing every pointer event on the page.
   */
  readonly captured = new Set<number>();

  setPointerCapture(pointerId = 0): void {
    this.captured.add(pointerId);
  }
  releasePointerCapture(pointerId = 0): void {
    this.captured.delete(pointerId);
  }
  hasPointerCapture(pointerId = 0): boolean {
    return this.captured.has(pointerId);
  }
  scrollIntoView(): void {}

  getBoundingClientRect(): { left: number; top: number; width: number; height: number } {
    return { left: 0, top: 0, width: this.clientWidth, height: this.clientHeight };
  }

  /** Null, as it is for a canvas with no backing implementation. */
  getContext(): null {
    return null;
  }

  querySelector(selector: string): FakeElement | null {
    for (const child of this.children) {
      if (child.tagName === selector) return child;
      const found = child.querySelector(selector);
      if (found) return found;
    }
    return null;
  }
}

/** What {@link installFakeDom} hands back, and what a test drives it through. */
export interface FakeDom {
  /** An element sized like a window, to mount a viewer or scroller into. */
  root: FakeElement;
  /** Callbacks handed to `requestAnimationFrame` and not yet run or cancelled. */
  pendingFrames(): number;
  /** How many callbacks have been handed over since the last {@link reset}. */
  scheduledFrames(): number;
  /** Runs every currently-queued frame callback. */
  runFrames(): void;
  /** Forgets the frame counters, so an assertion can be scoped to what follows. */
  reset(): void;
  /** Puts back whatever globals were there before. */
  restore(): void;
}

/**
 * Installs the fake globals and returns the handle that drives them.
 *
 * Frame callbacks are queued rather than run, because "did this schedule a
 * frame?" is the question, and an implementation that ran them would answer a
 * different one --- a loop that reschedules itself would run forever.
 */
export function installFakeDom(width = 900, height = 700): FakeDom {
  const previous = new Map<string, unknown>();
  const globals = globalThis as unknown as Record<string, unknown>;
  const set = (name: string, value: unknown): void => {
    previous.set(name, globals[name]);
    globals[name] = value;
  };

  let frames = new Map<number, () => void>();
  let nextFrame = 1;
  let scheduled = 0;

  const root = new FakeElement("div");
  root.clientWidth = width;
  root.clientHeight = height;

  const document = {
    createElement: (tag: string) => new FakeElement(tag),
    /**
     * A text node, modelled as an element with no tag and its text set.
     *
     * Not a separate class: every consumer here walks `children` and reads
     * `textContent`, and a second node type would need its own handling in each
     * of them. The empty `tagName` is what distinguishes it, and it is what
     * `a11y.test.ts` reads to tell a marked-up link from the prose around it.
     */
    createTextNode: (data: string) => {
      const node = new FakeElement("");
      node.textContent = data;
      return node;
    },
    documentElement: new FakeElement("html"),
  };

  const window = {
    devicePixelRatio: 1,
    matchMedia: () => ({
      matches: false,
      addEventListener: () => {},
      removeEventListener: () => {},
    }),
    addEventListener: () => {},
    removeEventListener: () => {},
  };

  set("document", document);
  set("window", window);
  set("devicePixelRatio", 1);
  set("getComputedStyle", () => ({ getPropertyValue: () => "" }));
  set("ResizeObserver", class {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  });
  set("requestAnimationFrame", (callback: () => void) => {
    const id = nextFrame++;
    scheduled++;
    frames.set(id, callback);
    return id;
  });
  set("cancelAnimationFrame", (id: number) => {
    frames.delete(id);
  });

  return {
    root,
    pendingFrames: () => frames.size,
    scheduledFrames: () => scheduled,
    runFrames: () => {
      const queued = [...frames.values()];
      frames = new Map();
      for (const callback of queued) callback();
    },
    reset: () => {
      scheduled = 0;
    },
    restore: () => {
      for (const [name, value] of previous) {
        if (value === undefined) delete globals[name];
        else globals[name] = value;
      }
    },
  };
}

/**
 * Lets every already-resolved promise run.
 *
 * The continuations under test are `.then` chains on a cache's own promise, so
 * "has it re-entered yet" needs several microtask turns, not one --- and a test
 * that awaited only one turn would report the guard as working while the wake
 * was still queued behind it.
 */
export async function settle(turns = 8): Promise<void> {
  for (let turn = 0; turn < turns; turn++) await Promise.resolve();
}
