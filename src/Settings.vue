<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, type HookStatus } from "./lib/api";
import { formatRelative } from "./lib/time";

const status = ref<HookStatus | null>(null);
const busy = ref(false);
const message = ref("");
const messageTone = ref<"info" | "error">("info");

let unlisteners: UnlistenFn[] = [];

const configured = computed(() => status.value?.configured ?? false);
const verified = computed(() => status.value?.access_verified ?? false);

const connectionLabel = computed(() => {
  if (!status.value) return "检查中";
  if (!status.value.configured) return "未连接";
  if (!status.value.access_verified) return "等待信任";
  return "已连接";
});

const connectionTone = computed(() => {
  if (!status.value || !status.value.configured) return "idle";
  if (!status.value.access_verified) return "wait";
  return "ok";
});

const lastEvent = computed(() => {
  const value = status.value?.last_event_at;
  return value ? formatRelative(value) : "暂无";
});

function setMessage(value: string, tone: "info" | "error" = "info") {
  message.value = value;
  messageTone.value = tone;
}

async function refresh() {
  try {
    status.value = await api.hookStatus();
  } catch (error) {
    setMessage(`读取状态失败：${String(error)}`, "error");
  }
}

async function install() {
  busy.value = true;
  setMessage("");
  try {
    status.value = await api.installHooks();
    if (status.value.configured && !status.value.access_verified) {
      setMessage("配置已写入。请在 Codex 设置 → 钩子 中信任 Irisport。");
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

async function openCodexSettings() {
  try {
    await api.openCodexSettings();
  } catch (error) {
    setMessage(`无法打开 Codex 设置：${String(error)}`, "error");
  }
}

onMounted(async () => {
  await refresh();
  unlisteners.push(await listen("hook-status-updated", refresh));
});

onUnmounted(() => {
  unlisteners.forEach((unlisten) => unlisten());
  unlisteners = [];
});
</script>

<template>
  <main class="settings-page">
    <header class="settings-head">
      <h1>设置</h1>
      <p>Irisport {{ status?.version ?? "0.1.0" }}</p>
    </header>

    <section class="settings-group">
      <h2>来源</h2>
      <div class="settings-list">
        <div class="settings-item source-item">
          <div>
            <div class="item-title">Codex 桌面端</div>
            <div class="item-sub">通过本地 hooks 监听请求与结果</div>
          </div>
          <span class="source-state" :class="connectionTone">
            <span class="dot"></span>
            {{ connectionLabel }}
          </span>
        </div>
      </div>

      <div class="settings-actions">
        <button v-if="!configured" class="button primary" :disabled="busy" @click="install">
          连接 Codex
        </button>
        <template v-else>
          <button class="button" :disabled="busy" @click="refresh">重新检查</button>
          <button class="button" :disabled="busy" @click="uninstall">断开 Codex</button>
        </template>
        <button class="link" type="button" @click="openCodexSettings">打开 Codex 设置</button>
      </div>

      <p v-if="configured && !verified" class="settings-note warn">
        配置已写入但尚未收到可验证事件。请在 Codex「设置 → 钩子」中信任 Irisport，并发送一次请求。
      </p>
      <p class="settings-note">
        当前只接入 Codex 桌面端；更多 Agent、ChatGPT 网页和 Kimi 等来源将在后续版本接入。
      </p>
    </section>

    <section class="settings-group">
      <h2>接入状态</h2>
      <div class="settings-list">
        <div class="settings-item">
          <span>配置</span>
          <span>{{ configured ? "已写入" : "未写入" }}</span>
        </div>
        <div class="settings-item">
          <span>接入验证</span>
          <span>{{ verified ? "已验证" : "等待真实事件" }}</span>
        </div>
        <div class="settings-item">
          <span>最近事件</span>
          <span>{{ lastEvent }}</span>
        </div>
        <div class="settings-item">
          <span>来源未通过校验</span>
          <span>{{ status?.rejected_count ?? 0 }}</span>
        </div>
      </div>
    </section>

    <section class="settings-group">
      <h2>数据</h2>
      <div class="settings-list">
        <div class="settings-item data-item">
          <span>应用数据</span>
          <code>{{ status?.data_path ?? "…" }}</code>
        </div>
        <div class="settings-item data-item">
          <span>Codex hooks</span>
          <code>{{ status?.hooks_path ?? "…" }}</code>
        </div>
      </div>
      <p class="settings-note">
        面板只显示待看结果；已读记录最多保留 7 天或 50 条。完整对话与回复正文留在原工具。
      </p>
    </section>

    <p v-if="message" class="message" :class="messageTone">{{ message }}</p>
  </main>
</template>
