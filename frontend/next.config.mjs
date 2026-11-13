// Static export: the Rust backend serves these files and the API from one port, so the browser calls /api/* on the
// same origin and nothing proxies. Build with `build-frontend-static.cmd`.
export default { output: "export", images: { unoptimized: true } };
