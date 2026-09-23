import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The window talks to the engine over Tauri IPC only; there is no HTTP API to
// proxy. `tauri dev` loads this server on port 5174.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5174,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    rollupOptions: {
      output: { manualChunks: { markdown: ["react-markdown", "remark-gfm"] } },
    },
  },
});
