import CheckoutForm from '@/components/CheckoutForm';
import CartSummary from '@/components/CartSummary';

export default function CheckoutPage() {
  return (
    <div className="max-w-5xl mx-auto">
      <h1 className="text-3xl font-bold mb-8">Checkout</h1>
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-12">
        <div>
          <CheckoutForm />
        </div>
        <div>
          <CartSummary />
        </div>
      </div>
    </div>
  );
}
