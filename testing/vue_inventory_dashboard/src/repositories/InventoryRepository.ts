import type { InventoryItem } from '../types/InventoryItem';

export class InventoryRepository {
  private items: InventoryItem[] = [
    {
      id: '1',
      name: 'Wireless Mouse',
      sku: 'WM-001',
      quantity: 150,
      price: 29.99,
      supplierId: 's1',
      lastUpdated: new Date()
    },
    {
      id: '2',
      name: 'Mechanical Keyboard',
      sku: 'MK-002',
      quantity: 85,
      price: 129.99,
      supplierId: 's2',
      lastUpdated: new Date()
    },
    {
      id: '3',
      name: '27-inch Monitor',
      sku: 'MN-003',
      quantity: 42,
      price: 249.99,
      supplierId: 's1',
      lastUpdated: new Date()
    }
  ];

  async getAll(): Promise<InventoryItem[]> {
    return new Promise((resolve) => {
      setTimeout(() => resolve([...this.items]), 500);
    });
  }

  async getById(id: string): Promise<InventoryItem | undefined> {
    return new Promise((resolve) => {
      setTimeout(() => resolve(this.items.find(i => i.id === id)), 200);
    });
  }

  async update(id: string, updates: Partial<InventoryItem>): Promise<InventoryItem> {
    return new Promise((resolve, reject) => {
      setTimeout(() => {
        const index = this.items.findIndex(i => i.id === id);
        if (index === -1) reject(new Error('Item not found'));
        
        this.items[index] = { ...this.items[index], ...updates, lastUpdated: new Date() };
        resolve(this.items[index]);
      }, 300);
    });
  }
}

export const inventoryRepository = new InventoryRepository();
