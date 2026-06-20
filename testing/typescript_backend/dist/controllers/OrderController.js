"use strict";
var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.OrderController = void 0;
class OrderController {
    constructor(orderService) {
        this.getAllOrders = (req, res) => __awaiter(this, void 0, void 0, function* () {
            try {
                const orders = yield this.orderService.getAllOrders();
                res.status(200).json(orders);
            }
            catch (error) {
                res.status(500).json({ error: 'Internal server error' });
            }
        });
        this.getMyOrders = (req, res) => __awaiter(this, void 0, void 0, function* () {
            try {
                const userId = req.user.userId;
                const orders = yield this.orderService.getUserOrders(userId);
                res.status(200).json(orders);
            }
            catch (error) {
                res.status(500).json({ error: 'Internal server error' });
            }
        });
        this.createOrder = (req, res) => __awaiter(this, void 0, void 0, function* () {
            try {
                const userId = req.user.userId;
                const orderData = Object.assign(Object.assign({}, req.body), { userId });
                const order = yield this.orderService.createOrder(orderData);
                res.status(201).json(order);
            }
            catch (error) {
                res.status(400).json({ error: error.message || 'Bad request' });
            }
        });
        this.getOrderById = (req, res) => __awaiter(this, void 0, void 0, function* () {
            var _a, _b;
            try {
                const id = req.params.id;
                const order = yield this.orderService.getOrderById(id);
                // Access control
                if (((_a = req.user) === null || _a === void 0 ? void 0 : _a.role) !== 'admin' && ((_b = req.user) === null || _b === void 0 ? void 0 : _b.userId) !== order.userId) {
                    res.status(403).json({ error: 'Forbidden' });
                    return;
                }
                res.status(200).json(order);
            }
            catch (error) {
                if (error.message === 'Order not found') {
                    res.status(404).json({ error: error.message });
                }
                else {
                    res.status(500).json({ error: 'Internal server error' });
                }
            }
        });
        this.orderService = orderService;
    }
}
exports.OrderController = OrderController;
