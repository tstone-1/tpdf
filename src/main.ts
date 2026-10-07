import { mount } from "svelte";
import App from "./App.svelte";
import { UI_LOCALE } from "./lib/i18n";

document.documentElement.lang = UI_LOCALE;

// A development instance says so. One started by `tauri dev` and the installed
// application are otherwise the same window with the same name, and a reader
// who has both open edits a real document in the build that is being changed
// under it. Vite sets `DEV` only for the dev server, so a release build and
// the checks build carry none of this.
if (import.meta.env.DEV) {
  const mark = document.createElement("style");
  mark.textContent =
    // A frame and a badge, and no colour on the header itself: its menus are
    // inside it and inherit what it is given, and white text on a red header
    // was white text on their white panels.
    //
    // The frame is a layer over the window, the same width on all four sides.
    // It was a shadow inside `body` with a thicker bar on the header, and
    // every row with a background of its own painted over the shadow: the
    // toolbar and the left panel had no red edge and the page area had one
    // only on the right. Reported from use. It takes no pointer events.
    "body::after{content:\"\";position:fixed;inset:0;border:3px solid #d42a2a;" +
    "pointer-events:none;z-index:2147483647}" +
    'header::before{content:"DEV";font:700 12px/1 system-ui,sans-serif;letter-spacing:0.08em;' +
    "padding:4px 7px;margin-right:6px;border-radius:4px;background:#d42a2a;color:#fff}";
  document.head.append(mark);
}

const target = document.getElementById("app");
if (!target) throw new Error("#app missing from index.html");

const app = mount(App, { target });

// Stamped here rather than inside a Svelte effect: effects run a microtask
// later, so an effect-side stamp would fold framework scheduling into whatever
// interval follows it. Read by the startup timeline (spike 0.2).
(window as unknown as Record<string, number>).__tpdfAppMounted = performance.now();

export default app;
