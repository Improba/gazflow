import { createRouter, createMemoryHistory, createWebHistory } from 'vue-router';

const routes = [
  {
    path: '/',
    component: () => import('src/layouts/MainLayout.vue'),
    children: [
      { path: '', component: () => import('src/pages/MapPage.vue') },
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
