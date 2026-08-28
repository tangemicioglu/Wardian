import type * as Monaco from "monaco-editor";

function wardianColor(name: string, fallback: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

export function fileMonacoThemeName() {
  return document.documentElement.getAttribute("data-theme") === "dark"
    ? "wardian-dark"
    : "wardian-light";
}

/** Defines the shared Monaco palette used by file source and comparison views. */
export function configureFileMonacoTheme(monaco: typeof Monaco) {
  const dark = document.documentElement.getAttribute("data-theme") === "dark";
  const background = wardianColor("--color-wardian-bg", dark ? "#191919" : "#fcfaf5");
  const card = wardianColor("--color-wardian-card", dark ? "#212121" : "#f3f4f6");
  const text = wardianColor("--color-wardian-text", dark ? "#ececec" : "#111827");
  const muted = wardianColor("--color-wardian-text-muted-neutral", dark ? "#a9a9a9" : "#4b5563");
  const accent = wardianColor("--color-wardian-accent", dark ? "#f2c14e" : "#926a09");
  const border = wardianColor("--color-wardian-border", dark ? "#2f2f2f" : "#e5e7eb");
  const color = (value: string) => value.replace(/^#/, "");
  monaco.editor.defineTheme(fileMonacoThemeName(), {
    base: dark ? "vs-dark" : "vs",
    inherit: true,
    rules: [
      { token: "comment", foreground: color(muted), fontStyle: "italic" },
      { token: "keyword", foreground: color(accent) },
      { token: "string", foreground: color(dark ? "#b7d990" : "#397a46") },
      { token: "number", foreground: color(dark ? "#d6a8ff" : "#7c3f9e") },
      { token: "type", foreground: color(dark ? "#77d7ea" : "#007f91") },
    ],
    colors: {
      "editor.background": background,
      "editor.foreground": text,
      "editor.lineHighlightBackground": card,
      "editorLineNumber.foreground": muted,
      "editorLineNumber.activeForeground": accent,
      "editorCursor.foreground": accent,
      "editor.selectionBackground": `${accent}3d`,
      "editor.inactiveSelectionBackground": `${accent}24`,
      "editorIndentGuide.background1": border,
      "editorIndentGuide.activeBackground1": accent,
      "editorBracketHighlight.foreground1": accent,
      "editorWidget.background": card,
      "editorWidget.border": border,
      "editorGutter.background": background,
    },
  });
}
