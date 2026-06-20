import { Router } from 'express';
import { OrderController } from '../controllers/OrderController';
import { authenticate, requireAdmin } from '../middleware/authMiddleware';

export const createOrderRouter = (orderController: OrderController): Router => {
  const router = Router();

  // All order routes require authentication
  router.use(authenticate);

  router.post('/', orderController.createOrder);
  router.get('/my-orders', orderController.getMyOrders);
  router.get('/:id', orderController.getOrderById);
  
  // Admin only routes
  router.get('/', requireAdmin, orderController.getAllOrders);

  return router;
};
