import { Router } from 'express';
import { AuthController } from '../controllers/AuthController';

export const createAuthRouter = (authController: AuthController): Router => {
  const router = Router();

  router.post('/register', authController.register);
  router.post('/login', authController.login);

  return router;
};
