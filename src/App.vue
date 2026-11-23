<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  isPermissionGranted,
  onAction,
  registerActionTypes,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import AppIcon from "./components/AppIcon.vue";
import SessionRow from "./components/SessionRow.vue";
import { api, type Preferences, type SessionView } from "./lib/api";

const sessions = ref<SessionView[]>([]);
const preferences = ref<Preferences | null>(null);
const busy = ref(false);
const message = ref("");
const messageTone = ref<"info" | "error">("info");
const panelRef = ref<HTMLElement | null>(null);
const now = ref(Date.now());

let unlisteners: UnlistenFn[] = [];
let notificationListener: { unregister: () => Promise<void> } | null = null;
let clockTimer: ReturnType<typeof setInterval> | null = null;

async function refreshSessions() {
  try {
    sessions.value = await api.listSessions();
  } catch (error) {
    setMessage(`读取列表失败：${String(error)}`, "error");
  }
}

async function refreshPreferences() {
  try {
    preferences.value = await api.preferences();
  } catch (error) {
    setMessage(`读取偏好失败：${String(error)}`, "error");
  }
}

function setMessage(value: string, tone: "info" | "error" = "info") {
  message.value = value;
  messageTone.value = tone;
}

async function fitPanelHeight() {
  await nextTick();
  const panel = panelRef.value;
  if (!panel) return;
  const content = panel.querySelector<HTMLElement>(".list-content");
  const footer = panel.querySelector<HTMLElement>(".panel-footer");
  const statusMessage = panel.querySelector<HTMLElement>(".message");
  if (!content || !footer) return;
  const target =
    content.scrollHeight +
    16 +
    footer.offsetHeight +
    (statusMessage?.offsetHeight ?? 0) +
    2;
  try {
    await api.setPanelHeight(Math.round(Math.min(520, Math.max(120, target))));
  } catch {}
}

watch([sessions, message], () => {
  void fitPanelHeight();
});

async function openSession(session: SessionView) {
  if (busy.value) return;
  busy.value = true;
  try {
    const result = await api.openSession(session);
    if (!result.ok) {
      setMessage(result.message || "打开失败，提醒已保留。", "error");
    } else {
      setMessage("");
    }
    await refreshSessions();
  } catch (error) {
    setMessage(`打开失败，提醒已保留：${String(error)}`, "error");
  } finally {
    busy.value = false;
  }
}

async function openSettings() {
  try {
    await api.openSettings();
    await api.hidePanel();
  } catch (error) {
    setMessage(`无法打开设置：${String(error)}`, "error");
  }
}

async function hide() {
  await api.hidePanel();
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    void hide();
    return;
  }
  if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
  const rows = Array.from(document.querySelectorAll<HTMLButtonElement>(".session"));
  if (rows.length === 0) return;
  event.preventDefault();
  const current = rows.indexOf(document.activeElement as HTMLButtonElement);
  const direction = event.key === "ArrowDown" ? 1 : -1;
  const next = current < 0 ? (direction > 0 ? 0 : rows.length - 1) : (current + direction + rows.length) % rows.length;
  rows[next]?.focus();
}

async function notifyResult(session: SessionView) {
  try {
    const prefs = preferences.value;
    if (!prefs?.notifications_enabled || !(await isPermissionGranted())) return;
    const title = session.session_title?.trim() || session.prompt_excerpt?.trim() || "Agent 任务已完成";
    sendNotification({
      title: `${session.source_label} · ${title}`,
      body: prefs.show_excerpt_in_notification
        ? session.prompt_excerpt?.trim() || "新结果等待查看"
        : "新结果等待查看",
      sound: prefs.notification_sound ? "Ping" : undefined,
      actionTypeId: "agent-result",
      extra: { source: session.source, sessionId: session.session_id },
    });
  } catch {}
}

async function setupNotifications() {
  try {
    await registerActionTypes([
      {
        id: "agent-result",
        actions: [{ id: "open", title: "查看结果" }],
      },
    ]);
    notificationListener = await onAction((event) => {
      void (async () => {
        const source = event.notification?.extra.source;
        const sessionId = event.notification?.extra.sessionId;
        if (typeof source === "string" && typeof sessionId === "string") {
          if (source !== "codex") {
            await api.showPanel();
            return;
          }
          const result = await api.openSession({ source, session_id: sessionId });
          if (!result.ok) {
            setMessage(result.message || "打开失败，提醒已保留。", "error");
            await api.showPanel();
          }
          await refreshSessions();
        } else {
          await api.showPanel();
        }
      })();
    });
  } catch {}
}

onMounted(async () => {
  await Promise.all([refreshSessions(), refreshPreferences()]);
  await fitPanelHeight();
  unlisteners.push(await listen("sessions-updated", refreshSessions));
  unlisteners.push(await listen("preferences-updated", refreshPreferences));
  unlisteners.push(await listen<SessionView>("task-completed", (event) => {
    void notifyResult(event.payload);
  }));
  unlisteners.push(
    await listen("panel-opened", async () => {
      await Promise.all([refreshSessions(), refreshPreferences()]);
      await fitPanelHeight();
    }),
  );
  await setupNotifications();
  clockTimer = setInterval(() => {
    now.value = Date.now();
  }, 1000);
  window.addEventListener("keydown", onKeydown);
});

onUnmounted(() => {
  unlisteners.forEach((unlisten) => unlisten());
  unlisteners = [];
  if (notificationListener) void notificationListener.unregister();
  if (clockTimer) clearInterval(clockTimer);
  window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <main ref="panelRef" class="panel">
    <section class="list" aria-label="Agent 任务与待查看结果">
      <div class="list-content">
        <div v-if="sessions.length === 0" class="empty">
          <p class="empty-title">暂无待查看的结果</p>
        </div>
        <SessionRow
          v-for="session in sessions"
          v-else
          :key="`${session.source}:${session.session_id}`"
          :session="session"
          :busy="busy"
          :now="now"
          @open="openSession"
        />
      </div>
    </section>

    <p v-if="message" class="message" :class="messageTone" role="status">{{ message }}</p>

    <footer class="panel-footer">
      <button class="settings-row" type="button" @click="openSettings">
        <AppIcon class="gear" name="gear" />
        <span>设置</span>
      </button>
    </footer>
  </main>
</template>
