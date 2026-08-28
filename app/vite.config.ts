import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri serves the built assets from the executable, so everything must be
// relative and self-contained: no CDN, no absolute paths, no code splitting
// that assumes a server.
export default defineConfig({
  plugins: [react()],
  base: "./",
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: {
    target: "chrome110",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    chunkSizeWarningLimit: 900,
  }
});
