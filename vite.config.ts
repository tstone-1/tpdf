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
  },
});
