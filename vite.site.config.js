import { defineConfig } from "vite";
export default defineConfig({
    root: "site",
    base: "./",
    publicDir: "public",
    plugins: [
        {
            name: "local-file-preview",
            transformIndexHtml: {
                order: "post",
                handler: (html) => html.replaceAll(" crossorigin", ""),
            },
        },
    ],
    build: {
        outDir: "../dist-site",
        emptyOutDir: true,
        assetsInlineLimit: 0,
    },
});
