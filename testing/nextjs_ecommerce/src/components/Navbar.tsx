'use client';

import React from 'react';
import Link from 'next/link';
import { useCart } from '@/hooks/useCart';

export default function Navbar() {
  const { cart } = useCart();

  return (
    <nav className="bg-gray-800 text-white p-4 shadow-md">
      <div className="container mx-auto flex justify-between items-center">
        <Link href="/" className="text-xl font-bold tracking-tight">NextShop</Link>
        <div className="space-x-6 flex items-center">
          <Link href="/products" className="hover:text-gray-300 transition-colors">Products</Link>
          <Link href="/cart" className="hover:text-gray-300 transition-colors">
            Cart ({cart.totalItems})
          </Link>
          <Link href="/checkout" className="bg-blue-600 px-4 py-2 rounded hover:bg-blue-700 transition-colors">
            Checkout
          </Link>
        </div>
      </div>
    </nav>
  );
}
