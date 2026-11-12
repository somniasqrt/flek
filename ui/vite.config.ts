import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

// Tauri expects a fixed port and no screen clearing (its own logs share the terminal).
export default defineConfig({
  plugins: [solid()],
  worker: { format: "es" },
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "safari16", outDir: "dist", emptyOutDir: true },
});
