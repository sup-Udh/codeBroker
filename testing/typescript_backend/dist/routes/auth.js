"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.createAuthRouter = void 0;
const express_1 = require("express");
const createAuthRouter = (authController) => {
    const router = (0, express_1.Router)();
    router.post('/register', authController.register);
    router.post('/login', authController.login);
    return router;
};
exports.createAuthRouter = createAuthRouter;
