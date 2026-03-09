import { boot } from 'quasar/wrappers';
import { Ion, buildModuleUrl } from 'cesium';

export default boot(() => {
  (window as Record<string, unknown>).CESIUM_BASE_URL = '/cesium';
  buildModuleUrl.setBaseUrl('/cesium/');

  // Désactiver le token Ion par défaut (on n'utilise pas Cesium Ion pour le MVP).
  Ion.defaultAccessToken = '';
});
