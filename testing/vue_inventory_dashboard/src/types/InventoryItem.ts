export interface InventoryItem {
  id: string;
  name: string;
  sku: string;
  quantity: number;
  price: number;
  supplierId: string;
  lastUpdated: Date;
}
