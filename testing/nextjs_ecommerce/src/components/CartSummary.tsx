'use client';

import React from 'react';
import { useCart } from '@/hooks/useCart';
import { formatCurrency } from '@/utils/currency';

export default function CartSummary() {
  const { cart } = useCart();

  if (cart.items.length === 0) {
    return <div className="p-4 border rounded bg-gray-50 text-center">Your cart is empty.</div>;
  }

  return (
    <div className="p-6 border rounded-lg bg-white shadow-sm">
      <h2 className="text-2xl font-bold mb-4">Order Summary</h2>
      <ul className="space-y-3 mb-6">
        {cart.items.map(item => (
          <li key={item.product.id} className="flex justify-between">
            <span>{item.quantity}x {item.product.name}</span>
            <span>{formatCurrency(item.product.price * item.quantity)}</span>
          </li>
        ))}
      </ul>
      <div className="border-t pt-4 flex justify-between font-bold text-lg">
        <span>Total ({cart.totalItems} items)</span>
        <span>{formatCurrency(cart.totalPrice)}</span>
      </div>
    </div>
  );
}
