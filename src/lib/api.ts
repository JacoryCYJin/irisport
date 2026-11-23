import { invoke } from "@tauri-apps/api/core";

export interface SessionView {
  session_id: string;
  source: string;
  source_label: string;
  session_title: string | null;
  prompt_excerpt: string | null;
  last_prompt_at: number | null;
  last_result_at: number | null;
  result_seq: number;
  unread: boolean;
  pending: boolean;
  state: "running" | "completed" | "failed" | "interrupted";
  updated_at: number;
}

export interface SourceStatus {
  source: string;
  label: string;
  description: string;
  installed: boolean;
  configured: boolean;
  update_available: boolean;
  access_verified: boolean;
  supports_completion: boolean;
  supports_open: boolean;
  supports_auto_read: boolean;
  last_event_at: number | null;
  rejected_count: number;
  config_path: string;
}

export interface AppStatus {
  sources: SourceStatus[];
  data_path: string;
  version: string;
}

export interface Preferences {
  show_unread_count: boolean;
  notifications_enabled: boolean;
  notification_sound: boolean;
  show_excerpt_in_notification: boolean;
}

export type SettingsSection = "general" | "notifications" | "sources" | "data";

export interface OpenResult {
  ok: boolean;
  message: string | null;
}

export const api = {
  listSessions: () => invoke<SessionView[]>("list_sessions"),
  openSession: (session: Pick<SessionView, "source" | "session_id">) =>
    invoke<OpenResult>("open_session", {
      source: session.source,
      sessionId: session.session_id,
    }),
  installSource: (source: string) =>
    invoke<AppStatus>("install_source", { source }),
  uninstallSource: (source: string) =>
    invoke<AppStatus>("uninstall_source", { source }),
  appStatus: () => invoke<AppStatus>("app_status"),
  preferences: () => invoke<Preferences>("preferences"),
  setPreference: (key: keyof Preferences, value: boolean) =>
    invoke<void>("set_preference", { key, value }),
  hidePanel: () => invoke<void>("hide_panel"),
  showPanel: () => invoke<void>("show_panel"),
  setPanelHeight: (height: number) =>
    invoke<void>("set_panel_height", { height }),
  openSettings: (section?: SettingsSection) =>
    invoke<void>("open_settings", { section }),
  openSource: (source: string) => invoke<void>("open_source", { source }),
  revealPath: (target: "app-data" | "codex-hooks" | "kimi-config") =>
    invoke<void>("reveal_path", { target }),
};
