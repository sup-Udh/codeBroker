import { Cart } from '@/types/Cart';
import { Order } from '@/types/Order';
import { CartService } from './CartService';

export class CheckoutService {
  static processCart(cart: Cart): boolean {
    if (cart.totalItems === 0) return false;
    return true;
  }

  static createOrder(customerDetails: any): Order | null {
    const cart = CartService.getCart();
    if (cart.totalItems === 0) return null;
    
    const order: Order = {
      id: Math.random().toString(36).substring(7),
      cart: { ...cart },
      customerName: customerDetails.name,
      email: customerDetails.email,
      shippingAddress: customerDetails.address,
      status: 'PENDING',
      createdAt: new Date()
    };
    
    CartService.clearCart();
    return order;
  }
}
