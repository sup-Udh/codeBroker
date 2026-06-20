import { Cart } from './Cart';

export interface Order {
  id: string;
  cart: Cart;
  customerName: string;
  email: string;
  shippingAddress: string;
  status: 'PENDING' | 'PROCESSING' | 'COMPLETED' | 'CANCELLED';
  createdAt: Date;
}

export interface OrderSummary {
  itemCount: number;
  totalValue: number;
}
