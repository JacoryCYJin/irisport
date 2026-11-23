<script setup lang="ts">
defineProps<{
  label: string;
  description?: string;
  modelValue: boolean;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  change: [value: boolean];
}>();
</script>

<template>
  <div class="toggle-row">
    <div class="toggle-copy">
      <span class="toggle-label">{{ label }}</span>
      <span v-if="description" class="toggle-description">{{ description }}</span>
    </div>
    <button
      class="switch"
      :class="{ on: modelValue }"
      type="button"
      role="switch"
      :aria-checked="modelValue"
      :aria-label="label"
      :disabled="disabled"
      @click="emit('change', !modelValue)"
    >
      <span class="switch-thumb"></span>
    </button>
  </div>
</template>
