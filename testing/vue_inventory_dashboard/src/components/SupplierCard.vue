<script setup lang="ts">
import type { Supplier } from '../types/Supplier';

defineProps<{
  supplier: Supplier;
}>();
</script>

<template>
  <div class="supplier-card">
    <h4>{{ supplier.name }}</h4>
    <p><strong>Email:</strong> {{ supplier.contactEmail }}</p>
    <p><strong>Phone:</strong> {{ supplier.phone }}</p>
    <p><strong>Rating:</strong> {{ supplier.rating }} / 5.0</p>
  </div>
</template>

<style scoped>
.supplier-card {
  padding: 1rem;
  border: 1px solid #eee;
  border-radius: 6px;
  background-color: #fafafa;
}
h4 {
  margin-top: 0;
  color: #0056b3;
}
p {
  margin: 0.25rem 0;
  font-size: 0.9rem;
}
</style>
