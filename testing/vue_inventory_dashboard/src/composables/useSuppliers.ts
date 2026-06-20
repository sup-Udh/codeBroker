import { ref, readonly } from 'vue';
import { supplierService } from '../services/SupplierService';
import type { Supplier } from '../types/Supplier';

export function useSuppliers() {
  const suppliers = ref<Supplier[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  const fetchSuppliers = async () => {
    loading.value = true;
    error.value = null;
    try {
      suppliers.value = await supplierService.getAllSuppliers();
    } catch (e) {
      error.value = e instanceof Error ? e.message : 'Failed to fetch suppliers';
    } finally {
      loading.value = false;
    }
  };

  return {
    suppliers: readonly(suppliers),
    loading: readonly(loading),
    error: readonly(error),
    fetchSuppliers
  };
}
