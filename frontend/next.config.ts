import type { NextConfig } from "next";

// Keep browser requests same-origin so the backend's Cookie Session works
// without exposing a separate CORS-enabled endpoint.
const backendUrl = process.env.BACKEND_URL ?? "http://127.0.0.1:3000";

const nextConfig: NextConfig = {
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
