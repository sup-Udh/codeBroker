<script setup lang="ts">
import type { InventoryItem } from '../types/InventoryItem';
import { formatCurrency, formatDate } from '../utils/formatter';

defineProps<{
  items: InventoryItem[]
}>();

const emit = defineEmits<{
  (e: 'restock', id: string): void
}>();
</script>

<template>
  <div class="table-container">
    <table>
      <thead>
        <tr>
          <th>SKU</th>
          <th>Name</th>
          <th>Quantity</th>
          <th>Price</th>
          <th>Last Updated</th>
          <th>Actions</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="item in items" :key="item.id" :class="{ 'low-stock': item.quantity < 50 }">
          <td>{{ item.sku }}</td>
          <td>{{ item.name }}</td>
          <td>{{ item.quantity }}</td>
          <td>{{ formatCurrency(item.price) }}</td>
          <td>{{ formatDate(item.lastUpdated) }}</td>
          <td>
            <button @click="emit('restock', item.id)">Restock</button>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>

<style scoped>
.table-container {
  overflow-x: auto;
}
table {
  width: 100%;
  border-collapse: collapse;
}
th, td {
  padding: 0.75rem;
  text-align: left;
  border-bottom: 1px solid #ddd;
}
.low-stock {
  background-color: #ffeaea;
}
button {
  padding: 0.25rem 0.5rem;
  background-color: #28a745;
  color: white;
  border: none;
  border-radius: 4px;
  cursor: pointer;
}
button:hover {
  background-color: #218838;
}
</style>
