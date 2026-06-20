"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
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
exports.OrderRepository = void 0;
const crypto = __importStar(require("crypto"));
class OrderRepository {
    constructor() {
        this.orders = new Map();
    }
    findAll() {
        return __awaiter(this, void 0, void 0, function* () {
            return Array.from(this.orders.values());
        });
    }
    findById(id) {
        return __awaiter(this, void 0, void 0, function* () {
            return this.orders.get(id) || null;
        });
    }
    findByUserId(userId) {
        return __awaiter(this, void 0, void 0, function* () {
            const result = [];
            for (const order of this.orders.values()) {
                if (order.userId === userId) {
                    result.push(order);
                }
            }
            return result;
        });
    }
    create(data) {
        return __awaiter(this, void 0, void 0, function* () {
            const id = crypto.randomUUID();
            const now = new Date();
            // Calculate total amount from items
            const totalAmount = data.items.reduce((sum, item) => sum + (item.price * item.quantity), 0);
            const newOrder = Object.assign(Object.assign({ id }, data), { totalAmount, createdAt: now, updatedAt: now });
            this.orders.set(id, newOrder);
            return newOrder;
        });
    }
    update(id, data) {
        return __awaiter(this, void 0, void 0, function* () {
            const existing = this.orders.get(id);
            if (!existing)
                return null;
            const updated = Object.assign(Object.assign(Object.assign({}, existing), data), { updatedAt: new Date() });
            // Recalculate total if items changed
            if (data.items) {
                updated.totalAmount = data.items.reduce((sum, item) => sum + (item.price * item.quantity), 0);
            }
            this.orders.set(id, updated);
            return updated;
        });
    }
    delete(id) {
        return __awaiter(this, void 0, void 0, function* () {
            return this.orders.delete(id);
        });
    }
}
exports.OrderRepository = OrderRepository;
