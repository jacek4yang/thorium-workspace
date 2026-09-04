/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Vite dev server for the Tauri WebView. strictPort so a busy port fails
// loudly instead of silently migrating to another port that Tauri does not
// watch.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: "es2022",
  },
  test: {
    environment: "jsdom",
    globals: false,
  },
});
