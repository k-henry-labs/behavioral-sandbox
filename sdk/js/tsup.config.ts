import { defineConfig } from "tsup";

export default defineConfig({
  entry: ["src/index.ts"],
  tsconfig: "tsconfig.build.json",
  format: ["cjs", "esm"],
  target: "node20",
  platform: "node",
  dts: true,
  sourcemap: true,
  splitting: false,
  clean: true,
  // napi's loader, not ours to bundle. It `require`s a `.node` per platform triple, and esbuild
  // resolving those fails the build; left external it is required at run time, from the package
  // root either output sits one directory below.
  external: ["../index.js"],
});
