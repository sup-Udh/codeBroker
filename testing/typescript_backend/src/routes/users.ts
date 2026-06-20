import { Router } from 'express';
import { UserController } from '../controllers/UserController';
import { authenticate, requireAdmin } from '../middleware/authMiddleware';

export const createUserRouter = (userController: UserController): Router => {
  const router = Router();

  // All user routes require authentication
  router.use(authenticate);

  router.get('/profile', userController.getProfile);
  router.get('/:id', userController.getUserById);
  
  // Admin only routes
  router.get('/', requireAdmin, userController.getAllUsers);

  return router;
};
