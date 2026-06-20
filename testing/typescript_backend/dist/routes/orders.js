"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.createOrderRouter = void 0;
const express_1 = require("express");
const authMiddleware_1 = require("../middleware/authMiddleware");
const createOrderRouter = (orderController) => {
    const router = (0, express_1.Router)();
    // All order routes require authentication
    router.use(authMiddleware_1.authenticate);
    router.post('/', orderController.createOrder);
    router.get('/my-orders', orderController.getMyOrders);
    router.get('/:id', orderController.getOrderById);
    // Admin only routes
    router.get('/', authMiddleware_1.requireAdmin, orderController.getAllOrders);
    return router;
};
exports.createOrderRouter = createOrderRouter;
