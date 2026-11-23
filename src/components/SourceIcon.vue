<script setup lang="ts">
import { computed } from "vue";
import AppIcon, { type IconName } from "./AppIcon.vue";

const props = defineProps<{
  source: string;
}>();

const icon = computed<IconName>(() => {
  if (props.source === "kimi-code") return "kimi-code";
  if (props.source === "workbuddy") return "workbuddy";
  return "codex";
});
</script>

<template>
  <span class="source-icon" :class="`source-${source}`" aria-hidden="true">
    <AppIcon :name="icon" />
  </span>
</template>
