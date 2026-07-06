/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        background: '#09090b', // Black/very dark gray
        card: '#111113', // Slightly lighter dark gray for cards
        border: '#1f1f22', // Subtle borders
        primary: '#ff6b35', // Orange highlight
        muted: '#71717a', // Muted text
        foreground: '#fafafa', // White text
      }
    },
  },
  plugins: [],
}
