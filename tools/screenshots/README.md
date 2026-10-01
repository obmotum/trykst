# README screenshots

Takes `preview/dashboard.png` and `preview/editor.png` for the main README:
signs in as `alice` through the dev Keycloak, creates sample documents and
captures the dashboard and the editor (Catppuccin Dark, 1900×950).

## Requirements

- Node.js 20+
- Microsoft Edge, Google Chrome or Chromium (found automatically; override with `BROWSER=/path/to/browser`)
- The dev Keycloak: `docker compose -f docker-compose.dev.yml up -d`
- A Trykst server on `http://localhost:3000` configured as in the header of
  `docker-compose.dev.yml`, with an **empty database** and a built frontend
  (`npm run build` in the repository root)

## Usage

```bash
cd tools/screenshots
npm install
npm run shoot
```

The images are written to `preview/`. Pass another directory as argument to
keep the current ones: `node shoot.js /tmp/shots`. Point `TRYKST_URL` at a
different instance if needed.

The sample paper is `paper.typ`; the meeting notes and the proposal are
inline in `shoot.js`.
