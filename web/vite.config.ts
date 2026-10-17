import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// In dev, the Worker (wrangler dev) serves /api and /ws on :8787.
const backend = process.env.MONI_BACKEND ?? "http://127.0.0.1:8787";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    // The Worker enforces Origin == target origin for cookie-authenticated writes and the
    // dashboard socket; present the proxied requests as same-origin (dev only).
    proxy: {
      "/api": { target: backend, changeOrigin: true, headers: { origin: backend } },
      "/ws": { target: backend, ws: true, changeOrigin: true, headers: { origin: backend } },
    },
  },
  build: { outDir: "dist", emptyOutDir: true, chunkSizeWarningLimit: 600 },
});
