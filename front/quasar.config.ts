import { configure } from 'quasar/wrappers';
import { viteStaticCopy } from 'vite-plugin-static-copy';

export default configure(() => {
  return {
    boot: ['cesium'],

    css: ['app.scss'],

    extras: ['roboto-font', 'material-icons'],

    build: {
      target: { browser: ['es2022', 'firefox115', 'chrome115', 'safari14'] },
      vueRouterMode: 'history',

      extendViteConf(viteConf) {
        viteConf.plugins ??= [];
        viteConf.plugins.push(
          viteStaticCopy({
            targets: [
              { src: 'node_modules/cesium/Build/Cesium/Workers', dest: 'cesium' },
              { src: 'node_modules/cesium/Build/Cesium/ThirdParty', dest: 'cesium' },
              { src: 'node_modules/cesium/Build/Cesium/Assets', dest: 'cesium' },
              { src: 'node_modules/cesium/Build/Cesium/Widgets', dest: 'cesium' },
            ],
          }),
        );
      },
    },

    devServer: {
      open: false,
      host: '0.0.0.0',
      proxy: {
        '/api': {
          target: process.env.API_URL || 'http://localhost:3001',
          changeOrigin: true,
        },
      },
    },

    framework: {
      plugins: ['Notify', 'Loading', 'Dialog'],
    },
  };
});
