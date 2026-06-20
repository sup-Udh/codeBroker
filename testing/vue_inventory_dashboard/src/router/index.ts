import { createRouter, createWebHistory } from 'vue-router'
import DashboardView from '../views/DashboardView.vue'

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    {
      path: '/',
      name: 'dashboard',
      component: DashboardView
    },
    {
      path: '/inventory',
      name: 'inventory',
      component: () => import('../views/InventoryView.vue')
    },
    {
      path: '/suppliers',
      name: 'suppliers',
      component: () => import('../views/SuppliersView.vue')
    }
  ]
})

export default router
