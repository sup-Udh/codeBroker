import type { Supplier } from '../types/Supplier';

export class SupplierRepository {
  private suppliers: Supplier[] = [
    {
      id: 's1',
      name: 'TechGear Solutions',
      contactEmail: 'contact@techgear.com',
      phone: '+1-555-0198',
      address: '123 Tech Park, Silicon Valley',
      rating: 4.8
    },
    {
      id: 's2',
      name: 'Peripheral Pro',
      contactEmail: 'sales@periphpro.com',
      phone: '+1-555-0234',
      address: '456 Keyboard Ave, Austin',
      rating: 4.5
    }
  ];

  async getAll(): Promise<Supplier[]> {
    return new Promise((resolve) => {
      setTimeout(() => resolve([...this.suppliers]), 400);
    });
  }

  async getById(id: string): Promise<Supplier | undefined> {
    return new Promise((resolve) => {
      setTimeout(() => resolve(this.suppliers.find(s => s.id === id)), 200);
    });
  }
}

export const supplierRepository = new SupplierRepository();
