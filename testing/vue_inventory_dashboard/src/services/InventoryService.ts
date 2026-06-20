import { inventoryRepository } from '../repositories/InventoryRepository';
import type { InventoryItem } from '../types/InventoryItem';

export class InventoryService {
  async getInventoryOverview() {
    const items = await inventoryRepository.getAll();
    const totalItems = items.reduce((acc, item) => acc + item.quantity, 0);
    const totalValue = items.reduce((acc, item) => acc + (item.quantity * item.price), 0);
    const lowStockItems = items.filter(item => item.quantity < 50);

    return {
      items,
      totalItems,
      totalValue,
      lowStockItems
    };
  }

  async restockItem(id: string, amount: number): Promise<InventoryItem> {
    const item = await inventoryRepository.getById(id);
    if (!item) throw new Error('Item not found');
    
    return inventoryRepository.update(id, { quantity: item.quantity + amount });
  }
}

export const inventoryService = new InventoryService();
