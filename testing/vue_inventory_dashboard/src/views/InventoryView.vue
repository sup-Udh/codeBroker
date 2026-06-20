<script setup lang="ts">
import { onMounted } from 'vue';
import { useInventory } from '../composables/useInventory';
import InventoryTable from '../components/InventoryTable.vue';

const { items, loading, error, fetchInventory, restock } = useInventory();

onMounted(() => {
  fetchInventory();
});

const handleRestock = async (id: string) => {
  await restock(id, 50); // Restock by 50 units
};
</script>

<template>
  <div class="inventory-view">
    <h1>Inventory Management</h1>
    
    <div v-if="error" class="error">{{ error }}</div>
    <div v-if="loading">Loading inventory data...</div>
    
    <InventoryTable 
      v-else 
      :items="items" 
      @restock="handleRestock" 
    />
  </div>
</template>

<style scoped>
.inventory-view {
  max-width: 1200px;
  margin: 0 auto;
}
.error {
  color: red;
  padding: 1rem;
  background-color: #ffeeee;
  border-radius: 4px;
}
</style>
