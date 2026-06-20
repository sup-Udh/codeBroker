'use client';

import React, { useState } from 'react';
import { isValidEmail, isValidAddress } from '@/utils/validation';
import { CheckoutService } from '@/services/CheckoutService';
import { useRouter } from 'next/navigation';

export default function CheckoutForm() {
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [address, setAddress] = useState('');
  const [error, setError] = useState('');
  const router = useRouter();

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!name || !isValidEmail(email) || !isValidAddress(address)) {
      setError('Please provide valid details.');
      return;
    }
    
    const order = CheckoutService.createOrder({ name, email, address });
    if (order) {
      alert(`Order ${order.id} placed successfully!`);
      router.push('/');
    } else {
      setError('Failed to place order. Cart might be empty.');
    }
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-4 max-w-md mx-auto p-6 border rounded shadow-sm">
      <h2 className="text-2xl font-bold mb-4">Checkout Details</h2>
      {error && <p className="text-red-500 text-sm">{error}</p>}
      
      <div>
        <label className="block text-sm font-medium text-gray-700">Name</label>
        <input type="text" value={name} onChange={e => setName(e.target.value)} className="mt-1 block w-full rounded-md border-gray-300 shadow-sm border p-2" required />
      </div>
      
      <div>
        <label className="block text-sm font-medium text-gray-700">Email</label>
        <input type="email" value={email} onChange={e => setEmail(e.target.value)} className="mt-1 block w-full rounded-md border-gray-300 shadow-sm border p-2" required />
      </div>

      <div>
        <label className="block text-sm font-medium text-gray-700">Shipping Address</label>
        <textarea value={address} onChange={e => setAddress(e.target.value)} className="mt-1 block w-full rounded-md border-gray-300 shadow-sm border p-2" rows={3} required></textarea>
      </div>

      <button type="submit" className="w-full bg-green-600 text-white py-2 px-4 rounded hover:bg-green-700">
        Place Order
      </button>
    </form>
  );
}
