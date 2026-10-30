import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { fileURLToPath, URL } from 'node:url';
import { runFilesPlugin } from './server/runFiles';

export default defineConfig({
  plugins: [runFilesPlugin(), react(), tailwindcss()],
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  server: { host: '127.0.0.1', port: 5173, watch: { ignored: ['**/public/runs/**'] }, proxy: { '/api': 'http://127.0.0.1:5174' } },
  preview: { host: '127.0.0.1', port: 4173 },
});
