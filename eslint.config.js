import js from "@eslint/js";
import globals from "globals";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";

/**
 * Wardian lint rules.
 *
 * `tsc --noEmit` already covers types and unused locals, so this config is
 * deliberately narrow: it holds the rules a type checker cannot express, and
 * the project conventions that lived only in AGENTS.md prose until now.
 *
 * Rules land as `error` only where the codebase is already clean, so the gate
 * starts green and a violation always means a new one. Everything else is a
 * `warn` with its count frozen in `budgets.json`.
 */

/** Tailwind palette classes that bypass the semantic theme tokens. */
const TAILWIND_PALETTE = String.raw`\b(?:text|bg|border|ring|from|via|to|divide|outline|shadow|decoration|accent|caret|fill|stroke)-(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)-\d{2,3}\b`;

export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "docs/.vitepress/dist/**",
      "docs/.vitepress/cache/**",
      "node_modules/**",
      "target/**",
      "vendor/**",
      "tools/**",
      "test-results/**",
      "coverage/**",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    // Rules that carry real signal but are not yet at zero. They stay visible
    // as warnings and their counts are frozen in `budgets.json`, so the total
    // can fall but never rise. Promote one to `error` when it reaches zero.
    rules: {
      // Terminal, ANSI, and PTY parsing match control characters deliberately.
      "no-control-regex": "off",
      "preserve-caught-error": "warn",
      "no-useless-escape": "warn",
      "prefer-const": "warn",
      "no-useless-assignment": "warn",
      "no-empty-pattern": "warn",
      "no-constant-condition": "warn",
      "@typescript-eslint/no-unused-vars": "warn",
    },
  },
  {
    // Build scripts and the native E2E harness are Node ESM/CJS, not browser
    // TypeScript. Without their globals every `process` and `console` reads as
    // undefined.
    files: ["**/*.{js,mjs,cjs}"],
    languageOptions: {
      // These files are Node, but the driver scripts also carry callbacks that
      // are serialized and evaluated inside the page, so browser globals are
      // legitimately referenced from Node source here.
      globals: { ...globals.node, ...globals.browser },
      sourceType: "module",
    },
    rules: {
      "@typescript-eslint/no-require-imports": "off",
    },
  },
  {
    files: ["public/**/*.js"],
    languageOptions: { globals: { ...globals.serviceworker, ...globals.browser } },
  },
  {
    files: ["**/*.cjs"],
    languageOptions: { sourceType: "commonjs" },
  },
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
    },
    plugins: { "react-hooks": reactHooks },
    rules: {
      // `tsc` owns these and reports them with better positions.
      "@typescript-eslint/no-unused-vars": "off",
      "no-unused-vars": "off",
      "no-undef": "off",

      // AGENTS.md: "ALWAYS use theme variables or themed classes instead of
      // hardcoded Tailwind colors." Enforced here rather than asked for in
      // prose. Status colours belong in the theme, not at a call site.
      "no-restricted-syntax": [
        "error",
        {
          selector: `Literal[value=/${TAILWIND_PALETTE}/]`,
          message:
            "Hardcoded Tailwind palette class. Use a theme token (var(--color-wardian-*)) or a themed class such as .text-muted.",
        },
        {
          selector: `TemplateElement[value.raw=/${TAILWIND_PALETTE}/]`,
          message:
            "Hardcoded Tailwind palette class. Use a theme token (var(--color-wardian-*)) or a themed class such as .text-muted.",
        },
      ],

      // AGENTS.md: "Never use `any` unless required by external library
      // constraints." The codebase already honours this.
      "@typescript-eslint/no-explicit-any": "error",

      // A dependency array that lies is the defect class behind several of the
      // stale-render fixes in the audited window. Warn-level while the
      // existing count is burned down under budget.
      "react-hooks/exhaustive-deps": "warn",
      "react-hooks/rules-of-hooks": "error",
    },
  },
  {
    // Test files legitimately reach for loose typing when standing in for a
    // provider payload, and fixtures name colours to assert on them.
    files: ["**/*.test.{ts,tsx}", "e2e/**/*.ts", "src/test/**/*.ts"],
    rules: {
      "no-restricted-syntax": "off",
      "@typescript-eslint/no-explicit-any": "off",
    },
  },
);
