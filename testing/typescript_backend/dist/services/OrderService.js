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
exports.OrderService = void 0;
class OrderService {
    constructor(orderRepository) {
        this.orderRepository = orderRepository;
    }
    getAllOrders() {
        return __awaiter(this, void 0, void 0, function* () {
            return this.orderRepository.findAll();
        });
    }
    getOrderById(id) {
        return __awaiter(this, void 0, void 0, function* () {
            const order = yield this.orderRepository.findById(id);
            if (!order) {
                throw new Error('Order not found');
            }
            return order;
        });
    }
    getUserOrders(userId) {
        return __awaiter(this, void 0, void 0, function* () {
            return this.orderRepository.findByUserId(userId);
        });
    }
    createOrder(data) {
        return __awaiter(this, void 0, void 0, function* () {
            if (!data.items || data.items.length === 0) {
                throw new Error('Order must contain at least one item');
            }
            // Default status
            if (!data.status) {
                data.status = 'pending';
            }
            return this.orderRepository.create(data);
        });
    }
    updateOrderStatus(id, status) {
        return __awaiter(this, void 0, void 0, function* () {
            const order = yield this.getOrderById(id);
            return this.orderRepository.update(id, { status });
        });
    }
}
exports.OrderService = OrderService;
