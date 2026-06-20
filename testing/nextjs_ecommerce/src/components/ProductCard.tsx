'use client';

import React from 'react';
import { Product } from '@/types/Product';
import { formatCurrency } from '@/utils/currency';
import { useCart } from '@/hooks/useCart';

interface ProductCardProps {
  product: Product;
}

export default function ProductCard({ product }: ProductCardProps) {
  const { addToCart } = useCart();

  return (
    <div className="border rounded-lg p-4 shadow-sm hover:shadow-md transition-shadow">
      <div className="h-48 bg-gray-200 mb-4 rounded flex items-center justify-center text-gray-500">
        [Image: {product.name}]
      </div>
      <h3 className="text-lg font-semibold">{product.name}</h3>
      <p className="text-gray-600 text-sm mb-2">{product.description}</p>
      <div className="flex items-center justify-between mt-4">
        <span className="text-xl font-bold">{formatCurrency(product.price)}</span>
        <button
          onClick={() => addToCart(product, 1)}
          className="bg-blue-600 text-white px-4 py-2 rounded hover:bg-blue-700"
        >
          Add to Cart
        </button>
      </div>
    </div>
  );
}
