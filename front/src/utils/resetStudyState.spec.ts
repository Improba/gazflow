import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

const spies = vi.hoisted(() => ({
  resetSimulation: vi.fn(),
  contingencyReset: vi.fn(),
  timeseriesReset: vi.fn(),
  clearCalibration: vi.fn(),
  clearCompare: vi.fn(),
  clearSelection: vi.fn(),
  setEditMode: vi.fn(),
  editor: {
    dirty: true,
    placingNode: true,
    editMode: true,
  },
}));

vi.mock('src/stores/simulate', () => ({
  useSimulateStore: () => ({
    resetSimulation: spies.resetSimulation,
  }),
}));

vi.mock('src/stores/contingency', () => ({
  useContingencyStore: () => ({
    reset: spies.contingencyReset,
  }),
}));

vi.mock('src/stores/timeseries', () => ({
  useTimeseriesStore: () => ({
    reset: spies.timeseriesReset,
  }),
}));

vi.mock('src/stores/network', () => ({
  useNetworkStore: () => ({
    clearCalibrationPressureResiduals: spies.clearCalibration,
  }),
}));

vi.mock('src/stores/scenarios', () => ({
  useScenariosStore: () => ({
    clearCompare: spies.clearCompare,
  }),
}));

vi.mock('src/stores/editor', () => ({
  useEditorStore: () => ({
    clearSelection: spies.clearSelection,
    get dirty() {
      return spies.editor.dirty;
    },
    set dirty(value: boolean) {
      spies.editor.dirty = value;
    },
    get placingNode() {
      return spies.editor.placingNode;
    },
    set placingNode(value: boolean) {
      spies.editor.placingNode = value;
    },
    get editMode() {
      return spies.editor.editMode;
    },
    setEditMode: spies.setEditMode,
  }),
}));

import { resetStudyState } from './resetStudyState';

describe('resetStudyState', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    spies.editor.dirty = true;
    spies.editor.placingNode = true;
    spies.editor.editMode = true;
  });

  it('resets simulation, contingency, timeseries, calibration, compare and editor', () => {
    resetStudyState();

    expect(spies.resetSimulation).toHaveBeenCalledTimes(1);
    expect(spies.contingencyReset).toHaveBeenCalledTimes(1);
    expect(spies.timeseriesReset).toHaveBeenCalledTimes(1);
    expect(spies.clearCalibration).toHaveBeenCalledTimes(1);
    expect(spies.clearCompare).toHaveBeenCalledTimes(1);
    expect(spies.clearSelection).toHaveBeenCalledTimes(1);
    expect(spies.editor.dirty).toBe(false);
    expect(spies.editor.placingNode).toBe(false);
    expect(spies.setEditMode).toHaveBeenCalledWith(false);
  });
});
