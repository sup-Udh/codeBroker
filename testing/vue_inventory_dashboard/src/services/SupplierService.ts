import { supplierRepository } from '../repositories/SupplierRepository';
import type { Supplier } from '../types/Supplier';

export class SupplierService {
  async getAllSuppliers(): Promise<Supplier[]> {
    return supplierRepository.getAll();
  }

  async getTopRatedSuppliers(minRating: number = 4.0): Promise<Supplier[]> {
    const suppliers = await supplierRepository.getAll();
    return suppliers.filter(s => s.rating >= minRating).sort((a, b) => b.rating - a.rating);
  }
}

export const supplierService = new SupplierService();
