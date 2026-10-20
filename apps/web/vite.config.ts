import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Cross-origin isolation enables SharedArrayBuffer, which the lock-free
// command ring between the UI and the audio thread relies on. Production
// hosting must send the same headers.
const crossOriginIsolation = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
};

export default defineConfig({
  plugins: [react()],
  server: { headers: crossOriginIsolation },
  preview: { headers: crossOriginIsolation },
});
