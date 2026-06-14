import type { NextConfig } from 'next';

const nextConfig: NextConfig = {
  output: 'export',
  images: { unoptimized: true },
  trailingSlash: true,
  // Dev proxy: forward API requests to Rust backend
  async rewrites() {
    return [
      { source: '/v1/:path*', destination: 'http://127.0.0.1:5100/v1/:path*' },
      { source: '/api/:path*', destination: 'http://127.0.0.1:5100/api/:path*' },
    ];
  },
};

export default nextConfig;

