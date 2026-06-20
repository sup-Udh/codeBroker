import CartSummary from '@/components/CartSummary';
import Link from 'next/link';

export default function CartPage() {
  return (
    <div className="max-w-4xl mx-auto">
      <h1 className="text-3xl font-bold mb-8">Shopping Cart</h1>
      <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
        <div className="md:col-span-2">
          <CartSummary />
        </div>
        <div>
          <div className="bg-gray-50 p-6 rounded-lg border">
            <h3 className="font-bold text-lg mb-4">Next Steps</h3>
            <p className="text-gray-600 mb-4 text-sm">Review your items and proceed to checkout.</p>
            <Link href="/checkout" className="block w-full text-center bg-blue-600 text-white py-2 px-4 rounded hover:bg-blue-700">
              Proceed to Checkout
            </Link>
          </div>
        </div>
      </div>
    </div>
  );
}
