# tinymist in the browser: feasibility test

Runs tinymist's language server, built for the web (`tinymist-web.tar.gz`
from the tinymist release, v0.15.8), in a Web Worker and drives it like the
Trykst editor would: LSP over postMessage, files handed over through
`tinymist/fsChange`, packages through `resolveFn`.

Put `tinymist.js` and `tinymist_bg.wasm` from the release into `vendor/`
(not committed, 32 MB), then:

    npx vite spike/tinymist --config spike/tinymist/vite.config.ts   # http://localhost:5199

Results are in `window.__tinymist`.

Findings (headless Edge):
- Starts in 0.5-1.4 s; completion 140-530 ms (first request), hover 10-55 ms.
- Hover and completion work across files and for a package served by us
  (doc comments included).
- Hand over all files of the document right after `initialized`; files only
  sent in answer to `tinymist/fs/watch` arrive after the first analysis and
  show up as missing.
- The web build has no fonts: every font family is reported as unknown
  (the editor already filters those diagnostics).
- No path completion inside `#include "…"`.
- 32.5 MB of WebAssembly, 12.3 MB with gzip.
