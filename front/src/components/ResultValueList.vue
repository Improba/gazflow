<template>
  <div>
    <q-input
      v-model="filter"
      dense
      outlined
      dark
      clearable
      :placeholder="searchPlaceholder"
      class="q-mb-xs"
    >
      <template #prepend>
        <q-icon name="search" />
      </template>
    </q-input>
    <div class="text-caption text-grey-5 q-mb-xs">
      {{ filteredItems.length }} / {{ items.length }} éléments
    </div>
    <q-virtual-scroll
      :items="filteredItems"
      virtual-scroll-item-size="32"
      class="result-scroll"
      v-slot="{ item, index }"
    >
      <q-item :key="item.id + '-' + index" dense dark class="q-px-none">
        <q-item-section>{{ item.id }}</q-item-section>
        <q-item-section side class="text-weight-bold">
          {{ formatValue(item.value) }}
        </q-item-section>
      </q-item>
    </q-virtual-scroll>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';

const props = withDefaults(
  defineProps<{
    items: Record<string, number>;
    decimals?: number;
    searchPlaceholder?: string;
  }>(),
  {
    decimals: 2,
    searchPlaceholder: 'Filtrer par identifiant…',
  },
);

const filter = ref('');

const sortedItems = computed(() =>
  Object.entries(props.items)
    .map(([id, value]) => ({ id, value }))
    .sort((a, b) => a.id.localeCompare(b.id, 'fr')),
);

const filteredItems = computed(() => {
  const q = filter.value.trim().toLowerCase();
  if (!q) {
    return sortedItems.value;
  }
  return sortedItems.value.filter((item) => item.id.toLowerCase().includes(q));
});

function formatValue(value: number): string {
  return value.toFixed(props.decimals);
}
</script>

<style scoped>
.result-scroll {
  max-height: 220px;
}
</style>
