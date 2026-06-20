import { useState, useEffect } from 'react';
import { Product } from '@/types/Product';
import { ProductService } from '@/services/ProductService';

export function useProducts() {
  const [products, setProducts] = useState<Product[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    ProductService.getAvailableProducts().then(data => {
      setProducts(data);
      setLoading(false);
    });
  }, []);

  return { products, loading };
}
