import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed dev-server port (set in app-tauri/tauri.conf.json's
// build.devUrl) and needs its own env vars ignored so Vite doesn't try to
// read unrelated host env as client-exposed vars.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
});
