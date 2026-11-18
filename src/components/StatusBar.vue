<script setup lang="ts">
import { computed } from "vue";
import type { HookStatus } from "../lib/api";
import { formatRelative } from "../lib/time";

const props = defineProps<{ status: HookStatus | null }>();

const tone = computed(() => {
  if (!props.status) return "idle";
  if (!props.status.configured) return "idle";
  if (!props.status.access_verified) return "wait";
  return "ok";
});

const label = computed(() => {
  if (!props.status) return "正在检查连接";
  if (!props.status.configured) return "未连接 Codex";
  if (!props.status.access_verified) return "配置已写入，等待 Codex 信任";
  return "已连接 Codex";
});

const detail = computed(() => {
  if (!props.status) return "";
  if (props.status.access_verified && props.status.last_event_at) {
    return `最近事件 ${formatRelative(props.status.last_event_at)}`;
  }
  if (props.status.rejected_count > 0) {
    return `${props.status.rejected_count} 个事件未通过来源校验`;
  }
  return "";
});
</script>

<template>
  <div class="status">
    <span class="status-dot" :class="tone"></span>
    <span>{{ label }}</span>
    <span v-if="detail" class="status-detail">{{ detail }}</span>
  </div>
</template>
