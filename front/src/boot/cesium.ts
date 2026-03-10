import { boot } from 'quasar/wrappers';
import { Ion, buildModuleUrl } from 'cesium';

export default boot(() => {
  (window as Record<string, unknown>).CESIUM_BASE_URL = '/cesium';
  buildModuleUrl.setBaseUrl('/cesium/');

  Ion.defaultAccessToken = 'none';
});
