// The browser talks only to Next; /api/* is proxied to the local FastAPI backend.
// `standalone` output lets the desktop installer ship a small self-contained UI server (no node_modules, no npm).
const backend = process.env.AO_BACKEND_URL || "http://127.0.0.1:8000";
export default {
  output: "standalone",
  async rewrites() {
    return [{ source: "/api/:path*", destination: `${backend}/api/:path*` }];
  },
};
