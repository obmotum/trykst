// Feasibility test: compile in a worker, render on the page. Results end up in window.__spike.
import { createTypstRenderer } from '@myriaddreamin/typst.ts';
import rendererWasm from '@myriaddreamin/typst-ts-renderer/pkg/typst_ts_renderer_bg.wasm?url';

const ms = (from: number) => Math.round((performance.now() - from) * 10) / 10;
const started = performance.now();

// While the worker compiles, the page must stay responsive: count blocked stretches.
const longTasks: number[] = [];
new PerformanceObserver((list) => {
	for (const entry of list.getEntries()) longTasks.push(Math.round(entry.duration));
}).observe({ entryTypes: ['longtask'] });

const worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
worker.onmessage = async (event) => {
	const { out, paper, long } = event.data as { out: Record<string, unknown>; paper?: Uint8Array; long?: Uint8Array };
	out.workerTotalMs = ms(started);
	out.mainThreadLongTasksDuringCompile = [...longTasks];
	try {
		if (paper && long) {
			let t = performance.now();
			const renderer = createTypstRenderer();
			await renderer.init({ getModule: () => rendererWasm });
			out.rendererInitMs = ms(t);

			t = performance.now();
			const paperSvg = await renderer.renderSvg({ format: 'vector', artifactContent: paper });
			out.paperRender = { ms: ms(t), svgKb: Math.round(paperSvg.length / 1024) };
			document.getElementById('paper')!.innerHTML = paperSvg;

			// The long document: page sizes, then only the first screenful instead of all pages.
			await renderer.runWithSession({ format: 'vector', artifactContent: long }, async (session: any) => {
				const pages = session.retrievePagesInfo();
				t = performance.now();
				const whole = await session.renderSvg({});
				const wholeMs = ms(t);
				t = performance.now();
				const firstPage = await session.renderSvg({ window: { lo: { x: 0, y: 0 }, hi: { x: pages[0].width, y: pages[0].height } } });
				out.longRender = { pages: pages.length, wholeMs, wholeKb: Math.round(whole.length / 1024), firstPageMs: ms(t), firstPageKb: Math.round(firstPage.length / 1024) };
			});
		}
	} catch (e) {
		out.renderError = String((e as any)?.stack ?? e);
	}
	out.resources = performance
		.getEntriesByType('resource')
		.map((r: any) => ({ name: r.name.split('/').slice(-2).join('/').slice(0, 60), kb: Math.round((r.transferSize || r.encodedBodySize) / 1024), ms: Math.round(r.duration) }))
		.filter((r) => r.kb > 200);
	(window as any).__spike = out;
	document.getElementById('status')!.textContent = 'done';
};
worker.postMessage('go');
