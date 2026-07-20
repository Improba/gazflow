<template>
  <div class="global-status-bar row items-center no-wrap q-gutter-sm" role="status" aria-live="polite">
    <q-icon name="hub" size="xs" class="text-grey-5 gt-xs" aria-hidden="true" />
    <q-chip
      dense
      flat
      class="status-chip"
      :color="networkToneColor"
      text-color="white"
      icon="lan"
    >
      <span class="ellipsis">{{ status.network.value ?? 'Aucun réseau' }}</span>
      <q-tooltip>{{ status.network.value ?? 'Aucun réseau chargé' }}</q-tooltip>
    </q-chip>

    <q-separator vertical dark class="status-sep gt-xs" />

    <q-chip
      dense
      flat
      class="status-chip"
      :color="toneColor(status.runStatus.value.tone)"
      text-color="white"
      icon="play_circle_outline"
    >
      {{ status.runStatus.value.label }}
    </q-chip>

    <q-chip
      dense
      flat
      class="status-chip gt-sm"
      :color="status.nomination.value.id ? 'blue-grey-8' : 'grey-9'"
      text-color="grey-2"
      icon="assignment"
    >
      <span class="ellipsis">{{ status.nomination.value.label }}</span>
      <q-tooltip>{{ status.nomination.value.label }}</q-tooltip>
    </q-chip>

    <q-space class="gt-xs" />

    <q-chip
      dense
      flat
      class="status-chip"
      :color="toneColor(status.n1Status.value.tone)"
      text-color="white"
      icon="verified_user"
    >
      <span class="ellipsis">{{ status.n1Status.value.label }}</span>
      <q-tooltip>{{ status.n1Status.value.label }}</q-tooltip>
    </q-chip>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useGlobalStatus, type StatusTone } from 'src/composables/useGlobalStatus';

const status = useGlobalStatus();

const networkToneColor = computed(() =>
  status.network.value ? 'green-9' : 'grey-9',
);

function toneColor(tone: StatusTone): string {
  switch (tone) {
    case 'success':
      return 'green-9';
    case 'warning':
      return 'orange-9';
    case 'danger':
      return 'red-9';
    default:
      return 'grey-9';
  }
}
</script>

<style scoped>
.global-status-bar {
  padding: 2px 12px;
  background: var(--scada-panel, #11161c);
  border-bottom: 1px solid var(--scada-border, #1f2a33);
  min-height: 30px;
  overflow-x: auto;
  scrollbar-width: none;
}

.global-status-bar::-webkit-scrollbar {
  display: none;
}

.status-chip {
  max-width: 220px;
  flex-shrink: 0;
}

.status-chip :deep(.q-chip__content) {
  white-space: nowrap;
  overflow: hidden;
}

.status-sep {
  height: 18px;
  margin: 0 2px;
}

@media (max-width: 599px) {
  .status-chip {
    max-width: 140px;
  }
}
</style>
