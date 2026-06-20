import express from 'express';
import { UserRepository } from './repositories/UserRepository';
import { OrderRepository } from './repositories/OrderRepository';
import { AuthService } from './services/AuthService';
import { UserService } from './services/UserService';
import { OrderService } from './services/OrderService';
import { AuthController } from './controllers/AuthController';
import { UserController } from './controllers/UserController';
import { OrderController } from './controllers/OrderController';
import { createAuthRouter } from './routes/auth';
import { createUserRouter } from './routes/users';
import { createOrderRouter } from './routes/orders';

const app = express();
const PORT = process.env.PORT || 3000;

// Middleware
app.use(express.json());

// Repositories
const userRepository = new UserRepository();
const orderRepository = new OrderRepository();

// Services
const authService = new AuthService(userRepository);
const userService = new UserService(userRepository);
const orderService = new OrderService(orderRepository);

// Controllers
const authController = new AuthController(authService);
const userController = new UserController(userService);
const orderController = new OrderController(orderService);

// Routes
app.use('/api/auth', createAuthRouter(authController));
app.use('/api/users', createUserRouter(userController));
app.use('/api/orders', createOrderRouter(orderController));

// Health check
app.get('/health', (req, res) => {
  res.status(200).json({ status: 'OK', message: 'API is running' });
});

// 404 handler
app.use((req, res) => {
  res.status(404).json({ error: 'Not found' });
});

// Error handler
app.use((err: any, req: express.Request, res: express.Response, next: express.NextFunction) => {
  console.error(err.stack);
  res.status(500).json({ error: 'Something broke!' });
});

app.listen(PORT, () => {
  console.log(`Server is running on port ${PORT}`);
});
