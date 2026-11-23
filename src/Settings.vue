<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import AppIcon, { type IconName } from "./components/AppIcon.vue";
import SourceIcon from "./components/SourceIcon.vue";
import ToggleRow from "./components/ToggleRow.vue";
import {
  api,
  type AppStatus,
  type Preferences,
  type SettingsSection,
  type SourceStatus,
} from "./lib/api";
import { formatRelative } from "./lib/time";

const tabs: { id: SettingsSection; label: string; icon: IconName }[] = [
  { id: "general", label: "通用", icon: "gear" },
  { id: "notifications", label: "通知", icon: "bell" },
  { id: "sources", label: "来源", icon: "sources" },
  { id: "data", label: "数据", icon: "folder" },
];

const initialSection = new URLSearchParams(window.location.search).get("section");
const section = ref<SettingsSection>(
  tabs.some((tab) => tab.id === initialSection)
    ? (initialSection as SettingsSection)
    : "general",
);
const status = ref<AppStatus | null>(null);
const preferences = ref<Preferences>({
  show_unread_count: false,
  notifications_enabled: false,
  notification_sound: false,
  show_excerpt_in_notification: true,
});
const autostartEnabled = ref(false);
const selectedSource = ref<string | null>(null);
const busy = ref(false);
const message = ref("");
const messageTone = ref<"info" | "error">("info");

let unlisteners: UnlistenFn[] = [];

const sources = computed(() => status.value?.sources ?? []);
const activeSource = computed(() =>
  sources.value.find((source) => source.source === selectedSource.value),
);
const activeTab = computed(() => tabs.find((tab) => tab.id === section.value));

function setMessage(value: string, tone: "info" | "error" = "info") {
  message.value = value;
  messageTone.value = tone;
}

function navigate(value: SettingsSection) {
  section.value = value;
  if (value !== "sources") selectedSource.value = null;
  setMessage("");
}

function connectionLabel(source: SourceStatus) {
  if (!source.supports_completion) return source.installed ? "已安装" : "未安装";
  if (!source.installed) return "未安装";
  if (!source.configured) return "未连接";
  if (!source.access_verified) return "等待验证";
  return "已连接";
}

function connectionTone(source: SourceStatus) {
  if (source.configured && source.access_verified && source.supports_completion) return "ok";
  if (source.configured || source.installed) return "wait";
  return "idle";
}

function capabilityLabel(source: SourceStatus) {
  if (!source.supports_completion) return "仅支持打开应用";
  return source.supports_auto_read ? "任务状态 · 原应用查看后已读" : "任务状态 · 暂不支持已读";
}

function lastEvent(source: SourceStatus) {
  return source.last_event_at ? formatRelative(source.last_event_at) : "暂无";
}

async function refresh() {
  try {
    status.value = await api.appStatus();
  } catch (error) {
    setMessage(`读取来源状态失败：${String(error)}`, "error");
  }
}

async function refreshPreferences() {
  try {
    preferences.value = await api.preferences();
  } catch (error) {
    setMessage(`读取偏好失败：${String(error)}`, "error");
  }
}

async function install(source: SourceStatus) {
  busy.value = true;
  setMessage("");
  try {
    status.value = await api.installSource(source.source);
    setMessage(
      source.source === "codex"
        ? "Codex 配置已写入。如出现提示，请重新信任 Irisport hooks，然后发送一次任务。"
        : source.source === "workbuddy"
          ? "WorkBuddy 配置已写入。请开始一个新会话，并发送一次任务验证 hooks。"
          : "Kimi Code 配置已写入。请开始一个新会话，让 hooks 生效。",
    );
  } catch (error) {
    setMessage(`连接失败：${String(error)}`, "error");
  } finally {
    busy.value = false;
  }
}

async function uninstall(source: SourceStatus) {
  busy.value = true;
  setMessage("");
  try {
    status.value = await api.uninstallSource(source.source);
    setMessage(`已断开 ${source.label}，其他 hooks 未被修改。`);
  } catch (error) {
    setMessage(`断开失败：${String(error)}`, "error");
  } finally {
    busy.value = false;
  }
}

async function openSource(source: SourceStatus) {
  try {
    await api.openSource(source.source);
  } catch (error) {
    setMessage(`无法打开 ${source.label}：${String(error)}`, "error");
  }
}

async function reveal(target: "app-data" | "codex-hooks" | "kimi-config") {
  try {
    await api.revealPath(target);
  } catch (error) {
    setMessage(`无法在 Finder 中显示：${String(error)}`, "error");
  }
}

async function savePreference(key: keyof Preferences, value: boolean) {
  const previous = preferences.value[key];
  preferences.value[key] = value;
  try {
    await api.setPreference(key, value);
  } catch (error) {
    preferences.value[key] = previous;
    setMessage(`保存设置失败：${String(error)}`, "error");
  }
}

async function setAutostart(value: boolean) {
  busy.value = true;
  try {
    if (value) {
      await enable();
    } else {
      await disable();
    }
    autostartEnabled.value = await isEnabled();
  } catch (error) {
    setMessage(`登录启动设置失败：${String(error)}`, "error");
  } finally {
    busy.value = false;
  }
}

async function setNotifications(value: boolean) {
  try {
    if (value) {
      let granted = await isPermissionGranted();
      if (!granted) {
        granted = (await requestPermission()) === "granted";
      }
      if (!granted) {
        setMessage("系统通知权限未开启。可在 macOS 设置 → 通知 → Irisport 中允许。", "error");
        return;
      }
    }
    await savePreference("notifications_enabled", value);
  } catch (error) {
    setMessage(`系统通知设置失败：${String(error)}`, "error");
  }
}

onMounted(async () => {
  await Promise.all([refresh(), refreshPreferences()]);
  try {
    autostartEnabled.value = await isEnabled();
  } catch (error) {
    setMessage(`读取登录启动状态失败：${String(error)}`, "error");
  }
  unlisteners.push(await listen("source-status-updated", refresh));
  unlisteners.push(await listen("preferences-updated", refreshPreferences));
  unlisteners.push(
    await listen<string>("settings-navigate", (event) => {
      if (tabs.some((tab) => tab.id === event.payload)) {
        navigate(event.payload as SettingsSection);
      }
    }),
  );
});

onUnmounted(() => {
  unlisteners.forEach((unlisten) => unlisten());
  unlisteners = [];
});
</script>

<template>
  <main class="settings-page">
    <nav class="settings-tabs" aria-label="设置分类" role="tablist">
      <button
        v-for="tab in tabs"
        :key="tab.id"
        class="settings-tab"
        :class="{ active: section === tab.id }"
        type="button"
        role="tab"
        :aria-selected="section === tab.id"
        @click="navigate(tab.id)"
      >
        <AppIcon :name="tab.icon" />
        <span>{{ tab.label }}</span>
      </button>
    </nav>

    <div class="settings-content">
      <p v-if="message" class="settings-message" :class="messageTone" role="status">
        {{ message }}
      </p>

      <section v-if="section === 'general'" class="settings-section" role="tabpanel">
        <header class="section-heading">
          <h1>通用</h1>
          <p>调整 Irisport 在菜单栏中的显示和启动方式。</p>
        </header>
        <div class="settings-list">
          <ToggleRow
            label="登录时启动"
            description="登录 Mac 后在菜单栏后台运行 Irisport。"
            :model-value="autostartEnabled"
            :disabled="busy"
            @change="setAutostart"
          />
          <ToggleRow
            label="显示未读数量"
            description="在菜单栏图标旁显示待查看结果数。"
            :model-value="preferences.show_unread_count"
            @change="savePreference('show_unread_count', $event)"
          />
        </div>
        <div class="settings-list compact-list">
          <div class="settings-item">
            <span>运行平台</span>
            <span>macOS</span>
          </div>
          <div class="settings-item">
            <span>界面语言</span>
            <span>简体中文</span>
          </div>
        </div>
      </section>

      <section v-else-if="section === 'notifications'" class="settings-section" role="tabpanel">
        <header class="section-heading">
          <h1>通知</h1>
          <p>决定 Agent 完成结果如何提醒，不影响菜单栏中的待查看列表。</p>
        </header>
        <div class="settings-list">
          <ToggleRow
            label="完成时显示系统通知"
            description="使用 macOS 通知中心接收 Codex、Kimi Code 和 WorkBuddy 的新结果。"
            :model-value="preferences.notifications_enabled"
            @change="setNotifications"
          />
          <ToggleRow
            label="播放通知声音"
            description="使用 macOS 的 Ping 提示音。"
            :model-value="preferences.notification_sound"
            :disabled="!preferences.notifications_enabled"
            @change="savePreference('notification_sound', $event)"
          />
          <ToggleRow
            label="在通知中显示任务摘录"
            description="关闭后，通知只显示来源和结果状态。"
            :model-value="preferences.show_excerpt_in_notification"
            :disabled="!preferences.notifications_enabled"
            @change="savePreference('show_excerpt_in_notification', $event)"
          />
        </div>
        <p class="settings-note">
          系统通知的横幅、声音和专注模式规则也可在 macOS「设置 → 通知 → Irisport」中调整。
        </p>
      </section>

      <section v-else-if="section === 'sources'" class="settings-section" role="tabpanel">
        <template v-if="!activeSource">
          <header class="section-heading">
            <h1>来源</h1>
            <p>集中管理 Agent 的完成事件、已读方式和原会话回跳。</p>
          </header>
          <div class="settings-list source-list">
            <button
              v-for="source in sources"
              :key="source.source"
              class="source-row"
              type="button"
              @click="selectedSource = source.source"
            >
              <SourceIcon :source="source.source" />
              <span class="source-copy">
                <span class="item-title">{{ source.label }}</span>
                <span class="item-sub">{{ capabilityLabel(source) }}</span>
              </span>
              <span class="source-state" :class="connectionTone(source)">
                <span class="dot"></span>{{ connectionLabel(source) }}
              </span>
              <AppIcon class="row-chevron" name="chevron-right" />
            </button>
          </div>
          <p class="settings-note">
            Irisport 只保存会话标识、任务短摘录和提醒状态；完整对话始终留在原应用中。
          </p>
        </template>

        <template v-else>
          <button class="back-button" type="button" @click="selectedSource = null">
            <AppIcon name="chevron-left" />所有来源
          </button>
          <header class="source-detail-head">
            <SourceIcon :source="activeSource.source" />
            <div>
              <h1>{{ activeSource.label }}</h1>
              <span class="source-state" :class="connectionTone(activeSource)">
                <span class="dot"></span>{{ connectionLabel(activeSource) }}
              </span>
            </div>
          </header>
          <p class="source-description">{{ activeSource.description }}</p>

          <div class="settings-list compact-list">
            <div class="settings-item">
              <span>完成结果</span>
              <span>{{ activeSource.supports_completion ? "支持自动接收" : "等待官方事件接口" }}</span>
            </div>
            <div class="settings-item">
              <span>原会话回跳</span>
              <span>{{ activeSource.supports_open ? "支持" : "暂不支持" }}</span>
            </div>
            <div class="settings-item">
              <span>已读方式</span>
              <span>{{ activeSource.supports_auto_read ? "原应用已读状态" : "不可用" }}</span>
            </div>
          </div>

          <template v-if="activeSource.supports_completion">
            <h2 class="group-title">接入状态</h2>
            <div class="settings-list compact-list">
              <div class="settings-item"><span>配置</span><span>{{ activeSource.configured ? "已写入" : "未写入" }}</span></div>
              <div class="settings-item"><span>接入验证</span><span>{{ activeSource.access_verified ? "已收到真实事件" : "等待真实事件" }}</span></div>
              <div class="settings-item"><span>最近事件</span><span>{{ lastEvent(activeSource) }}</span></div>
              <div class="settings-item"><span>未通过校验</span><span>{{ activeSource.rejected_count }}</span></div>
            </div>
          </template>

          <div class="source-detail-actions">
            <button
              v-if="activeSource.supports_completion && !activeSource.configured"
              class="button primary"
              :disabled="busy || !activeSource.installed"
              @click="install(activeSource)"
            >
              连接 {{ activeSource.label }}
            </button>
            <button
              v-if="activeSource.update_available"
              class="button primary"
              :disabled="busy"
              @click="install(activeSource)"
            >
              更新 Hooks
            </button>
            <button
              v-if="activeSource.supports_completion && activeSource.configured"
              class="button"
              :disabled="busy"
              @click="uninstall(activeSource)"
            >
              断开连接
            </button>
            <button class="button" :disabled="!activeSource.installed" @click="openSource(activeSource)">
              {{ activeSource.source === 'codex' ? '打开 Codex 设置' : `打开 ${activeSource.label}` }}
            </button>
          </div>
          <p v-if="activeSource.update_available" class="settings-note">
            有新的任务状态规则。更新后，如原应用提示，请重新信任 Irisport hooks。
          </p>
          <p v-if="!activeSource.installed" class="settings-note warn">
            未检测到 {{ activeSource.label }} 应用。请先安装应用，再连接来源。
          </p>
          <p v-else-if="activeSource.configured && !activeSource.access_verified" class="settings-note warn">
            配置已写入但尚未收到真实事件。请开始一个新会话并发送一次任务。
          </p>
        </template>
      </section>

      <section v-else class="settings-section" role="tabpanel">
        <header class="section-heading">
          <h1>数据</h1>
          <p>Irisport 的数据只保存在这台 Mac 上。</p>
        </header>
        <div class="settings-list data-list">
          <div class="data-row">
            <div><span class="item-title">应用数据</span><span class="item-sub">提醒状态、事件收件箱与本地数据库</span></div>
            <button class="button secondary" @click="reveal('app-data')">在 Finder 中显示</button>
          </div>
          <div class="data-row">
            <div><span class="item-title">Codex hooks</span><span class="item-sub">仅管理包含 Irisport 标识的规则</span></div>
            <button class="button secondary" @click="reveal('codex-hooks')">在 Finder 中显示</button>
          </div>
          <div class="data-row">
            <div><span class="item-title">Kimi Code 配置</span><span class="item-sub">保留其他配置、注释与 hooks</span></div>
            <button class="button secondary" @click="reveal('kimi-config')">在 Finder 中显示</button>
          </div>
        </div>
        <div class="privacy-note">
          <AppIcon name="info" />
          <p>已读记录最多保留 7 天或 50 条；未读和进行中的请求不受此限制。完整对话、回复正文和 transcript 不会写入 Irisport。</p>
        </div>
      </section>
    </div>

    <footer class="settings-footer">
      <span>Irisport {{ status?.version ?? "0.1.0" }}</span>
      <span>{{ activeTab?.label }}</span>
    </footer>
  </main>
</template>
