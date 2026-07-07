import { createRouter, createMemoryHistory, createWebHistory } from 'vue-router';

const routes = [
  {
    path: '/',
    component: () => import('src/layouts/MainLayout.vue'),
    children: [
      { path: '', name: 'map', component: () => import('src/pages/MapPage.vue') },
      { path: 'import', name: 'import', component: () => import('src/pages/ImportPage.vue') },
      {
        path: 'contingency',
        name: 'contingency',
        component: () => import('src/pages/ContingencyPage.vue'),
      },
      {
        path: 'calibration',
        name: 'calibration',
        component: () => import('src/pages/CalibrationPage.vue'),
      },
      {
        path: 'transient',
        name: 'transient',
        component: () => import('src/pages/TransientPage.vue'),
      },
      {
        path: 'exports',
        name: 'exports',
        component: () => import('src/pages/ExportsPage.vue'),
      },
      {
        path: 'batch',
        name: 'batch',
        component: () => import('src/pages/BatchPage.vue'),
      },
    ],
  },
];

export default createRouter({
  history:
    typeof window !== 'undefined'
      ? createWebHistory()
      : createMemoryHistory(),
  routes,
});
