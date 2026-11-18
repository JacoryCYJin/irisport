<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import SessionRow from "./components/SessionRow.vue";
import { api, type HookStatus, type SessionView } from "./lib/api";

const sessions = ref<SessionView[]>([]);
const status = ref<HookStatus | null>(null);
const busy = ref(false);
const message = ref("");
const messageTone = ref<"info" | "error">("info");
const listContent = ref<HTMLElement | null>(null);
const messageRef = ref<HTMLElement | null>(null);

let unlisteners: UnlistenFn[] = [];

const version = computed(() => status.value?.version ?? "0.1.0");

async function refreshSessions() {
  try {
    sessions.value = await api.listSessions();
  } catch (error) {
    setMessage(`读取列表失败：${String(error)}`, "error");
  }
}

async function refreshStatus() {
  try {
    status.value = await api.hookStatus();
  } catch (error) {
    setMessage(`读取状态失败：${String(error)}`, "error");
  }
}

function setMessage(value: string, tone: "info" | "error" = "info") {
  message.value = value;
  messageTone.value = tone;
}

async function fitPanelHeight() {
  await nextTick();
  const content = listContent.value;
  if (!content) return;
  const messageHeight = messageRef.value ? messageRef.value.offsetHeight + 8 : 0;
  const target = Math.min(
    480,
    Math.max(100, content.scrollHeight + 16 + 1 + 44 + messageHeight),
  );
  try {
    await api.setPanelHeight(Math.round(target));
  } catch {
    // Resizing is cosmetic; ignore transient failures.
  }
}

watch([sessions, message], () => {
  void fitPanelHeight();
});

async function openSession(session: SessionView) {
  if (busy.value) return;
  busy.value = true;
  try {
    const result = await api.openSession(session.session_id);
    if (!result.ok) {
      setMessage(result.message || "打开失败，提醒已保留。", "error");
    } else {
      setMessage("");
    }
    await Promise.all([refreshSessions(), refreshStatus()]);
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
  }
}

onMounted(async () => {
  await Promise.all([refreshSessions(), refreshStatus()]);
  await fitPanelHeight();
  unlisteners.push(await listen("sessions-updated", refreshSessions));
  unlisteners.push(await listen("hook-status-updated", refreshStatus));
  unlisteners.push(
    await listen("panel-opened", async () => {
      await Promise.all([refreshSessions(), refreshStatus()]);
      await fitPanelHeight();
    }),
  );
  window.addEventListener("keydown", onKeydown);
});

onUnmounted(() => {
  unlisteners.forEach((unlisten) => unlisten());
  unlisteners = [];
  window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <main class="panel">
    <section class="list">
      <div ref="listContent" class="list-content">
        <div v-if="sessions.length === 0" class="empty">
          <p class="empty-title">暂无待查看的结果</p>
        </div>
        <SessionRow
          v-for="session in sessions"
          v-else
          :key="session.session_id"
          :session="session"
          :busy="busy"
          @open="openSession"
        />
      </div>
    </section>

    <p v-if="message" ref="messageRef" class="message" :class="messageTone">{{ message }}</p>

    <div class="settings-divider" aria-hidden="true"></div>

    <button class="settings-row" type="button" @click="openSettings">
      <svg class="gear" viewBox="0 0 16 16" aria-hidden="true">
        <path
          d="M6.9 1.6h2.2l.3 1.5.9.4 1.2-.9 1.6 1.6-.9 1.2.4.9 1.5.3v2.2l-1.5.3-.4.9.9 1.2-1.6 1.6-1.2-.9-.9.4-.3 1.5H6.9l-.3-1.5-.9-.4-1.2.9-1.6-1.6.9-1.2-.4-.9-1.5-.3V6.6l1.5-.3.4-.9-.9-1.2 1.6-1.6 1.2.9.9-.4.3-1.5Z"
          fill="none"
          stroke="currentColor"
          stroke-width="1"
          stroke-linejoin="round"
        />
        <circle cx="8" cy="8" r="2.1" fill="none" stroke="currentColor" stroke-width="1" />
      </svg>
      <span class="settings-label">设置</span>
      <span class="settings-version">{{ version }}</span>
    </button>
  </main>
</template>
