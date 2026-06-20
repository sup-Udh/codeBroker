<script setup lang="ts">
import { onMounted } from 'vue';
import { useSuppliers } from '../composables/useSuppliers';
import SupplierCard from '../components/SupplierCard.vue';

const { suppliers, loading, error, fetchSuppliers } = useSuppliers();

onMounted(() => {
  fetchSuppliers();
});
</script>

<template>
  <div class="suppliers-view">
    <h1>Our Suppliers</h1>
    
    <div v-if="error" class="error">{{ error }}</div>
    <div v-if="loading">Loading suppliers...</div>
    
    <div v-else class="suppliers-grid">
      <SupplierCard 
        v-for="supplier in suppliers" 
        :key="supplier.id" 
        :supplier="supplier" 
      />
    </div>
  </div>
</template>

<style scoped>
.suppliers-view {
  max-width: 1200px;
  margin: 0 auto;
}
.suppliers-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: 1.5rem;
  margin-top: 1.5rem;
}
.error {
  color: red;
}
</style>
