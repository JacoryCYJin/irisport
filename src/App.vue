<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import SessionRow from "./components/SessionRow.vue";
import StatusBar from "./components/StatusBar.vue";
import { api, type HookStatus, type SessionView } from "./lib/api";

const sessions = ref<SessionView[]>([]);
const status = ref<HookStatus | null>(null);
const busy = ref(false);
const message = ref("");
const messageTone = ref<"info" | "error">("info");

let unlisteners: UnlistenFn[] = [];

const unreadCount = computed(
  () => sessions.value.filter((session) => session.unread).length,
);
const configured = computed(() => status.value?.configured ?? false);
const verified = computed(() => status.value?.access_verified ?? false);

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
    setMessage(`读取连接状态失败：${String(error)}`, "error");
  }
}

function setMessage(value: string, tone: "info" | "error" = "info") {
  message.value = value;
  messageTone.value = tone;
}

async function install() {
  busy.value = true;
  setMessage("");
  try {
    status.value = await api.installHooks();
    if (status.value.configured && !status.value.access_verified) {
      setMessage("已写入配置。请在 Codex 设置 → 钩子 中信任 Irisport。");
    }
  } catch (error) {
    setMessage(`写入配置失败：${String(error)}`, "error");
  } finally {
    busy.value = false;
  }
}

async function uninstall() {
  busy.value = true;
  try {
    status.value = await api.uninstallHooks();
    setMessage("已断开 Codex，其他 hooks 未被修改。");
  } catch (error) {
    setMessage(`断开失败：${String(error)}`, "error");
  } finally {
    busy.value = false;
  }
}

async function openSession(session: SessionView) {
  if (busy.value) return;
  busy.value = true;
  try {
    const result = await api.openSession(session.session_id, session.result_seq);
    if (!result.ok) {
      setMessage(result.message || "打开失败，提醒已保留。", "error");
    } else if (result.kept_due_to_new_result) {
      setMessage("回跳期间收到了新结果，提醒已保留。");
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

async function openCodexSettings() {
  try {
    await api.openCodexSettings();
  } catch (error) {
    setMessage(`无法打开 Codex 设置：${String(error)}`, "error");
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
  unlisteners.push(await listen("sessions-updated", refreshSessions));
  unlisteners.push(await listen("hook-status-updated", refreshStatus));
  unlisteners.push(
    await listen("panel-opened", async () => {
      await Promise.all([refreshSessions(), refreshStatus()]);
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
    <header class="header">
      <div>
        <h1 class="title">Irisport</h1>
        <p class="subtitle">Codex 任务提醒</p>
      </div>
      <div class="summary">
        <span v-if="unreadCount > 0" class="badge">{{ unreadCount }}</span>
        <span class="summary-text">
          {{ unreadCount > 0 ? "个新结果" : "暂无新结果" }}
        </span>
      </div>
    </header>

    <StatusBar :status="status" />

    <section class="list">
      <div v-if="sessions.length === 0" class="empty">
        <div class="empty-icon">
          <svg viewBox="0 0 32 32" aria-hidden="true">
            <circle cx="16" cy="16" r="9.5" fill="none" stroke="currentColor" stroke-width="1.8" />
            <circle cx="16" cy="16" r="3.2" fill="currentColor" />
            <path d="M16 3.5v3M16 25.5v3" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
          </svg>
        </div>
        <p class="empty-title">还没有 Codex 任务</p>
        <p class="empty-copy">连接后照常发任务，结果会出现在这里。</p>
        <div class="empty-steps">
          <div class="empty-step"><span class="step-mark"></span>连接 Codex，安装本地 hooks</div>
          <div class="empty-step"><span class="step-mark"></span>在 Codex 设置 → 钩子 中信任 Irisport</div>
          <div class="empty-step"><span class="step-mark"></span>打开面板即视为已查看</div>
          <div class="empty-step"><span class="step-mark"></span>点击任务行可回到原聊天</div>
        </div>
        <p class="empty-note">完整对话和回复正文留在 Codex。</p>
      </div>
      <SessionRow
        v-for="session in sessions"
        v-else
        :key="session.session_id"
        :session="session"
        :busy="busy"
        @open="openSession"
      />
    </section>

    <footer class="footer">
      <p v-if="message" class="message" :class="messageTone">{{ message }}</p>

      <div v-if="configured && !verified" class="trust-hint">
        配置已写入但尚未收到可验证事件。请确认已在 Codex「设置 → 钩子」中信任 Irisport，并发送一次请求。
      </div>

      <div class="actions">
        <button v-if="!configured" class="button primary" :disabled="busy" @click="install">
          连接 Codex
        </button>
        <template v-else>
          <button class="button" :disabled="busy" @click="refreshStatus">
            重新检查
          </button>
          <button class="button" :disabled="busy" @click="uninstall">
            断开 Codex
          </button>
        </template>
        <button class="link" type="button" @click="openCodexSettings">Codex 设置</button>
      </div>
    </footer>
  </main>
</template>
