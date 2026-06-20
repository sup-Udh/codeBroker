<script setup lang="ts">
import { onMounted } from 'vue';
import { useInventory } from '../composables/useInventory';
import InventoryCard from '../components/InventoryCard.vue';
import { formatCurrency } from '../utils/formatter';

const { totalItems, totalValue, lowStockItems, loading, fetchInventory } = useInventory();

onMounted(() => {
  fetchInventory();
});
</script>

<template>
  <div class="dashboard">
    <h1>Dashboard Overview</h1>
    
    <div v-if="loading">Loading...</div>
    <div v-else>
      <div class="cards-grid">
        <InventoryCard title="Total Items" :value="totalItems" />
        <InventoryCard title="Total Value" :value="formatCurrency(totalValue)" highlight />
        <InventoryCard title="Low Stock Items" :value="lowStockItems.length" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.dashboard {
  max-width: 1200px;
  margin: 0 auto;
}
.cards-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
  gap: 1.5rem;
  margin-top: 2rem;
}
</style>
