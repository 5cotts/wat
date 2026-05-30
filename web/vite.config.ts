import { defineConfig } from "vite";

export default defineConfig({
  // Relative asset paths so the build works whether served at the domain root
  // (e.g. wat-5cotts.zo.computer) or under a subpath (e.g. zo.pub/5cotts/wat).
  base: "./",
  server: {
    allowedHosts: [".zocomputer.io", ".zo.computer"],
    // HMR disabled: when proxied via zo.computer the WS connection is unstable,
    // and Vite's client calls location.reload() on every reconnect — which on
    // mobile fires whenever the soft keyboard opens, making the page appear to
    // reload on every tap/keystroke. Manual refresh is fine for this app.
    hmr: false,
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
  },
});
