<script setup lang="ts">
import { computed } from "vue";
import chatGptLogo from "../assets/sources/chatgpt.png";
import kimiCodeLogo from "../assets/sources/kimi-code.png";
import workBuddyLogo from "../assets/sources/workbuddy.png";

const props = defineProps<{
  source: string;
}>();

const logo = computed(() => {
  if (props.source === "kimi-code") return kimiCodeLogo;
  if (props.source === "workbuddy") return workBuddyLogo;
  return chatGptLogo;
});
</script>

<template>
  <span class="source-icon" :class="`source-${source}`" aria-hidden="true">
    <img :src="logo" alt="" />
  </span>
</template>
