<script setup lang="ts">
import { computed } from "vue";
import type { SessionView } from "../lib/api";
import { formatClock, formatRelative, sessionTime } from "../lib/time";

const props = defineProps<{
  session: SessionView;
  busy: boolean;
}>();

const emit = defineEmits<{
  open: [session: SessionView];
}>();

const excerpt = computed(() => {
  const value = props.session.prompt_excerpt?.trim();
  return value || "未记录到请求摘录";
});

const timeLabel = computed(() => {
  return (
    formatClock(props.session.last_result_at) ||
    formatRelative(sessionTime(props.session))
  );
});
</script>

<template>
  <button
    class="session"
    :class="{ unread: session.unread, busy }"
    type="button"
    :disabled="busy"
    @click="emit('open', session)"
  >
    <span class="session-main">
      <span class="session-top">
        <span class="source">
          <span v-if="session.unread" class="unread-dot"></span>
          Codex
        </span>
        <span class="session-meta">
          <span class="time">{{ timeLabel }}</span>
        </span>
      </span>
      <span class="excerpt">{{ excerpt }}</span>
    </span>
    <svg class="chevron" viewBox="0 0 10 10" aria-hidden="true">
      <path
        d="M3.5 1.5 7 5l-3.5 3.5"
        fill="none"
        stroke="currentColor"
        stroke-width="1.4"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
  </button>
</template>
