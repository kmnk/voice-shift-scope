import { defineConfig } from "vite";

// Tauri の画面。開発のときは 5173 番で待ち、build したものは ../frontend/dist に書く
export default defineConfig({
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
});
