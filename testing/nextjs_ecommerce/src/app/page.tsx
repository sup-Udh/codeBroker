import Link from 'next/link';
import ProductList from '@/components/ProductList';

export default function Home() {
  return (
    <div className="space-y-12">
      <section className="text-center py-12 bg-gray-50 rounded-lg">
        <h1 className="text-4xl font-extrabold text-gray-900 mb-4">Welcome to NextShop</h1>
        <p className="text-xl text-gray-600 mb-8">Discover our featured products and great deals.</p>
        <Link href="/products" className="bg-gray-800 text-white px-6 py-3 rounded-md text-lg font-medium hover:bg-gray-900">
          Shop Now
        </Link>
      </section>

      <section>
        <h2 className="text-3xl font-bold mb-6">Featured Products</h2>
        <ProductList />
      </section>
    </div>
  );
}
