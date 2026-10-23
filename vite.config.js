/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import babel from "@rolldown/plugin-babel";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;
// https://vite.dev/config/
export default defineConfig(() => ({
    // React Compiler memoizes the components and hooks; every one of them must compile
    // (src/compiler.test.ts checks, and "渲染与 memo" in docs/ARCHITECTURE.md says why).
    plugins: [react(), babel({ presets: [reactCompilerPreset()] })],
    test: {
        // Vitest caches a test file's transform while the file is unchanged, `import.meta.glob`
        // included: the tests that list the sources that way (layers, icons, React Compiler…)
        // would miss a new file until then. Turning the cache off costs well under a second.
        fsModuleCache: false,
        // Stylesheets are read as text by styles.test.ts, which Vitest leaves empty by default (the
        // pattern has no `$`: the id ends in `?raw`).
        css: { include: [/\.css/] },
    },
    // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
    //
    // 1. prevent Vite from obscuring rust errors
    clearScreen: false,
    // xterm.js alone is ~500 kB, and React Compiler's memoization adds ~150 kB; bundle size
    // matters little for a locally loaded desktop app.
    build: {
        chunkSizeWarningLimit: 2000,
        // React Compiler runs through Babel and takes most of the build's time on every build:
        // known, so not reported each time.
        rolldownOptions: { checks: { bundlerTimings: false } },
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
