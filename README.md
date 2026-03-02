# T3 Chat Desktop (Tauri)

Desktop wrapper for `https://t3.chat`.

## Behavior

- The main window loads `https://t3.chat`.
- Off-domain navigations are opened in your system browser, except auth.
- The embedded webview stays focused on `t3.chat` pages.

## Scripts

- `bun run dev` starts the Vite frontend dev server.
- `bun run tauri dev` runs the desktop app in development.
- `bun run build` builds the frontend.
- `bun run tauri build` builds the desktop app.
