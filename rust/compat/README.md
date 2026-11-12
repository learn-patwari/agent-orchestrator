# Python and Rust backends: compatibility checks

Run from the repository root with `python -I`. They need the backend's Python dependencies, not a running server.

- `py_writes.py`: writes a data folder with the Python code (`AO_DATA_DIR` must point at an empty folder). Start `ao-backend` on it and
  check that login, providers (key masked), settings, role models, profiles, MCP servers, schedules, features and tasks all read back.
- `py_reads_rust.py`: reads a data folder written by `ao-backend` with the Python code (password, token, provider key decrypts, settings,
  role model, profiles, MCP, schedules).
- `py_routes.py`: prints the routes that exist in only one of the two backends (expected: none, 93 each).

Last run: Rust to Python and Python to Rust both read back every table checked; route sets identical (93).
