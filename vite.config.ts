import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The Tauri conf's devUrl points at http://localhost:4179.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 4179,
    strictPort: true,
  },
});
