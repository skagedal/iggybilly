// Flat config, ESLint v9. Named .mjs rather than .js because there is no
// package.json here to declare `"type": "module"` — the extension is
// what makes this file ESM on every Node version.
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactPlugin from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";

export default tseslint.config(
  { ignores: ["node_modules/**"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  reactPlugin.configs.flat.recommended,
  reactPlugin.configs.flat["jsx-runtime"],
  {
    plugins: { "react-hooks": reactHooks },
    rules: reactHooks.configs.recommended.rules,
  },
  {
    languageOptions: {
      globals: { ...globals.browser },
    },
    settings: { react: { version: "detect" } },
  },
  {
    // build.mjs is a Node script, not part of the browser bundle.
    files: ["build.mjs"],
    languageOptions: { globals: { ...globals.node } },
  },
);
