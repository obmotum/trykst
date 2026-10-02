# Client-side compile: feasibility test

Checks whether the live preview can be compiled in the browser instead of on
the server, with [typst.ts](https://github.com/Myriad-Dreamin/typst.ts)
(compiler and renderer as WebAssembly).

    npx vite spike --config spike/vite.config.ts   # http://localhost:5199

The page compiles in a Web Worker and renders on the page; results are in
`window.__spike`. It covers the multi-file sample paper from
`tools/screenshots/sample`, a package from Typst Universe, a package served
from memory (as `@project` packages would be), error diagnostics, and a long
document with single-character edits and incremental output.

Temporary: to be replaced by the real integration.
