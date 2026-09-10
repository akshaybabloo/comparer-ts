import { defineConfig } from "vite";
import { resolve } from "path";
import dts from "vite-plugin-dts";
import wasm from "vite-plugin-wasm";

export default defineConfig({
  build: {
    lib: {
      entry: resolve(import.meta.dirname, "src/index.ts"),
      name: "Comparer",
      fileName: (format) => `comparer.${format}.js`,
      formats: ["es"],
    },
    target: "esnext",
    outDir: "dist",
    assetsInlineLimit: 0,
  },
  test: {
    include: ["src/**/*.test.ts"],
  },
  plugins: [
    wasm(),
    dts({
      exclude: ["**/*.test.ts"],
      insertTypesEntry: true,
    }),
  ],
});
