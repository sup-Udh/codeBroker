import { Product } from './Product';
import { OrderSummary } from './Order';

export interface CartItem {
  product: Product;
  quantity: number;
}

export interface Cart {
  items: CartItem[];
  totalItems: number;
  totalPrice: number;
  toOrderSummary(): OrderSummary;
}
