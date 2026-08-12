import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri serves the built assets from disk; a fixed port keeps devUrl in
// tauri.conf.json honest, and failing rather than falling back to another port
// avoids a silently blank window.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2021",
    sourcemap: false,
  },
});
