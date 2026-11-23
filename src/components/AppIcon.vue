<script setup lang="ts">
export type IconName =
  | "gear"
  | "bell"
  | "sources"
  | "folder"
  | "arrow-right"
  | "chevron-right"
  | "chevron-left"
  | "check"
  | "external"
  | "inbox"
  | "codex"
  | "kimi-code"
  | "workbuddy"
  | "info";

defineProps<{
  name: IconName;
}>();
</script>

<template>
  <svg
    class="app-icon"
    viewBox="0 0 20 20"
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <template v-if="name === 'gear'">
      <circle cx="10" cy="10" r="2.5" />
      <path d="M8.3 2.8h3.4l.5 2 1.2.7 1.9-.8 1.7 2.9-1.4 1.5v1.5l1.4 1.5-1.7 2.9-1.9-.8-1.2.7-.5 2H8.3l-.5-2-1.2-.7-1.9.8-1.7-2.9 1.4-1.5V9.1L3 7.6l1.7-2.9 1.9.8 1.2-.7.5-2Z" />
    </template>
    <template v-else-if="name === 'bell'">
      <path d="M4.2 13.8h11.6l-1.4-2.2V8.4a4.4 4.4 0 0 0-8.8 0v3.2l-1.4 2.2Z" />
      <path d="M8.2 16a2 2 0 0 0 3.6 0" />
    </template>
    <template v-else-if="name === 'sources'">
      <rect x="2.8" y="3" width="14.4" height="4.3" rx="1.4" />
      <rect x="2.8" y="12.7" width="14.4" height="4.3" rx="1.4" />
      <path d="M6 5.2h.1M6 14.7h.1M9 5.2h4.8M9 14.7h4.8" />
    </template>
    <template v-else-if="name === 'folder'">
      <path d="M2.5 6.5V5.2c0-.8.6-1.4 1.4-1.4h3.4l1.5 1.8h7.8c.8 0 1.4.6 1.4 1.4v7.8c0 .8-.6 1.4-1.4 1.4H3.9c-.8 0-1.4-.6-1.4-1.4V6.5Z" />
      <path d="M2.7 8.8h14.6" />
    </template>
    <template v-else-if="name === 'arrow-right'">
      <path d="M3.5 10h13M11.5 5l5 5-5 5" />
    </template>
    <template v-else-if="name === 'chevron-right'">
      <path d="m7.5 4.5 5.5 5.5-5.5 5.5" />
    </template>
    <template v-else-if="name === 'chevron-left'">
      <path d="m12.5 4.5-5.5 5.5 5.5 5.5" />
    </template>
    <template v-else-if="name === 'check'">
      <path d="m4 10.5 3.8 3.8L16 5.8" />
    </template>
    <template v-else-if="name === 'external'">
      <path d="M11 3.5h5.5V9M16.5 3.5 9 11" />
      <path d="M14.5 11.5v4c0 .8-.6 1.4-1.4 1.4H4.9c-.8 0-1.4-.6-1.4-1.4V6.9c0-.8.6-1.4 1.4-1.4h4" />
    </template>
    <template v-else-if="name === 'inbox'">
      <path d="M3.2 11.2 5.4 4.6c.2-.7.8-1.1 1.5-1.1h6.2c.7 0 1.3.4 1.5 1.1l2.2 6.6v3.4c0 .9-.7 1.6-1.6 1.6H4.8c-.9 0-1.6-.7-1.6-1.6v-3.4Z" />
      <path d="M3.4 11.2h4.1l1 1.7h3l1-1.7h4.1" />
    </template>
    <template v-else-if="name === 'codex'">
      <path d="m7 5-4 5 4 5M13 5l4 5-4 5M11.4 3.5l-2.8 13" />
    </template>
    <template v-else-if="name === 'kimi-code'">
      <rect x="2.5" y="3.2" width="15" height="13.6" rx="2" />
      <path d="m5.5 7.5 2.3 2.5-2.3 2.5M10 12.8h4.5" />
    </template>
    <template v-else-if="name === 'workbuddy'">
      <rect x="3" y="3" width="14" height="14" rx="4" />
      <path d="m6.5 10.2 2.3 2.3 4.7-5" />
    </template>
    <template v-else-if="name === 'info'">
      <circle cx="10" cy="10" r="7.2" />
      <path d="M10 9v5M10 5.8h.1" />
    </template>
  </svg>
</template>
