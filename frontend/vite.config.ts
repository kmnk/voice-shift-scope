/// <reference types="vitest/config" />
import { defineConfig } from "vite";

// The Tauri screen. Serves on port 5173 during development and writes the build to ../frontend/dist
export default defineConfig({
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
  // Unit tests read style.css with ?raw, so do not empty the CSS
  test: { css: true },
});
