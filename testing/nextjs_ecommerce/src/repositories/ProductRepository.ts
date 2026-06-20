import { Product } from '@/types/Product';

const mockProducts: Product[] = [
  { id: '1', name: 'Wireless Headphones', description: 'High quality noise-canceling headphones.', price: 199.99, imageUrl: '/images/headphones.jpg', stock: 50 },
  { id: '2', name: 'Mechanical Keyboard', description: 'RGB mechanical keyboard with cherry MX red switches.', price: 129.50, imageUrl: '/images/keyboard.jpg', stock: 20 },
  { id: '3', name: 'Gaming Mouse', description: 'Ergonomic gaming mouse with adjustable DPI.', price: 59.99, imageUrl: '/images/mouse.jpg', stock: 100 },
];

export class ProductRepository {
  static async getAllProducts(): Promise<Product[]> {
    return new Promise(resolve => setTimeout(() => resolve(mockProducts), 100));
  }

  static async getProductById(id: string): Promise<Product | undefined> {
    return new Promise(resolve => setTimeout(() => resolve(mockProducts.find(p => p.id === id)), 100));
  }
}
