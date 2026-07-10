import { ref } from 'vue';
import { Notify } from 'quasar';
import { runDemoCase, DEMO_NETWORK_ID } from 'src/utils/demoCase';
import { formatApiError } from 'src/utils/importError';
import { useRecentNetworks } from 'src/composables/useRecentNetworks';

export function useDemo() {
  const isLoadingDemo = ref(false);
  const demoError = ref<string | null>(null);

  async function launchDemo(): Promise<void> {
    if (isLoadingDemo.value) {
      return;
    }
    isLoadingDemo.value = true;
    demoError.value = null;
    try {
      await runDemoCase();
      const { addRecent } = useRecentNetworks();
      addRecent(DEMO_NETWORK_ID);
      Notify.create({
        type: 'positive',
        message: 'Cas démo GasLib-11 chargé et simulé.',
        timeout: 3000,
      });
    } catch (err) {
      demoError.value = formatApiError(err);
      Notify.create({
        type: 'negative',
        message: demoError.value,
        timeout: 5000,
      });
    } finally {
      isLoadingDemo.value = false;
    }
  }

  return {
    isLoadingDemo,
    demoError,
    launchDemo,
  };
}
