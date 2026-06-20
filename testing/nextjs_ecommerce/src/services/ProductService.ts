import { Product } from '@/types/Product';
import { ProductRepository } from '@/repositories/ProductRepository';

export class ProductService {
  static async getAvailableProducts(): Promise<Product[]> {
    const products = await ProductRepository.getAllProducts();
    return products.filter(p => p.stock > 0);
  }

  static async getProductDetails(id: string): Promise<Product | undefined> {
    return await ProductRepository.getProductById(id);
  }
}
