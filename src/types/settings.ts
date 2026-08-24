export interface ShellOption {
  id: string;
  label: string;
  executable: string;
  default_args: string[];
}

export type CodexSandboxMode = 'read-only' | 'workspace-write' | 'danger-full-access';
export type CodexApprovalPolicy = 'untrusted' | 'on-request' | 'never';
export type DefaultProviderSetting = 'auto' | 'claude' | 'codex' | 'gemini' | 'antigravity' | 'opencode' | 'pi';
export type ConversationLoggingSetting = 'enabled' | 'disabled';
export type AppThemeSetting = 'dark' | 'light' | 'system';
export type WatchlistNewAgentPosition = 'top' | 'bottom';
export type ExternalEditorSetting = 'system' | 'vscode' | 'custom';
export type FileOpenAction = 'wardian' | 'external';
export type FileOpenKind = 'text' | 'image' | 'pdf';

export interface FileOpenActions {
  text: FileOpenAction;
  image: FileOpenAction;
  pdf: FileOpenAction;
}

export const DEFAULT_FILE_OPEN_ACTIONS: FileOpenActions = {
  text: 'wardian',
  image: 'wardian',
  pdf: 'wardian',
};

export type WorkbenchNewTabAction = 'home' | 'palette';

export interface CodexRuntimePolicy {
  sandbox_mode: CodexSandboxMode;
  approval_policy: CodexApprovalPolicy;
  full_auto: boolean;
  trust_workspaces: boolean;
}

export interface CodexRuntimePolicyOverrides {
  sandbox_mode?: CodexSandboxMode;
  approval_policy?: CodexApprovalPolicy;
  full_auto?: boolean;
  trust_workspaces?: boolean;
}

export interface ShellSettings {
  shell_id: string;
  custom_executable: string | null;
  custom_args: string | null;
  agent_session_persistence: 'fresh' | 'resume';
  codex_runtime_policy?: CodexRuntimePolicy;
  default_provider?: DefaultProviderSetting;
  conversation_logging?: ConversationLoggingSetting;
}

export interface ShellSettingsOverrides {
  shell_id?: string;
  custom_executable?: string | null;
  custom_args?: string | null;
  agent_session_persistence?: 'fresh' | 'resume';
  codex_runtime_policy?: CodexRuntimePolicyOverrides;
  default_provider?: DefaultProviderSetting;
  conversation_logging?: ConversationLoggingSetting;
}

export interface AppSettings {
  theme: AppThemeSetting;
  auto_patch_gemini: boolean;
  terminal_font_size: number;
  terminal_font_family: string | null;
  grid_card_display_mode: 'terminal' | 'chat';
  watchlist_new_agent_position: WatchlistNewAgentPosition;
  titlebar_telemetry_visible: boolean;
  external_editor: ExternalEditorSetting;
  external_editor_custom_executable: string | null;
  file_open_actions: FileOpenActions;
  workbench_new_tab_action: WorkbenchNewTabAction;
}

export interface AppSettingsOverrides {
  theme?: AppThemeSetting;
  auto_patch_gemini?: boolean;
  terminal_font_size?: number;
  terminal_font_family?: string | null;
  grid_card_display_mode?: 'terminal' | 'chat';
  watchlist_new_agent_position?: WatchlistNewAgentPosition;
  titlebar_telemetry_visible?: boolean;
  external_editor?: ExternalEditorSetting;
  external_editor_custom_executable?: string | null;
  file_open_actions?: FileOpenActions;
  workbench_new_tab_action?: WorkbenchNewTabAction;
}

export interface SettingsDocument<TSettings, TOverrides> {
  schema_version: 2;
  settings: TSettings;
  overrides: TOverrides;
  persisted?: boolean;
}
