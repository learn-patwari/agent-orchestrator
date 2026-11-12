// Two builds:
//  - AO_STATIC=1 (the Rust desktop app): a static export. The Rust backend serves these files and the API from one port,
//    so the browser calls /api/* on the same origin and nothing proxies.
//  - default (the Electron/Python app): `standalone` server, with /api/* proxied to the local backend.
const backend = process.env.AO_BACKEND_URL || "http://127.0.0.1:8000";
const config = process.env.AO_STATIC === "1"
  ? { output: "export", images: { unoptimized: true } }
  : {
      output: "standalone",
      async rewrites() {
        return [{ source: "/api/:path*", destination: `${backend}/api/:path*` }];
      },
    };
export default config;
