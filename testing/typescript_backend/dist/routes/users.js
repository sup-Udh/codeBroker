"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.createUserRouter = void 0;
const express_1 = require("express");
const authMiddleware_1 = require("../middleware/authMiddleware");
const createUserRouter = (userController) => {
    const router = (0, express_1.Router)();
    // All user routes require authentication
    router.use(authMiddleware_1.authenticate);
    router.get('/profile', userController.getProfile);
    router.get('/:id', userController.getUserById);
    // Admin only routes
    router.get('/', authMiddleware_1.requireAdmin, userController.getAllUsers);
    return router;
};
exports.createUserRouter = createUserRouter;
