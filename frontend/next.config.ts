import type { NextConfig } from "next";

// Keep browser requests same-origin so the backend's Cookie Session works
// without exposing a separate CORS-enabled endpoint.
const backendUrl = process.env.BACKEND_URL ?? "http://127.0.0.1:3001";

const nextConfig: NextConfig = {
  // Permit this development machine's LAN address so client-side hydration is not blocked.
  allowedDevOrigins: ["192.168.1.100"],
  experimental: {
    // Paper PDFs are accepted up to 100 MiB by the Rust backend. External rewrites
    // are proxied by Next.js, whose default buffered proxy body limit is smaller.
    // Leave room for multipart/form-data headers as well as the PDF payload itself.
    proxyClientMaxBodySize: "101mb",
  },
  async rewrites() {
    return [
      {
        source: "/api/backend/:path*",
        destination: `${backendUrl}/:path*`,
      },
    ];
  },
};

export default nextConfig;
