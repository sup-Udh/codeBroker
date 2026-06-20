import { Request, Response } from 'express';
import { OrderService } from '../services/OrderService';

export class OrderController {
  private orderService: OrderService;

  constructor(orderService: OrderService) {
    this.orderService = orderService;
  }

  public getAllOrders = async (req: Request, res: Response): Promise<void> => {
    try {
      const orders = await this.orderService.getAllOrders();
      res.status(200).json(orders);
    } catch (error) {
      res.status(500).json({ error: 'Internal server error' });
    }
  };

  public getMyOrders = async (req: Request, res: Response): Promise<void> => {
    try {
      const userId = req.user!.userId;
      const orders = await this.orderService.getUserOrders(userId);
      res.status(200).json(orders);
    } catch (error) {
      res.status(500).json({ error: 'Internal server error' });
    }
  };

  public createOrder = async (req: Request, res: Response): Promise<void> => {
    try {
      const userId = req.user!.userId;
      const orderData = { ...req.body, userId };
      
      const order = await this.orderService.createOrder(orderData);
      res.status(201).json(order);
    } catch (error: any) {
      res.status(400).json({ error: error.message || 'Bad request' });
    }
  };

  public getOrderById = async (req: Request, res: Response): Promise<void> => {
    try {
      const id = req.params.id;
      const order = await this.orderService.getOrderById(id);
      
      // Access control
      if (req.user?.role !== 'admin' && req.user?.userId !== order.userId) {
        res.status(403).json({ error: 'Forbidden' });
        return;
      }
      
      res.status(200).json(order);
    } catch (error: any) {
      if (error.message === 'Order not found') {
        res.status(404).json({ error: error.message });
      } else {
        res.status(500).json({ error: 'Internal server error' });
      }
    }
  };
}
