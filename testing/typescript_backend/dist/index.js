"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const express_1 = __importDefault(require("express"));
const UserRepository_1 = require("./repositories/UserRepository");
const OrderRepository_1 = require("./repositories/OrderRepository");
const AuthService_1 = require("./services/AuthService");
const UserService_1 = require("./services/UserService");
const OrderService_1 = require("./services/OrderService");
const AuthController_1 = require("./controllers/AuthController");
const UserController_1 = require("./controllers/UserController");
const OrderController_1 = require("./controllers/OrderController");
const auth_1 = require("./routes/auth");
const users_1 = require("./routes/users");
const orders_1 = require("./routes/orders");
const app = (0, express_1.default)();
const PORT = process.env.PORT || 3000;
// Middleware
app.use(express_1.default.json());
// Repositories
const userRepository = new UserRepository_1.UserRepository();
const orderRepository = new OrderRepository_1.OrderRepository();
// Services
const authService = new AuthService_1.AuthService(userRepository);
const userService = new UserService_1.UserService(userRepository);
const orderService = new OrderService_1.OrderService(orderRepository);
// Controllers
const authController = new AuthController_1.AuthController(authService);
const userController = new UserController_1.UserController(userService);
const orderController = new OrderController_1.OrderController(orderService);
// Routes
app.use('/api/auth', (0, auth_1.createAuthRouter)(authController));
app.use('/api/users', (0, users_1.createUserRouter)(userController));
app.use('/api/orders', (0, orders_1.createOrderRouter)(orderController));
// Health check
app.get('/health', (req, res) => {
    res.status(200).json({ status: 'OK', message: 'API is running' });
});
// 404 handler
app.use((req, res) => {
    res.status(404).json({ error: 'Not found' });
});
// Error handler
app.use((err, req, res, next) => {
    console.error(err.stack);
    res.status(500).json({ error: 'Something broke!' });
});
app.listen(PORT, () => {
    console.log(`Server is running on port ${PORT}`);
});
