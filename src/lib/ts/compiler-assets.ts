// The WebAssembly files of the compiler for the live preview. The page and the
// worker load them from these URLs, so loading them early fills the cache the
// editor reads from.
import compilerWasm from '@myriaddreamin/typst-ts-web-compiler/pkg/typst_ts_web_compiler_bg.wasm?url';
import rendererWasm from '@myriaddreamin/typst-ts-renderer/pkg/typst_ts_renderer_bg.wasm?url';

export { compilerWasm, rendererWasm };

let prefetched = false;

/**
 * Loads the compiler, the renderer and the built-in fonts in the background,
 * once the browser is idle, so the first document opens quickly. Everything
 * is served with long-lived cache headers, so the editor gets it from the cache.
 */
export function prefetchCompiler() {
	if (prefetched || typeof window === 'undefined') return;
	prefetched = true;
	const whenIdle = (task: () => void) =>
		typeof window.requestIdleCallback === 'function' ? window.requestIdleCallback(task, { timeout: 5000 }) : setTimeout(task, 2000);

	whenIdle(async () => {
		const get = (url: string) => fetch(url, { priority: 'low' } as RequestInit);
		try {
			// Compiling it here as well lets the browser keep the machine code for later.
			await Promise.all([WebAssembly.compileStreaming(get(compilerWasm)), get(rendererWasm)]);
			const fonts: string[] = await (await get('/api/fonts/default')).json();
			for (const font of fonts) await get(font);
		} catch {
			// Only a head start; the editor loads whatever is missing itself.
		}
	});
}
