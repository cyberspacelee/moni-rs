import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// In dev, the Worker (wrangler dev) serves /api and /ws on :8787.
const backend = process.env.MONI_BACKEND ?? "http://127.0.0.1:8787";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  server: {
    // The Worker enforces Origin == target origin for cookie-authenticated writes and the
    // dashboard socket; present the proxied requests as same-origin (dev only).
    proxy: {
      "/api": { target: backend, changeOrigin: true, headers: { origin: backend } },
      "/ws": { target: backend, ws: true, changeOrigin: true, headers: { origin: backend } },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    chunkSizeWarningLimit: 600,
    rolldownOptions: {
      output: {
        // React changes far less often than the app; its own chunk stays cached across deploys. The UI
        // libraries are left to automatic splitting so admin-only components stay out of the landing page.
        codeSplitting: {
          groups: [
            { name: "react", test: /node_modules[\\/](react|react-dom|scheduler)[\\/]/ },
          ],
        },
      },
    },
    // Flags are fetched only for countries on screen; inlining would put all 270 in the bundle.
    assetsInlineLimit: (file) => (file.includes("flag-icons") ? false : undefined),
  },
});
