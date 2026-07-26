import { mount } from "svelte";
import App from "./App.svelte";

const target = document.getElementById("app");
if (!target) throw new Error("#app missing from index.html");

const app = mount(App, { target });

// Stamped here rather than inside a Svelte effect: effects run a microtask
// later, so an effect-side stamp would fold framework scheduling into whatever
// interval follows it. Read by the startup timeline (spike 0.2).
(window as unknown as Record<string, number>).__tpdfAppMounted = performance.now();

export default app;
