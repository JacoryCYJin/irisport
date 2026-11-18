import { invoke } from "@tauri-apps/api/core";

export interface SessionView {
  session_id: string;
  source: string;
  prompt_excerpt: string | null;
  last_prompt_at: number | null;
  last_result_at: number | null;
  result_seq: number;
  unread: boolean;
  pending: boolean;
  updated_at: number;
}

export interface HookStatus {
  configured: boolean;
  access_verified: boolean;
  last_event_at: number | null;
  rejected_count: number;
  hooks_path: string;
  hook_installed: boolean;
}

export interface OpenResult {
  ok: boolean;
  cleared: boolean;
  kept_due_to_new_result: boolean;
  message: string | null;
}

export const api = {
  listSessions: () => invoke<SessionView[]>("list_sessions"),
  openSession: (sessionId: string, expectedResultSeq: number) =>
    invoke<OpenResult>("open_session", {
      sessionId,
      expectedResultSeq,
    }),
  installHooks: () => invoke<HookStatus>("install_codex_hooks"),
  uninstallHooks: () => invoke<HookStatus>("uninstall_codex_hooks"),
  hookStatus: () => invoke<HookStatus>("hook_status"),
  hidePanel: () => invoke<void>("hide_panel"),
  openCodexSettings: () => invoke<void>("open_codex_settings"),
};
