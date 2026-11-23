<script setup lang="ts">
import { computed } from "vue";
import type { SessionView } from "../lib/api";
import { formatDuration } from "../lib/time";
import SourceIcon from "./SourceIcon.vue";

const props = defineProps<{
  session: SessionView;
  busy: boolean;
  now: number;
}>();

const emit = defineEmits<{
  open: [session: SessionView];
}>();

const title = computed(() => {
  const sessionTitle = props.session.session_title?.trim();
  if (sessionTitle) return sessionTitle;
  const excerpt = props.session.prompt_excerpt?.trim();
  return excerpt || "Agent 任务";
});

const workTime = computed(() => {
  const start = props.session.last_prompt_at ?? props.session.updated_at;
  const end = props.session.pending
    ? props.now
    : props.session.last_result_at ?? props.session.updated_at;
  return formatDuration(end - start);
});

const dotClass = computed(() => ({
  pending: props.session.pending,
  done: props.session.state === "completed",
  failed: props.session.state === "failed",
  interrupted: props.session.state === "interrupted",
}));

const canOpen = computed(() => props.session.source === "codex");
const actionLabel = computed(() =>
  props.session.pending ? "打开原对话，任务保持进行中" : "打开结果",
);
</script>

<template>
  <component
    :is="canOpen ? 'button' : 'div'"
    class="session"
    :class="{ unread: session.unread, busy, pending: session.pending, static: !canOpen }"
    :type="canOpen ? 'button' : undefined"
    :disabled="canOpen ? busy : undefined"
    :aria-label="canOpen ? `${actionLabel}：${session.source_label}，${title}，工作时间 ${workTime}` : undefined"
    @click="canOpen && emit('open', session)"
  >
    <SourceIcon :source="session.source" />
    <span class="session-main">
      <span class="session-top">
        <span class="source">
          <span
            v-if="session.unread || session.pending"
            class="unread-dot"
            :class="dotClass"
          ></span>
          {{ session.source_label }}
        </span>
        <span class="work-time">{{ workTime }}</span>
      </span>
      <span class="session-title">{{ title }}</span>
    </span>
  </component>
</template>
