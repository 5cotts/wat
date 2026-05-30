import { defineConfig } from "vite";

export default defineConfig({
  server: {
    allowedHosts: [".zocomputer.io", ".zo.computer"],
    hmr: {
      protocol: "wss",
      clientPort: 443,
    },
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
  },
});
