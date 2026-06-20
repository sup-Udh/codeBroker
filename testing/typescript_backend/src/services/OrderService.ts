import { OrderRepository } from '../repositories/OrderRepository';
import { Order, OrderCreateDTO, OrderUpdateDTO } from '../models/Order';

export class OrderService {
  private orderRepository: OrderRepository;

  constructor(orderRepository: OrderRepository) {
    this.orderRepository = orderRepository;
  }

  public async getAllOrders(): Promise<Order[]> {
    return this.orderRepository.findAll();
  }

  public async getOrderById(id: string): Promise<Order> {
    const order = await this.orderRepository.findById(id);
    if (!order) {
      throw new Error('Order not found');
    }
    return order;
  }

  public async getUserOrders(userId: string): Promise<Order[]> {
    return this.orderRepository.findByUserId(userId);
  }

  public async createOrder(data: OrderCreateDTO): Promise<Order> {
    if (!data.items || data.items.length === 0) {
      throw new Error('Order must contain at least one item');
    }
    
    // Default status
    if (!data.status) {
      data.status = 'pending';
    }
    
    return this.orderRepository.create(data);
  }

  public async updateOrderStatus(id: string, status: Order['status']): Promise<Order> {
    const order = await this.getOrderById(id);
    return this.orderRepository.update(id, { status }) as Promise<Order>;
  }
}
