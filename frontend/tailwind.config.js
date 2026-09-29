/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        primary: {
          50: '#f7f7f7',
          100: '#ececec',
          200: '#dcdcdc',
          300: '#c8c8c8',
          400: '#aaaaaa',
          500: '#888888',
          600: '#111111',
          700: '#000000',
          800: '#000000',
          900: '#000000',
        },
      },
    },
  },
  plugins: [],
}