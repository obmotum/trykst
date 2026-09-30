<script lang="ts">
	import '../app.css';
	import { fetchUser, redirectToLogin, userStore } from '$lib/ts/auth';
	import { onMount } from 'svelte';
	import { get } from 'svelte/store';
	import { page } from '$app/stores';
	import { themeStore, darkModeStore } from '$lib/ts/store';
	import { themes } from '$lib/ts/themes';

	let { children } = $props();
	let loaded = $state(false);

	// Pages that anonymous visitors may open: documents and spaces can be public,
	// and those pages send the visitor to sign-in themselves when access is denied.
	const PUBLIC_PREFIXES = ['/doc/', '/space/', '/api-docs'];

	onMount(async () => {
		await fetchUser();
		const path = $page.url.pathname;
		if (!get(userStore) && !PUBLIC_PREFIXES.some((p) => path.startsWith(p))) {
			redirectToLogin();
			return;
		}
		loaded = true;
	});

	let currentTheme = $derived(themes[$themeStore as keyof typeof themes] || themes['Catppuccin']);
	let currentColors = $derived($darkModeStore ? currentTheme.dark : currentTheme.light);
</script>

<svelte:head>
	<title>Trykst</title>
	<meta name="description" content="Trykst - A collaborative Typst editor and document manager." />
	<meta name="theme-color" content={currentColors.background} />
	<meta property="og:title" content="Trykst" />
	<meta property="og:description" content="A collaborative Typst editor and document manager." />
	<meta property="og:type" content="website" />
</svelte:head>

{#if loaded}
	<div 
		class="min-h-screen w-full flex flex-col font-sans transition-colors duration-200"
		style="
			background-color: {currentColors.background}; 
			color: {currentColors.text}; 
			--theme-bg: {currentColors.background};
			--theme-text: {currentColors.text};
			--theme-border: {currentColors.selection};
			--theme-cursor: {currentColors.cursor};
		"
	>
		{@render children()}
	</div>
{:else}
	<div class="min-h-screen w-full flex items-center justify-center bg-gray-50 dark:bg-zinc-950">
		<div class="text-gray-500 dark:text-gray-400 font-medium animate-pulse">Loading Trykst...</div>
	</div>
{/if}
