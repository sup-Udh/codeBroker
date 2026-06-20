import { Order, OrderCreateDTO, OrderUpdateDTO } from '../models/Order';
import * as crypto from 'crypto';

export class OrderRepository {
  private orders: Map<string, Order> = new Map();

  public async findAll(): Promise<Order[]> {
    return Array.from(this.orders.values());
  }

  public async findById(id: string): Promise<Order | null> {
    return this.orders.get(id) || null;
  }

  public async findByUserId(userId: string): Promise<Order[]> {
    const result: Order[] = [];
    for (const order of this.orders.values()) {
      if (order.userId === userId) {
        result.push(order);
      }
    }
    return result;
  }

  public async create(data: OrderCreateDTO): Promise<Order> {
    const id = crypto.randomUUID();
    const now = new Date();
    
    // Calculate total amount from items
    const totalAmount = data.items.reduce((sum, item) => sum + (item.price * item.quantity), 0);
    
    const newOrder: Order = {
      id,
      ...data,
      totalAmount,
      createdAt: now,
      updatedAt: now
    };
    this.orders.set(id, newOrder);
    return newOrder;
  }

  public async update(id: string, data: OrderUpdateDTO): Promise<Order | null> {
    const existing = this.orders.get(id);
    if (!existing) return null;

    const updated: Order = {
      ...existing,
      ...data,
      updatedAt: new Date()
    };
    
    // Recalculate total if items changed
    if (data.items) {
      updated.totalAmount = data.items.reduce((sum, item) => sum + (item.price * item.quantity), 0);
    }
    
    this.orders.set(id, updated);
    return updated;
  }

  public async delete(id: string): Promise<boolean> {
    return this.orders.delete(id);
  }
}
