import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed port and surfaces Rust errors itself, so Vite must not
// clear the screen or wander to another port.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/vendor/**"] },
  },
  build: {
    target: "safari15",
    sourcemap: true,
    rollupOptions: {
      // Relative to the Vite root, so no `node:path` and no `__dirname` --
      // there are no Node type declarations in this project.
      input: {
        // The app.
        index: "index.html",
        // The shell floor (spike 0.7): the same startup work with no framework
        // under it, so the payload's cost can be measured rather than assumed.
        shell: "shell.html",
      },
    },
  },
});
