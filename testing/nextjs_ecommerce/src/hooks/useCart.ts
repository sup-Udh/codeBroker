import { useState, useCallback, useEffect } from 'react';
import { Cart } from '@/types/Cart';
import { Product } from '@/types/Product';
import { CartService } from '@/services/CartService';

export function useCart() {
  const [cart, setCart] = useState<Cart>(CartService.getCart());
  const [, setTick] = useState(0);

  const updateState = useCallback(() => {
    setCart({ ...CartService.getCart() });
    setTick(t => t + 1);
  }, []);

  const addToCart = useCallback((product: Product, quantity: number) => {
    CartService.addToCart(product, quantity);
    updateState();
  }, [updateState]);

  const clearCart = useCallback(() => {
    CartService.clearCart();
    updateState();
  }, [updateState]);

  return { cart, addToCart, clearCart };
}
