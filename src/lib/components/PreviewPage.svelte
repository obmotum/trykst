<script lang="ts">
	// One page of the preview. A page is tens of thousands of SVG nodes, so only
	// pages in or near the visible area are put into the DOM; the others keep
	// their place as an empty sheet of the same size.
	let { svg, root }: { svg: string; root: HTMLElement | undefined } = $props();

	let sheet: HTMLDivElement;
	let near = $state(false);

	// Page size in pt, from the head of the SVG.
	let size = $derived.by(() => {
		const match = /viewBox="0 0 ([\d.]+) ([\d.]+)"/.exec(svg.slice(0, 500));
		return match ? { width: Number(match[1]), height: Number(match[2]) } : { width: 595.28, height: 841.89 };
	});

	$effect(() => {
		const observer = new IntersectionObserver(([entry]) => (near = entry.isIntersecting), {
			root,
			rootMargin: '1500px 0px'
		});
		observer.observe(sheet);
		return () => observer.disconnect();
	});
</script>

<div
	bind:this={sheet}
	class="preview-container shadow-xl bg-white max-w-full lg:max-w-[95%] flex-shrink-0"
	style="width: {size.width}pt; aspect-ratio: {size.width} / {size.height};"
>
	<!-- Its own layer, sealed off from the rest of the page: typing in the editor
	     must not make the browser lay out and paint the pages again. -->
	<div class="h-full w-full [contain:strict] [will-change:transform]">
		{#if near}{@html svg}{/if}
	</div>
</div>

<style>
	:global(.preview-container svg) {
		max-width: 100%;
		height: auto;
		display: block;
	}
</style>
