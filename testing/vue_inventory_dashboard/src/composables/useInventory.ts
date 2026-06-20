import { ref, readonly } from 'vue';
import { inventoryService } from '../services/InventoryService';
import type { InventoryItem } from '../types/InventoryItem';

export function useInventory() {
  const items = ref<InventoryItem[]>([]);
  const totalItems = ref(0);
  const totalValue = ref(0);
  const lowStockItems = ref<InventoryItem[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  const fetchInventory = async () => {
    loading.value = true;
    error.value = null;
    try {
      const data = await inventoryService.getInventoryOverview();
      items.value = data.items;
      totalItems.value = data.totalItems;
      totalValue.value = data.totalValue;
      lowStockItems.value = data.lowStockItems;
    } catch (e) {
      error.value = e instanceof Error ? e.message : 'Failed to fetch inventory';
    } finally {
      loading.value = false;
    }
  };

  const restock = async (id: string, amount: number) => {
    try {
      await inventoryService.restockItem(id, amount);
      await fetchInventory(); // Refresh data
    } catch (e) {
      error.value = e instanceof Error ? e.message : 'Failed to restock item';
    }
  };

  return {
    items: readonly(items),
    totalItems: readonly(totalItems),
    totalValue: readonly(totalValue),
    lowStockItems: readonly(lowStockItems),
    loading: readonly(loading),
    error: readonly(error),
    fetchInventory,
    restock
  };
}
