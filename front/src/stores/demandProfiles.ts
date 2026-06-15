import { defineStore } from 'pinia';
import { ref } from 'vue';
import type { DemandProfileDto } from 'src/utils/demandProfiles';

const STORAGE_PREFIX = 'gazflow:demand-profiles:';

function datasetKey(datasetId: string | null | undefined): string {
  const safeDatasetId = datasetId && datasetId.trim().length > 0 ? datasetId : 'default';
  return `${STORAGE_PREFIX}${safeDatasetId}`;
}

function storageAvailable(): boolean {
  return typeof window !== 'undefined' && !!window.localStorage;
}

function isDemandProfileRecord(value: unknown): value is Record<string, DemandProfileDto> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function readStoredProfiles(datasetId: string | null | undefined): Record<string, DemandProfileDto> {
  if (!storageAvailable()) {
    return {};
  }
  const raw = window.localStorage.getItem(datasetKey(datasetId));
  if (!raw) {
    return {};
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    return isDemandProfileRecord(parsed) ? parsed : {};
  } catch {
    return {};
  }
}

function writeStoredProfiles(
  datasetId: string | null | undefined,
  data: Record<string, DemandProfileDto>,
): void {
  if (!storageAvailable()) return;
  window.localStorage.setItem(datasetKey(datasetId), JSON.stringify(data));
}

export const useDemandProfilesStore = defineStore('demandProfiles', () => {
  const profiles = ref<Record<string, DemandProfileDto>>({});
  const currentDatasetId = ref<string | null>(null);

  function load(datasetId: string | null | undefined): void {
    currentDatasetId.value = datasetId ?? null;
    profiles.value = readStoredProfiles(datasetId);
  }

  function persist(datasetId: string | null | undefined): void {
    writeStoredProfiles(datasetId, profiles.value);
  }

  function setProfile(
    nodeId: string,
    profile: DemandProfileDto,
    datasetId: string | null | undefined = currentDatasetId.value,
  ): void {
    if (datasetId === currentDatasetId.value) {
      profiles.value = {
        ...profiles.value,
        [nodeId]: profile,
      };
      persist(datasetId);
      return;
    }
    const stored = readStoredProfiles(datasetId);
    writeStoredProfiles(datasetId, { ...stored, [nodeId]: profile });
  }

  function removeMissing(nodeIds: string[], datasetId: string | null | undefined): void {
    const allowed = new Set(nodeIds);
    if (datasetId === currentDatasetId.value) {
      const next: Record<string, DemandProfileDto> = {};
      for (const [nodeId, profile] of Object.entries(profiles.value)) {
        if (allowed.has(nodeId)) {
          next[nodeId] = profile;
        }
      }
      profiles.value = next;
      persist(datasetId);
      return;
    }
    const stored = readStoredProfiles(datasetId);
    const next: Record<string, DemandProfileDto> = {};
    for (const [nodeId, profile] of Object.entries(stored)) {
      if (allowed.has(nodeId)) {
        next[nodeId] = profile;
      }
    }
    writeStoredProfiles(datasetId, next);
  }

  function reset(datasetId: string | null | undefined = currentDatasetId.value): void {
    if (datasetId === currentDatasetId.value) {
      profiles.value = {};
    }
    if (!storageAvailable()) return;
    window.localStorage.removeItem(datasetKey(datasetId));
  }

  return {
    profiles,
    currentDatasetId,
    load,
    persist,
    setProfile,
    removeMissing,
    reset,
  };
});
