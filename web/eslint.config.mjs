// Flat config, ESLint v10. Named .mjs rather than .js because there is no
// package.json here to declare `"type": "module"` — the extension is
// what makes this file ESM on every Node version.
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import eslintReact from "@eslint-react/eslint-plugin";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";

export default tseslint.config(
  { ignores: ["node_modules/**"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  // The -typescript preset, not -type-checked: the latter wants a parser
  // pointed at tsconfig.json, and nothing else here does type-aware linting.
  eslintReact.configs["recommended-typescript"],
  {
    plugins: { "react-hooks": reactHooks },
    rules: reactHooks.configs.recommended.rules,
  },
  {
    languageOptions: {
      globals: { ...globals.browser },
    },
  },
  {
    // router.tsx holds the current page's component in state and renders it,
    // which static-components reads as a component built during render. It
    // is only ever a reference to a module already imported, so the rule is
    // wrong here, and it is wrong about the whole design of the file rather
    // than about two lines of it. Same root cause as
    // https://github.com/Rel1cx/eslint-react/issues/1762 — a component
    // reaching JSX through a hook call isn't traced back to its origin.
    files: ["src/router.tsx"],
    rules: { "@eslint-react/static-components": "off" },
  },
  {
    // build.mjs is a Node script, not part of the browser bundle.
    files: ["build.mjs"],
    languageOptions: { globals: { ...globals.node } },
  },
);
