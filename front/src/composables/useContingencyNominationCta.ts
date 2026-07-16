import { computed, type Ref } from 'vue';
import { useEditorStore } from 'src/stores/editor';
import { useNetworkStore } from 'src/stores/network';
import { useNominationStore } from 'src/stores/nomination';

export function useContingencyNominationCta(scenarioDirty?: Ref<boolean>) {
  const editorStore = useEditorStore();
  const networkStore = useNetworkStore();
  const nominationStore = useNominationStore();

  const novaNominationId = computed(() => nominationStore.activeId);

  const contingencyNominationLink = computed(() => ({
    name: 'contingency' as const,
    query: novaNominationId.value ? { scenario_id: novaNominationId.value } : {},
  }));

  const disabled = computed(() => {
    if (!novaNominationId.value) return true;
    if (networkStore.nodes.length === 0) return true;
    if (editorStore.dirty || editorStore.saving) return true;
    if (scenarioDirty?.value) return true;
    return false;
  });

  const disabledTooltip = computed(() => {
    if (!novaNominationId.value) {
      return 'Sélectionnez une nomination active pour lancer l\'analyse N-1.';
    }
    if (networkStore.nodes.length === 0) {
      return 'Chargez un réseau avant l\'analyse N-1.';
    }
    if (editorStore.saving) {
      return 'Enregistrement du réseau en cours — patientez avant l\'analyse N-1.';
    }
    if (editorStore.dirty) {
      return 'Modifications réseau non enregistrées — enregistrez ou annulez avant l\'analyse N-1.';
    }
    if (scenarioDirty?.value) {
      return 'Nomination modifiée depuis la dernière validation — relancez la simulation avant l\'analyse N-1.';
    }
    return 'Ouvre l\'analyse de contingence N-1 avec les demandes de la nomination active.';
  });

  return {
    novaNominationId,
    contingencyNominationLink,
    disabled,
    disabledTooltip,
  };
}
