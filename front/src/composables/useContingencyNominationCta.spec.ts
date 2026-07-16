import { beforeEach, describe, expect, it } from 'vitest';
import { computed } from 'vue';
import { createPinia, setActivePinia } from 'pinia';
import { useContingencyNominationCta } from './useContingencyNominationCta';
import { useEditorStore } from 'src/stores/editor';
import { useNetworkStore } from 'src/stores/network';
import { useNominationStore } from 'src/stores/nomination';
import { useSimulateStore } from 'src/stores/simulate';

describe('useContingencyNominationCta', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('is disabled without nomination or network', () => {
    const { disabled, disabledTooltip } = useContingencyNominationCta();

    expect(disabled.value).toBe(true);
    expect(disabledTooltip.value).toContain('nomination active');
  });

  it('is disabled when the editor is dirty', () => {
    const nominationStore = useNominationStore();
    const networkStore = useNetworkStore();
    const editorStore = useEditorStore();

    nominationStore.selectById('nomination_mild_618');
    networkStore.nodes = [{ id: 'N1' } as never];
    editorStore.dirty = true;

    const { disabled, disabledTooltip } = useContingencyNominationCta();

    expect(disabled.value).toBe(true);
    expect(disabledTooltip.value).toContain('non enregistrées');
  });

  it('is disabled when scenario is dirty on Map', () => {
    const nominationStore = useNominationStore();
    const networkStore = useNetworkStore();

    nominationStore.selectById('nomination_mild_618');
    networkStore.nodes = [{ id: 'N1' } as never];

    const scenarioDirty = computed(() => true);
    const { disabled, disabledTooltip } = useContingencyNominationCta(scenarioDirty);

    expect(disabled.value).toBe(true);
    expect(disabledTooltip.value).toContain('dernière validation');
  });

  it('is disabled when scenario is dirty on Workspace', () => {
    const nominationStore = useNominationStore();
    const networkStore = useNetworkStore();
    const simulateStore = useSimulateStore();

    nominationStore.selectById('nomination_mild_618');
    networkStore.nodes = [{ id: 'N1' } as never];
    simulateStore.status = 'converged';

    const scenarioDirty = computed(() => simulateStore.scenarioDirty);
    const { disabled, disabledTooltip } = useContingencyNominationCta(scenarioDirty);

    expect(disabled.value).toBe(true);
    expect(disabledTooltip.value).toContain('dernière validation');
  });

  it('is enabled when nomination, network and editor are ready', () => {
    const nominationStore = useNominationStore();
    const networkStore = useNetworkStore();

    nominationStore.selectById('nomination_mild_618');
    networkStore.nodes = [{ id: 'N1' } as never];

    const scenarioDirty = computed(() => false);
    const { disabled, contingencyNominationLink } = useContingencyNominationCta(scenarioDirty);

    expect(disabled.value).toBe(false);
    expect(contingencyNominationLink.value).toEqual({
      name: 'contingency',
      query: { scenario_id: 'nomination_mild_618' },
    });
  });
});
