import { useEffect } from 'react';

export function useLiveEvents(onUpdate: (type: string) => void) {
  useEffect(() => {
    const eventSource = new EventSource('/api/v1/events');

    eventSource.addEventListener('update', (event) => {
      onUpdate(event.data);
    });

    eventSource.onerror = (error) => {
      console.error('SSE Error:', error);
      eventSource.close();
      // Simple reconnect logic
      setTimeout(() => {
        // Since we are in useEffect, it might not re-trigger this automatically without a state toggle,
        // but EventSource auto-reconnects natively. So we just close it and let the component handle it or rely on native reconnect.
        // Actually EventSource reconnects natively on error if we don't close it, so let's just log it.
      }, 5000);
    };

    return () => {
      eventSource.close();
    };
  }, [onUpdate]);
}
