import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import {
  api,
  type CreateNodeRequest,
  type CreatePipeRequest,
  type UpdateNodeRequest,
  type UpdatePipeRequest,
} from 'src/services/api';
import { useNetworkStore, type NodeDto, type PipeDto } from 'src/stores/network';

const MAX_UNDO = 10;
const REF_LAT = 50.0;
const REF_LON = 10.0;
const KM_PER_DEG_LAT = 111.0;
const KM_PER_DEG_LON = 111.0 * Math.cos((REF_LAT * Math.PI) / 180);

export type SelectionKind = 'node' | 'pipe';

export interface EditorMutation {
  label: string;
  undo: () => Promise<void>;
}

function lonLatToXY(lon: number, lat: number) {
  return {
    x: (lon - REF_LON) * KM_PER_DEG_LON,
    y: (lat - REF_LAT) * KM_PER_DEG_LAT,
  };
}

function nextNodeId(nodes: NodeDto[]) {
  let index = nodes.length + 1;
  let candidate = `N${index}`;
  const existing = new Set(nodes.map((node) => node.id));
  while (existing.has(candidate)) {
    index += 1;
    candidate = `N${index}`;
  }
  return candidate;
}

export const useEditorStore = defineStore('editor', () => {
  const editMode = ref(false);
  const placingNode = ref(false);
  const selectedKind = ref<SelectionKind | null>(null);
  const selectedId = ref<string | null>(null);
  const saving = ref(false);
  const dirty = ref(false);
  const lastSavedAt = ref<number | null>(null);
  const error = ref<string | null>(null);
  const undoStack = ref<EditorMutation[]>([]);

  const networkStore = useNetworkStore();

  const selectedNode = computed(() =>
    selectedKind.value === 'node' && selectedId.value
      ? (networkStore.nodes.find((node) => node.id === selectedId.value) ?? null)
      : null,
  );

  const selectedPipe = computed(() =>
    selectedKind.value === 'pipe' && selectedId.value
      ? (networkStore.pipes.find((pipe) => pipe.id === selectedId.value) ?? null)
      : null,
  );

  const saveIndicator = computed(() => {
    if (saving.value) return 'saving';
    if (dirty.value) return 'dirty';
    if (lastSavedAt.value) return 'saved';
    return 'idle';
  });

  function clearSelection() {
    selectedKind.value = null;
    selectedId.value = null;
  }

  function selectNode(id: string) {
    selectedKind.value = 'node';
    selectedId.value = id;
    placingNode.value = false;
  }

  function selectPipe(id: string) {
    selectedKind.value = 'pipe';
    selectedId.value = id;
    placingNode.value = false;
  }

  function setEditMode(enabled: boolean) {
    editMode.value = enabled;
    if (!enabled) {
      placingNode.value = false;
      clearSelection();
      error.value = null;
    }
  }

  function togglePlacingNode() {
    placingNode.value = !placingNode.value;
    if (placingNode.value) {
      clearSelection();
    }
  }

  function pushUndo(mutation: EditorMutation) {
    undoStack.value = [...undoStack.value.slice(-(MAX_UNDO - 1)), mutation];
  }

  async function refreshNetwork() {
    await networkStore.fetchNetwork();
    dirty.value = false;
    lastSavedAt.value = Date.now();
  }

  async function runMutation(label: string, action: () => Promise<void>, undo: () => Promise<void>) {
    saving.value = true;
    error.value = null;
    try {
      await action();
      pushUndo({ label, undo });
      await refreshNetwork();
    } catch (err) {
      error.value = err instanceof Error ? err.message : 'Échec de la modification';
      throw err;
    } finally {
      saving.value = false;
    }
  }

  async function undoLast() {
    const mutation = undoStack.value.pop();
    if (!mutation) return;
    saving.value = true;
    error.value = null;
    try {
      await mutation.undo();
      await refreshNetwork();
    } catch (err) {
      error.value = err instanceof Error ? err.message : 'Échec de l’annulation';
      throw err;
    } finally {
      saving.value = false;
    }
  }

  async function createNodeAt(lon: number, lat: number) {
    const { x, y } = lonLatToXY(lon, lat);
    const payload: CreateNodeRequest = {
      id: nextNodeId(networkStore.nodes),
      x,
      y,
      lon,
      lat,
      height_m: 0,
    };

    await runMutation(
      `Créer nœud ${payload.id}`,
      async () => {
        dirty.value = true;
        await api.createNode(payload);
      },
      async () => {
        await api.deleteNode(payload.id);
      },
    );

    selectNode(payload.id);
    placingNode.value = false;
  }

  async function deleteSelected() {
    if (!selectedKind.value || !selectedId.value) return;

    if (selectedKind.value === 'node') {
      const node = networkStore.nodes.find((item) => item.id === selectedId.value);
      if (!node) return;
      const connectedPipes = networkStore.pipes.filter(
        (pipe) => pipe.from === node.id || pipe.to === node.id,
      );
      const nodeId = node.id;
      const snapshot = { ...node };
      const pipeSnapshots = connectedPipes.map((pipe) => ({ ...pipe }));

      clearSelection();

      await runMutation(
        `Supprimer nœud ${nodeId}`,
        async () => {
          dirty.value = true;
          await api.deleteNode(nodeId);
        },
        async () => {
          const recreate: CreateNodeRequest = {
            id: snapshot.id,
            x: snapshot.x,
            y: snapshot.y,
            lon: snapshot.lon ?? undefined,
            lat: snapshot.lat ?? undefined,
            height_m: snapshot.height_m,
            pressure_fixed_bar: snapshot.pressure_fixed_bar ?? undefined,
          };
          await api.createNode(recreate);
          for (const pipe of pipeSnapshots) {
            const pipePayload: CreatePipeRequest = {
              id: pipe.id,
              from: pipe.from,
              to: pipe.to,
              kind: pipe.kind,
              length_km: pipe.length_km,
              diameter_mm: pipe.diameter_mm,
              equipment: pipe.equipment,
            };
            await api.createPipe(pipePayload);
          }
        },
      );
      return;
    }

    const pipe = networkStore.pipes.find((item) => item.id === selectedId.value);
    if (!pipe) return;
    const pipeId = pipe.id;
    const snapshot = { ...pipe };
    clearSelection();

    await runMutation(
      `Supprimer conduite ${pipeId}`,
      async () => {
        dirty.value = true;
        await api.deletePipe(pipeId);
      },
      async () => {
        const recreate: CreatePipeRequest = {
          id: snapshot.id,
          from: snapshot.from,
          to: snapshot.to,
          kind: snapshot.kind,
          length_km: snapshot.length_km,
          diameter_mm: snapshot.diameter_mm,
          equipment: snapshot.equipment,
        };
        await api.createPipe(recreate);
      },
    );
  }

  async function updateSelectedPipe(fields: Pick<UpdatePipeRequest, 'length_km' | 'diameter_mm'>) {
    if (selectedKind.value !== 'pipe' || !selectedId.value) return;
    const pipe = networkStore.pipes.find((item) => item.id === selectedId.value);
    if (!pipe) return;

    const previous: UpdatePipeRequest = {
      length_km: pipe.length_km,
      diameter_mm: pipe.diameter_mm,
    };
    const pipeId = pipe.id;

    await runMutation(
      `Modifier conduite ${pipeId}`,
      async () => {
        dirty.value = true;
        await api.updatePipe(pipeId, fields);
      },
      async () => {
        await api.updatePipe(pipeId, previous);
      },
    );
  }

  async function updateSelectedNode(fields: UpdateNodeRequest) {
    if (selectedKind.value !== 'node' || !selectedId.value) return;
    const node = networkStore.nodes.find((item) => item.id === selectedId.value);
    if (!node) return;

    const previous: UpdateNodeRequest = {
      x: node.x,
      y: node.y,
      lon: node.lon,
      lat: node.lat,
      height_m: node.height_m,
      pressure_fixed_bar: node.pressure_fixed_bar,
    };
    const nodeId = node.id;

    await runMutation(
      `Modifier nœud ${nodeId}`,
      async () => {
        dirty.value = true;
        await api.updateNode(nodeId, fields);
      },
      async () => {
        await api.updateNode(nodeId, previous);
      },
    );
  }

  return {
    editMode,
    placingNode,
    selectedKind,
    selectedId,
    selectedNode,
    selectedPipe,
    saving,
    dirty,
    lastSavedAt,
    error,
    undoStack,
    saveIndicator,
    clearSelection,
    selectNode,
    selectPipe,
    setEditMode,
    togglePlacingNode,
    undoLast,
    createNodeAt,
    deleteSelected,
    updateSelectedPipe,
    updateSelectedNode,
  };
});
