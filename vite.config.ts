import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // xterm.js alone is ~500 kB; bundle size matters little for a locally loaded desktop app.
  build: {
    chunkSizeWarningLimit: 1500,
  },
  // 2. tauri expects a fixed port, fail if that port is not available. Ports below ~15000 can fall in the ranges
  //    Windows reserves for Hyper-V / WinNAT when the dynamic port range starts low (listen fails with EACCES), and
  //    49152 and above is the default dynamic range, so the ports sit between the two.
  server: {
    port: 31420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 31421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
