import { Cart, CartItem } from '@/types/Cart';
import { Product } from '@/types/Product';
import { CheckoutService } from './CheckoutService';

export class CartService {
  private static cart: Cart = { items: [], totalItems: 0, totalPrice: 0, toOrderSummary() { return { itemCount: this.totalItems, totalValue: this.totalPrice }; } };

  static getCart(): Cart {
    return this.cart;
  }

  static addToCart(product: Product, quantity: number): void {
    const existing = this.cart.items.find(i => i.product.id === product.id);
    if (existing) {
      existing.quantity += quantity;
    } else {
      this.cart.items.push({ product, quantity });
    }
    this.recalculate();
  }

  static clearCart(): void {
    this.cart.items = [];
    this.recalculate();
  }

  private static recalculate() {
    this.cart.totalItems = this.cart.items.reduce((sum, item) => sum + item.quantity, 0);
    this.cart.totalPrice = this.cart.items.reduce((sum, item) => sum + (item.product.price * item.quantity), 0);
  }

  static initiateCheckout(): boolean {
    return CheckoutService.processCart(this.cart);
  }
}
