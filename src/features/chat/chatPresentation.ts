import { FileText, GitCompare, ListChecks, Search, ShieldAlert, Terminal, Wrench } from "lucide-react";
import type { LucideIcon } from "lucide-react";

import type { AgentChatEvent } from "../../types";
import { isGenericActivityTitle, type ActivityBlockModel, type ActivityTone } from "../grid/activityBlocks";
import { shouldShowStatusEvent, type PresentedWorkEntry } from "../grid/workLogPresentation";

/**
 * Shared transcript presentation helpers.
 *
 * The desktop grid chat and the remote mobile chat render the same normalized
 * `AgentChatEvent` stream. They previously carried independent copies of every
 * helper below, which drifted: tone classes, tool classification, and changed
 * file chips diverged surface by surface. This module is the single source for
 * how an event is classified and labelled; `ChatTranscriptRows.tsx` is the
 * single source for how it is drawn.
 */

export type ToolDisplayKind = "diff" | "file" | "permission" | "search" | "shell" | "todo" | "generic";

export interface ToolPresentation {
  kind: ToolDisplayKind;
  label: string;
  title: string;
  details: string[];
  icon: LucideIcon;
}

export const TONE_CLASSES: Record<ActivityTone, string> = {
  error: "border-[var(--color-wardian-error)]",
  neutral: "border-wardian-light",
  processing: "border-[var(--color-wardian-processing)]",
  success: "border-[var(--color-wardian-success)]",
  warning: "border-[var(--color-wardian-warning)]",
};

export function toneDotClass(tone: ActivityTone): string {
  if (tone === "error") return "bg-[var(--color-wardian-error)]";
  if (tone === "warning") return "bg-[var(--color-wardian-warning)]";
  if (tone === "processing") return "bg-[var(--color-wardian-processing)]";
  if (tone === "success") return "bg-[var(--color-wardian-success)]";
  return "bg-[var(--color-wardian-text-muted)]";
}

export function toolIconClass(kind: ToolDisplayKind): string {
  if (kind === "permission") {
    return "border-[color-mix(in_srgb,var(--color-wardian-warning),transparent_42%)] bg-[color-mix(in_srgb,var(--color-wardian-warning),transparent_88%)] text-[var(--color-wardian-warning)]";
  }
  if (kind === "diff") {
    return "border-[color-mix(in_srgb,var(--color-wardian-success),transparent_45%)] bg-[color-mix(in_srgb,var(--color-wardian-success),transparent_88%)] text-[var(--color-wardian-success)]";
  }
  if (kind === "shell") {
    return "border-[color-mix(in_srgb,var(--color-wardian-processing),transparent_42%)] bg-[color-mix(in_srgb,var(--color-wardian-processing),transparent_88%)] text-[var(--color-wardian-processing)]";
  }
  return "border-wardian-light bg-[var(--color-wardian-card)] text-muted-neutral";
}

export function toolPresentation(event: AgentChatEvent, block: ActivityBlockModel): ToolPresentation {
  const rawType = stringMetadata(event.metadata, "raw_type");
  const toolName = toolNameFromEvent(event);
  const haystack = [event.kind, event.title, event.source, event.command, rawType, toolName, event.path, block.language]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  const details = [
    toolLabelFromEvent(event, rawType, toolName),
    formatStatus(event.status),
    event.path ? compactPath(event.path) : null,
    typeof event.exit_code === "number" ? `exit ${event.exit_code}` : null,
    `${block.lineCount} ${block.lineCount === 1 ? "line" : "lines"}`,
  ].filter((detail): detail is string => Boolean(detail?.trim()));

  if (event.kind === "approval" || event.status === "action_required") {
    return { kind: "permission", label: "Permission", title: readableToolTitle(event, "Permission required"), details, icon: ShieldAlert };
  }

  if (haystack.includes("todo")) {
    return { kind: "todo", label: "Todo", title: readableToolTitle(event, "Todo update"), details, icon: ListChecks };
  }

  if (block.language === "diff" || /\b(apply_patch|patch|diff|edit|write)\b/.test(haystack)) {
    return { kind: "diff", label: "Change", title: readableToolTitle(event, "File change"), details, icon: GitCompare };
  }

  if (event.command?.trim() || /\b(bash|shell|exec|command|powershell|pwsh|cmd)\b/.test(haystack)) {
    return { kind: "shell", label: "Shell", title: readableToolTitle(event, "Shell command"), details, icon: Terminal };
  }

  if (/\b(search|grep|glob|rg|find|webfetch|websearch)\b/.test(haystack)) {
    return { kind: "search", label: "Search", title: readableToolTitle(event, "Search"), details, icon: Search };
  }

  if (event.path || /\b(read|file|filesystem)\b/.test(haystack)) {
    return { kind: "file", label: "File", title: readableToolTitle(event, "File operation"), details, icon: FileText };
  }

  return { kind: "generic", label: "Tool", title: readableToolTitle(event, block.title || "Tool activity"), details, icon: Wrench };
}

export function readableToolTitle(event: AgentChatEvent, fallback: string): string {
  const title = event.title?.trim();
  const toolName = toolNameFromEvent(event);
  const command = event.command?.trim();
  if (title && !isGenericActivityTitle(title)) return title.replace(/_/g, " ");
  if (toolName) return toolName.replace(/_/g, " ");
  if (command) return commandName(command);
  return fallback;
}

function commandName(command: string): string {
  const first = command.trim().split(/\s+/)[0];
  if (!first) return "Shell command";
  return first.replace(/\.(exe|cmd|ps1)$/i, "");
}

function toolLabelFromEvent(event: AgentChatEvent, rawType: string | null, toolName: string | null): string | null {
  const title = event.title?.trim();
  if (title && !isGenericActivityTitle(title)) return title.replace(/_/g, " ");
  if (toolName) return toolName.replace(/_/g, " ");
  if (rawType) return rawType.replace(/_/g, " ");
  if (event.kind === "tool_call") return "tool call";
  if (event.kind === "tool_result") return "tool result";
  return null;
}

export function toolNameFromEvent(event: AgentChatEvent): string | null {
  return (
    stringMetadata(event.metadata, "tool_name") ||
    stringMetadata(event.metadata, "function_name") ||
    stringMetadata(event.metadata, "name") ||
    stringMetadata(event.metadata, "tool")
  );
}

export function stringMetadata(metadata: Record<string, unknown>, key: string): string | null {
  const value = metadata[key];
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

export function outputWithoutCommandPrefix(content: string, command: string | null): string {
  if (!command?.trim()) return content;
  const escaped = command.trim().replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return content.replace(new RegExp(`^\\$\\s+${escaped}\\s*(?:\\r?\\n){1,2}`), "").trimEnd();
}

export function parseTodoItems(content: string): Array<{ done: boolean; label: string }> {
  return content
    .replace(/\r\n|\r/g, "\n")
    .split("\n")
    .map((line) => {
      const checkbox = /^\s*(?:[-*]\s*)?\[([ xX])\]\s+(.+)$/.exec(line);
      if (checkbox) return { done: checkbox[1].toLowerCase() === "x", label: checkbox[2].trim() };
      const prefixed = /^\s*(?:done|completed|pending|todo|in_progress|in progress)\s*[:-]\s*(.+)$/i.exec(line);
      if (prefixed) return { done: /^(done|completed)/i.test(line.trim()), label: prefixed[1].trim() };
      return null;
    })
    .filter((item): item is { done: boolean; label: string } => Boolean(item?.label));
}

export function diffStats(content: string): { added: number; removed: number; files: string[] } {
  const files = new Set<string>();
  let added = 0;
  let removed = 0;

  content.split(/\r\n|\r|\n/).forEach((line) => {
    if (/^\+[^+]/.test(line)) added += 1;
    if (/^-[^-]/.test(line)) removed += 1;
    const diffFile = /^diff --git a\/(.+?) b\/(.+)$/.exec(line);
    if (diffFile) files.add(diffFile[2]);
    const patchFile = /^(\*\*\* (?:Add|Update|Delete) File:\s+)(.+)$/.exec(line);
    if (patchFile) files.add(patchFile[2].trim());
  });

  return { added, removed, files: [...files] };
}

export function compactPath(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  if (parts.length <= 2) return path;
  return `.../${parts.slice(-2).join("/")}`;
}

export function previewContent(content: string): string {
  const lines = content.split(/\r\n|\r|\n/);
  const linePreview = lines.slice(0, 6).join("\n");
  const charPreview = linePreview.slice(0, 900);
  return `${charPreview}\n\nOutput collapsed; show output to inspect all lines.`;
}

export function compactTerminalPreview(content: string): string {
  return content
    .split(/\r\n|\r|\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(0, 2)
    .join("  ");
}

export function formatStatus(status: AgentChatEvent["status"]): string | null {
  if (!status) return null;
  return status.replace(/_/g, " ");
}

export function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function workGroupTitleFromEntries(entries: PresentedWorkEntry[]): string {
  if (entries.some((entry) => entry.primary_event.kind === "error" || entry.primary_event.status === "failed")) {
    return "Work log with error";
  }
  if (entries.some((entry) => entry.primary_event.status === "action_required")) return "Work log needs attention";
  return "Work log";
}

export function isThinkingIndicator(event: AgentChatEvent): boolean {
  return event.kind === "status" && event.metadata?.chat_thinking_indicator === true;
}

export function hasMeaningfulToolIdentity(event: AgentChatEvent): boolean {
  const title = event.title?.trim();
  if (title && !isGenericActivityTitle(title)) return true;
  return Boolean(toolNameFromEvent(event));
}

export function sortTranscriptEvents(events: AgentChatEvent[]): AgentChatEvent[] {
  return [...events].sort((a, b) => {
    if (typeof a.sequence === "number" && typeof b.sequence === "number" && a.sequence !== b.sequence) {
      return a.sequence - b.sequence;
    }

    const aTime = Date.parse(a.created_at ?? "");
    const bTime = Date.parse(b.created_at ?? "");
    if (!Number.isNaN(aTime) && !Number.isNaN(bTime) && aTime !== bTime) {
      return aTime - bTime;
    }

    return 0;
  });
}

/**
 * A tool call with no command, no text, and no recognizable identity is a
 * provider keepalive rather than work worth a row. Everything else is shown,
 * except status events, which only earn a row when they carry a failure or the
 * synthetic thinking indicator.
 */
export function shouldShowChatEvent(event: AgentChatEvent): boolean {
  if (
    event.kind === "tool_call" &&
    !event.command?.trim() &&
    !event.text?.trim() &&
    !hasMeaningfulToolIdentity(event) &&
    (event.status === "running" || event.status === "processing")
  ) {
    return false;
  }
  if (event.kind !== "status") return true;
  if (isThinkingIndicator(event)) return true;
  return shouldShowStatusEvent(event);
}
