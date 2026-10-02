<script lang="ts" module>
	import { createTypstRenderer } from '@myriaddreamin/typst.ts';
	import type { TypstRenderer } from '@myriaddreamin/typst.ts';
	import rendererWasm from '@myriaddreamin/typst-ts-renderer/pkg/typst_ts_renderer_bg.wasm?url';

	// One renderer for the whole application; each preview gets its own session.
	let rendererPromise: Promise<TypstRenderer> | undefined;
	function getRenderer(): Promise<TypstRenderer> {
		return (rendererPromise ??= (async () => {
			const renderer = createTypstRenderer();
			await renderer.init({ getModule: () => rendererWasm });
			return renderer;
		})());
	}
</script>

<script lang="ts">
	// The preview of a document compiled in the browser. It receives the
	// compiler's output (a full artifact, then deltas) and keeps one SVG up to
	// date by patching it; only the pages in or near the visible area are filled.
	import { onMount, tick } from 'svelte';
	import { patchRoot } from '@myriaddreamin/typst.ts/render/svg/patch';
	import { documentZoomStore } from '../ts/store';

	let {
		onReady,
		onLost
	}: {
		/** The preview can take data now; it starts empty. */
		onReady: () => void;
		/** The preview lost track of the document and needs it in full again. */
		onLost: () => void;
	} = $props();

	/** Space between pages, in document units (pt). */
	const GAP = 24;
	/** How far beyond the visible area pages are rendered, in document units. */
	const MARGIN = 1200;

	interface Sheet {
		x: number;
		/** Top in the document as the renderer stacks it: no gaps, page heights rounded up. */
		docY: number;
		/** Top as displayed (with gaps). */
		y: number;
		width: number;
		height: number;
	}

	let scroller: HTMLDivElement;
	let paper: HTMLDivElement;
	let host: HTMLDivElement;
	let session: any;
	let sheets = $state.raw<Sheet[]>([]);
	let docWidth = $state(595.28);
	let displayHeight = $state(841.89);
	let frame = 0;
	/** Page index by the renderer's own vertical offset of the page. */
	let rawOffsets = new Map<number, number>();
	/** The renderer's stylesheet (hides the text-selection layer and link boxes); patches do not carry it. */
	let css = $state('');

	/** Applies compiler output: `reset` replaces the document, `merge` updates it. */
	export async function apply(action: 'reset' | 'merge', data: Uint8Array) {
		if (!session) return;
		if (action === 'reset') {
			// A new document: forget what was rendered, on both sides.
			session.reset();
			host.innerHTML = '';
			if (!css) css = await stylesheet(data);
		}
		session.manipulateData({ action: 'merge', data });
		layout();
		await tick(); // the paper takes its new size before the visible part is measured
		render();
	}

	function layout() {
		const infos: { width: number; height: number }[] = session.retrievePagesInfo();
		let docY = 0;
		sheets = infos.map((info, i) => {
			const sheet = { x: 0, docY, y: docY + i * GAP, width: info.width, height: info.height };
			docY += Math.ceil(info.height);
			return sheet;
		});
		rawOffsets = new Map(sheets.map((sheet, i) => [sheet.docY, i]));
		docWidth = session.docWidth || 595.28;
		displayHeight = Math.max(docY + Math.max(infos.length - 1, 0) * GAP, 1);
	}

	/** A displayed position (with gaps) as a position in the renderer's document. */
	function toDocY(y: number): number {
		for (const sheet of sheets) {
			if (y < sheet.y) return sheet.docY;
			if (y <= sheet.y + sheet.height) return sheet.docY + (y - sheet.y);
		}
		const last = sheets.at(-1);
		return last ? last.docY + last.height : 0;
	}

	function render() {
		if (!session || !sheets.length) return;
		const paperRect = paper.getBoundingClientRect();
		const scale = paperRect.width / docWidth;
		if (!scale) return;
		const top = (scroller.getBoundingClientRect().top - paperRect.top) / scale;
		const bottom = top + scroller.clientHeight / scale;

		const patch: string = session.renderSvgDiff({
			window: { lo: { x: 0, y: toDocY(top - MARGIN) }, hi: { x: docWidth, y: toDocY(bottom + MARGIN) } }
		});
		const previous = host.firstElementChild as SVGElement | null;
		try {
			if (previous) {
				const holder = document.createElement('div');
				holder.innerHTML = patch;
				fillPlaceholders(previous);
				patchRoot(previous, holder.firstElementChild as SVGElement);
			} else {
				host.innerHTML = patch;
			}
			arrange(host.firstElementChild as SVGElement);
		} catch (e) {
			// The patch does not fit what is on screen: start over with the whole document.
			console.warn('Preview out of sync, reloading it', e);
			host.innerHTML = '';
			onLost();
		}
	}

	/**
	 * Pages outside the rendered area are empty placeholder groups. When one
	 * comes into view, the patch fills it by reusing a child it expects to find
	 * there, so every placeholder gets that (empty) child before patching.
	 */
	function fillPlaceholders(root: SVGElement) {
		for (const group of root.querySelectorAll(':scope > g.typst-page[data-dummy]')) {
			const tid = group.getAttribute('data-tid');
			if (group.children.length || !tid) continue;
			const child = document.createElementNS('http://www.w3.org/2000/svg', 'g');
			child.setAttribute('data-tid', 'g' + tid.slice(1));
			group.appendChild(child);
		}
	}

	/**
	 * Fits the SVG to the paper and moves the pages apart; the renderer stacks
	 * them without gaps. Patching reorders the page groups and resets their
	 * position, so each group is recognized by the offset the renderer gave it.
	 */
	function arrange(root: SVGElement) {
		root.setAttribute('viewBox', `0 0 ${docWidth} ${displayHeight}`);
		root.removeAttribute('width');
		root.removeAttribute('height');
		root.style.cssText = 'display: block; width: 100%; height: auto; overflow: visible;';
		for (const group of root.querySelectorAll(':scope > g.typst-page')) {
			const transform = group.getAttribute('transform') ?? '';
			if (transform === group.getAttribute('data-arranged')) continue; // still where we put it
			const match = /translate\(\s*(-?[\d.]+)[\s,]+(-?[\d.]+)/.exec(transform);
			const index = match ? rawOffsets.get(Math.round(Number(match[2]))) : undefined;
			if (!match || index === undefined) continue;
			const moved = `translate(${match[1]}, ${sheets[index].y})`;
			group.setAttribute('transform', moved);
			group.setAttribute('data-arranged', moved);
		}
	}

	/**
	 * The renderer's stylesheet, taken from a throwaway session: rendering it
	 * in the live session would count as "everything is on screen already".
	 */
	async function stylesheet(data: Uint8Array): Promise<string> {
		const renderer = await getRenderer();
		let styled = '';
		await renderer.runWithSession(async (scratch: any) => {
			scratch.manipulateData({ action: 'reset', data });
			styled = await scratch.renderSvg({ data_selection: { body: false, defs: false, css: true, js: false } });
		});
		return /<style[^>]*>([\s\S]*?)<\/style>/.exec(styled)?.[1] || ' ';
	}

	function schedule() {
		cancelAnimationFrame(frame);
		frame = requestAnimationFrame(render);
	}

	onMount(() => {
		let release: (() => void) | undefined;
		let gone = false;
		getRenderer().then((renderer) => {
			if (gone) return;
			void renderer.runWithSession((created: unknown) => {
				session = created;
				onReady();
				return new Promise<void>((done) => (release = done));
			});
		});
		// Zooming and resizing change what is visible.
		const observer = new ResizeObserver(schedule);
		observer.observe(paper);
		observer.observe(scroller);
		return () => {
			gone = true;
			cancelAnimationFrame(frame);
			observer.disconnect();
			session = undefined;
			release?.();
		};
	});
</script>

<div bind:this={scroller} onscroll={schedule} class="relative h-full w-full overflow-auto py-8 px-4">
	{#if sheets.length === 0}
		<div class="text-gray-400 flex items-center justify-center h-full">
			<p>Document is empty or compiling...</p>
		</div>
	{/if}
	<div
		bind:this={paper}
		class="relative mx-auto"
		style="width: calc(min({docWidth}pt, 100%) * {$documentZoomStore / 100}); aspect-ratio: {docWidth} / {displayHeight}; {sheets.length ? '' : 'visibility: hidden;'}"
	>
		{#each sheets as sheet, i (i)}
			<div
				class="absolute bg-white shadow-xl"
				style="left: {(sheet.x / docWidth) * 100}%; top: {(sheet.y / displayHeight) * 100}%; width: {(sheet.width / docWidth) * 100}%; height: {(sheet.height / displayHeight) * 100}%;"
			></div>
		{/each}
		{@html `<style>${css}</style>`}
		<div bind:this={host} class="preview-container absolute inset-0"></div>
	</div>
</div>
