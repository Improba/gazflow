<template>
  <q-card flat bordered class="bg-dark text-white">
    <q-card-section class="q-pb-xs">
      <div class="text-subtitle2">Aperçu import</div>
    </q-card-section>

    <q-card-section v-if="error" class="q-pt-none">
      <q-banner dense rounded class="bg-red-10 text-red-2">
        {{ error }}
      </q-banner>
    </q-card-section>

    <q-card-section v-else-if="result" class="q-pt-none q-gutter-sm">
      <div class="text-body2">
        <div v-if="result.validate_only">Validation OK — prêt à importer.</div>
        <div v-else>Réseau importé : <strong>{{ result.network_id }}</strong></div>
        <div>Nœuds : {{ result.node_count }} — Conduites : {{ result.edge_count }}</div>
        <div v-if="result.active">Réseau actif sur la carte.</div>
      </div>

      <ImportMapPreview :geometry="result.preview ?? null" />
    </q-card-section>

    <q-card-section v-else class="q-pt-none text-grey-5 text-body2">
      Chargez un mapping et des fichiers réseau, puis validez ou importez.
    </q-card-section>
  </q-card>
</template>

<script setup lang="ts">
import ImportMapPreview from 'src/components/ImportMapPreview.vue';
import type { ImportNetworkResponse } from 'src/services/api';

defineProps<{
  result: ImportNetworkResponse | null;
  error: string | null;
}>();
</script>
