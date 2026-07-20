import { useContingencyStore } from 'src/stores/contingency';
import { useEditorStore } from 'src/stores/editor';
import { useNetworkStore } from 'src/stores/network';
import { useScenariosStore } from 'src/stores/scenarios';
import { useSimulateStore } from 'src/stores/simulate';
import { useTimeseriesStore } from 'src/stores/timeseries';

/**
 * Remet à zéro l'état d'étude côté client après un changement de réseau / topologie.
 * À appeler après selectNetwork, import actif, applyScenario, ou cas démo.
 */
export function resetStudyState(): void {
  useSimulateStore().resetSimulation();
  useContingencyStore().reset();
  useTimeseriesStore().reset();
  useNetworkStore().clearCalibrationPressureResiduals();
  useScenariosStore().clearCompare();

  const editor = useEditorStore();
  editor.clearSelection();
  editor.dirty = false;
  editor.placingNode = false;
  if (editor.editMode) {
    editor.setEditMode(false);
  }
}
