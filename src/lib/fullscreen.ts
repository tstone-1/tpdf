/**
 * Full screen and presentation: what the window is showing, and the viewer's
 * layout to put back afterwards.
 *
 * Full screen hides the toolbar, the tabs and the sidebar and leaves the
 * layout alone. Presentation is full screen with one page at a time at fit
 * page, on black; leaving it puts back the layout, fit and zoom the reader had.
 * `App.svelte` draws the chrome from {@link ScreenMode.mode} and hands the
 * window and the viewer in, so that this is testable without either.
 */

import type { Spread } from "./scroller";
import type { FitMode, Fitted } from "./zoom";

export type Screen = "window" | "full" | "presenting";

/** The part of the viewer a change of screen reads and sets. */
export interface ScreenViewer {
  readonly spread: Spread;
  readonly fitMode: FitMode;
  readonly currentZoom: number;
  setSpread(spread: Spread): void;
  setFit(mode: Fitted): void;
  setZoomFixed(zoom: number): void;
}

/** The part of the window a change of screen sets. */
export interface ScreenWindow {
  setFullscreen(on: boolean): Promise<void>;
}

interface Saved {
  spread: Spread;
  fit: FitMode;
  zoom: number;
}

export class ScreenMode {
  /** What the window is showing now. */
  mode: Screen = "window";
  /**
   * While presenting, the screen blacked or whited out in place of the page:
   * `b` and `w`, as in presentation programs. Any key or press brings the page
   * back.
   */
  blank: "black" | "white" | null = null;

  /**
   * A key while presenting: `b` or `w` blanks the screen, and anything while
   * it is blank brings the page back. Answers whether the key was used, so the
   * caller can keep it from doing anything else.
   */
  presentingKey(key: string): boolean {
    if (this.mode !== "presenting") return false;
    if (this.blank !== null) {
      this.blank = null;
      return true;
    }
    const lower = key.toLowerCase();
    if (lower !== "b" && lower !== "w") return false;
    this.blank = lower === "b" ? "black" : "white";
    return true;
  }

  private readonly saved = new Map<ScreenViewer, Saved>();

  /** The ordinary layout to keep with this tab, even when it is shown as a presentation. */
  layoutOf(viewer: ScreenViewer): Saved {
    return this.saved.get(viewer) ?? { spread: viewer.spread, fit: viewer.fitMode, zoom: viewer.currentZoom };
  }

  /** Apply presentation once to each mounted viewer, including tabs opened partway through. */
  show(viewer: ScreenViewer): void {
    if (this.mode !== "presenting" || this.saved.has(viewer)) return;
    this.saved.set(viewer, this.layoutOf(viewer));
    viewer.setSpread({ ...viewer.spread, continuous: false });
    viewer.setFit("page");
  }

  /** The tab keeps its ordinary layout through layoutOf when the viewer is torn down. */
  unmounted(viewer: ScreenViewer): void {
    this.saved.delete(viewer);
  }

  /** Fills the screen with the document, the layout as it is. */
  async full(window: ScreenWindow): Promise<void> {
    if (this.mode !== "window") return;
    await window.setFullscreen(true);
    this.mode = "full";
  }

  /**
   * Presents: full screen, one page (or pair) at a time, fitted to the page.
   * From full screen as well as from the window.
   */
  async present(window: ScreenWindow): Promise<void> {
    if (this.mode === "presenting") return;
    if (this.mode === "window") await window.setFullscreen(true);
    this.mode = "presenting";
  }

  async leave(window: ScreenWindow): Promise<void> {
    if (this.mode === "window") return;
    await window.setFullscreen(false);
    this.left();
  }

  /** Called on successful exit, including when the platform's own control was used. */
  left(): void {
    this.mode = "window";
    this.blank = null;
    for (const [viewer, saved] of this.saved) {
      viewer.setSpread(saved.spread);
      if (saved.fit === "none") viewer.setZoomFixed(saved.zoom);
      else viewer.setFit(saved.fit);
    }
    this.saved.clear();
  }
}
