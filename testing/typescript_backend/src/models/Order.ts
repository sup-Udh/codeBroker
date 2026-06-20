export interface OrderItem {
  productId: string;
  quantity: number;
  price: number;
}

export interface Order {
  id: string;
  userId: string;
  items: OrderItem[];
  totalAmount: number;
  status: 'pending' | 'paid' | 'shipped' | 'delivered' | 'cancelled';
  shippingAddress: string;
  createdAt: Date;
  updatedAt: Date;
}

export type OrderCreateDTO = Omit<Order, 'id' | 'createdAt' | 'updatedAt' | 'totalAmount'>;
export type OrderUpdateDTO = Partial<Omit<Order, 'id' | 'createdAt' | 'updatedAt'>>;
