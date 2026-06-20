'use client';

import React from 'react';
import { useProducts } from '@/hooks/useProducts';
import ProductCard from './ProductCard';

export default function ProductList() {
  const { products, loading } = useProducts();

  if (loading) {
    return <div className="text-center py-10">Loading products...</div>;
  }

  return (
    <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
      {products.map(product => (
        <ProductCard key={product.id} product={product} />
      ))}
    </div>
  );
}
