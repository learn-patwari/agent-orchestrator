// The browser talks only to Next; /api/* is proxied to the local FastAPI backend.
const backend = process.env.AO_BACKEND_URL || "http://127.0.0.1:8000";
export default {
  async rewrites() {
    return [{ source: "/api/:path*", destination: `${backend}/api/:path*` }];
  },
};
