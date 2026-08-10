// Bundles the TypeScript/React frontend into ../static/dist, which the
// Rust app serves via its existing ServeDir on /static.
//
// There are two entry points — src/main.tsx and the stylesheet. Pages
// aren't entries: main.tsx imports them dynamically (see pageData.ts),
// so esbuild splits each into its own chunk that's only fetched when
// that page is actually visited.
//
// Output filenames carry a content hash, so the Rust side can't guess
// them: we write a manifest.json mapping logical entry names ("main.js",
// "app.css") to their hashed URLs, and the server reads it at startup to
// fill in the <script>/<link> hrefs. See src/assets.rs.
//
//   node build.mjs            production build (minified)
//   node build.mjs --watch    rebuild on change, unminified, sourcemaps

import * as esbuild from "esbuild";
import { writeFileSync, rmSync, mkdirSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));
const outdir = resolve(root, "../static/dist");

const watch = process.argv.includes("--watch");
const dev = watch || process.argv.includes("--dev");

const entryPoints = [
  resolve(root, "src/main.tsx"),
  resolve(root, "src/styles/app.css"),
];

/** Writes static/dist/manifest.json after every successful build. */
const manifestPlugin = {
  name: "manifest",
  setup(build) {
    build.onEnd((result) => {
      if (result.errors.length > 0 || !result.metafile) return;
      const manifest = {};
      for (const [outPath, meta] of Object.entries(result.metafile.outputs)) {
        // Split chunks have no entryPoint; they're reached through the
        // entries' own import statements, so they need no manifest row.
        if (!meta.entryPoint) continue;
        const name = basename(meta.entryPoint).replace(/\.(tsx?|css)$/, "");
        const ext = outPath.endsWith(".css") ? "css" : "js";
        manifest[`${name}.${ext}`] = `/static/dist/${basename(outPath)}`;
      }
      writeFileSync(
        join(outdir, "manifest.json"),
        JSON.stringify(manifest, null, 2) + "\n",
      );
      const stamp = new Date().toTimeString().slice(0, 8);
      console.log(`[${stamp}] built ${Object.keys(manifest).length} entries`);
    });
  },
};

/** @type {import("esbuild").BuildOptions} */
const options = {
  entryPoints,
  outdir,
  bundle: true,
  format: "esm",
  // Required for the dynamic page imports to become separate chunks
  // rather than being inlined into main.js.
  splitting: true,
  target: ["es2022"],
  jsx: "automatic",
  minify: !dev,
  sourcemap: true,
  metafile: true,
  entryNames: "[name]-[hash]",
  chunkNames: "chunk-[hash]",
  assetNames: "[name]-[hash]",
  logLevel: "info",
  define: {
    "process.env.NODE_ENV": JSON.stringify(dev ? "development" : "production"),
  },
  plugins: [manifestPlugin],
};

// Start from an empty outdir so hashed files from earlier builds don't
// pile up in the image (and in git-ignored local state).
rmSync(outdir, { recursive: true, force: true });
mkdirSync(outdir, { recursive: true });

if (watch) {
  const ctx = await esbuild.context(options);
  await ctx.watch();
  console.log(`watching ${entryPoints.length} entries → ${outdir}`);
} else {
  await esbuild.build(options);
}
