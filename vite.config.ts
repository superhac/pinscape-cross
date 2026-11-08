import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Frontend lives in ./ui; Tauri loads ./dist in release builds.
export default defineConfig({
  root: "ui",
  // Served as-is at the web root: /help/... (upstream help pages, synced by
  // scripts/sync-upstream-assets.sh).
  publicDir: "../assets/public",
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    target: "es2021",
    chunkSizeWarningLimit: 4096, // Monaco is large; it's a local desktop app
  },
});
