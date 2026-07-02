import type { NextConfig } from "next";

// Keep browser requests same-origin so the backend's Cookie Session works
// without exposing a separate CORS-enabled endpoint.
const backendUrl = process.env.BACKEND_URL ?? "http://127.0.0.1:3001";

const nextConfig: NextConfig = {
  // Permit this development machine's LAN address so client-side hydration is not blocked.
  allowedDevOrigins: ["192.168.1.100"],
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
