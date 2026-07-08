import { ref } from 'vue';
import { useQuasar } from 'quasar';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';

export function useDemo() {
  const $q = useQuasar();
  const networkStore = useNetworkStore();
  const simulateStore = useSimulateStore();

  const isLoadingDemo = ref(false);
  const demoError = ref(null);

  /**
   * Lance la démo instantanée avec GasLib-11
   */
  const launchDemo = async () => {
    isLoadingDemo.value = true;
    demoError.value = null;

    try {
      $q.loading.show({
        message: 'Préparation de la démo...',
        spinnerColor: '#0066CC',
      });

      // Étape 1: Sélectionner le réseau GasLib-11
      await networkStore.selectNetwork('GasLib-11');

      // Étape 2: Charger le réseau (déjà fait dans selectNetwork, mais on attend pour être sûr)
      if (networkStore.nodes.length === 0) {
        await networkStore.fetchNetwork();
      }

      // Étape 3: Vérifier que le réseau est chargé
      if (networkStore.nodes.length === 0) {
        throw new Error('Aucun nœud chargé. Vérifiez que GasLib-11 est disponible.');
      }

      // Étape 4: Configurer un scénario par défaut (demandes simples)
      const defaultDemands = {};
      networkStore.nodes.forEach((node) => {
        // Exemple : injecter +100 m³/s aux sources, retirer -50 m³/s aux puits
        if (node.flow_max_m3s !== null && node.flow_max_m3s > 0) {
          defaultDemands[node.id] = 100; // Injection
        } else if (node.flow_min_m3s !== null && node.flow_min_m3s < 0) {
          defaultDemands[node.id] = -50; // Retrait
        }
      });

      // Étape 5: Lancer la simulation avec les demandes par défaut
      await simulateStore.startSimulation({
        demands: defaultDemands,
        // Autres paramètres par défaut (optionnels)
        max_iterations: 20,
        tolerance: 1e-6,
      });

      // Succès : notification
      $q.notify({
        type: 'positive',
        message: 'Démo chargée ! Explorez le réseau GasLib-11.',
        icon: 'check_circle',
        timeout: 3000,
      });
    } catch (error) {
      console.error('Erreur lors du chargement de la démo:', error);
      demoError.value = error.message || 'Échec du chargement de la démo.';
      $q.notify({
        type: 'negative',
        message: demoError.value,
        icon: 'error_outline',
        timeout: 5000,
      });
    } finally {
      isLoadingDemo.value = false;
      $q.loading.hide();
    }
  };

  return {
    isLoadingDemo,
    demoError,
    launchDemo,
  };
}
