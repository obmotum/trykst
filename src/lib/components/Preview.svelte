<script lang="ts">
	import { documentZoomStore } from '../ts/store';
	import type { PreviewPage as Page } from '../ts/typst-api';
	import PreviewPage from './PreviewPage.svelte';

	let { pages = [] }: { pages?: Required<Page>[] } = $props();

	let scroller = $state<HTMLElement>();
</script>

<div bind:this={scroller} class="relative h-full w-full overflow-auto bg-transparent py-8 px-4 flex flex-col items-center gap-8">
	<div class="flex flex-col items-center gap-8 transition-transform duration-200 max-w-full" style="transform: scale({$documentZoomStore / 100}); transform-origin: top center;">
		{#if pages.length > 0}
			<!-- Keyed by position: a page whose content did not change keeps its DOM. -->
			{#each pages as page, i (i)}
				<PreviewPage svg={page.svg} root={scroller} />
			{/each}
		{:else}
			<div class="text-gray-400 flex flex-col items-center justify-center h-full">
				<p>Document is empty or compiling...</p>
			</div>
		{/if}
	</div>
</div>
