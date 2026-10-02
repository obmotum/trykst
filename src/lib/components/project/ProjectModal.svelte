<script lang="ts">
	import { untrack } from 'svelte';
	import Icon from '@iconify/svelte';

	// Name and description of a project: used to create one and to edit one.
	let {
		title,
		submitLabel,
		name = '',
		description = '',
		onSubmit,
		onClose
	}: {
		title: string;
		submitLabel: string;
		name?: string;
		description?: string;
		onSubmit: (name: string, description: string) => Promise<void>;
		onClose: () => void;
	} = $props();

	let nameValue = $state(untrack(() => name));
	let descriptionValue = $state(untrack(() => description));
	let saving = $state(false);
	let error = $state('');

	async function submit(e: Event) {
		e.preventDefault();
		saving = true;
		error = '';
		try {
			await onSubmit(nameValue.trim(), descriptionValue.trim());
		} catch (err) {
			error = err instanceof Error ? err.message : 'Something went wrong';
		}
		saving = false;
	}

	function focus(el: HTMLInputElement) {
		el.focus();
		el.select();
	}
</script>

<div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 backdrop-blur-sm p-4" role="presentation" onclick={onClose}>
	<div tabindex="-1" class="bg-[var(--theme-bg)] text-[var(--theme-text)] rounded-xl shadow-2xl border border-gray-200 dark:border-white/10 w-full max-w-md" role="dialog" aria-modal="true" aria-labelledby="project-modal-title" onclick={(e) => e.stopPropagation()} onkeydown={(e) => { if (e.key === 'Escape') onClose(); }}>
		<div class="flex justify-between items-center p-5 border-b border-gray-200 dark:border-white/10">
			<h2 id="project-modal-title" class="text-lg font-semibold flex items-center gap-2">
				<Icon icon="mdi:folder-account" class="text-blue-500 text-xl" /> {title}
			</h2>
			<button onclick={onClose} aria-label="Close" class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 rounded-full p-1 transition-colors">
				<Icon icon="mdi:close" class="text-xl" />
			</button>
		</div>

		<form onsubmit={submit} class="p-5 space-y-4">
			<div class="space-y-1.5">
				<label for="project-name" class="text-sm font-medium block">Name</label>
				<input id="project-name" use:focus type="text" required maxlength="200" bind:value={nameValue} placeholder="e.g. Annual report 2026" class="w-full bg-black/5 dark:bg-white/5 border border-gray-300 dark:border-white/20 text-sm rounded-lg px-4 py-2.5 focus:outline-none focus:ring-2 focus:ring-blue-500" />
			</div>
			<div class="space-y-1.5">
				<label for="project-description" class="text-sm font-medium block">Description <span class="font-normal text-gray-500 dark:text-gray-400">(optional)</span></label>
				<textarea id="project-description" rows="2" bind:value={descriptionValue} class="w-full bg-black/5 dark:bg-white/5 border border-gray-300 dark:border-white/20 text-sm rounded-lg px-4 py-2.5 focus:outline-none focus:ring-2 focus:ring-blue-500 resize-none"></textarea>
			</div>
			{#if error}
				<p class="text-sm text-red-600 dark:text-red-400" role="alert">{error}</p>
			{/if}
			<div class="pt-2 flex justify-end gap-3">
				<button type="button" onclick={onClose} class="px-4 py-2 text-sm font-medium hover:bg-gray-100 dark:hover:bg-white/10 rounded-lg transition-colors">Cancel</button>
				<button type="submit" disabled={saving} class="bg-blue-600 hover:bg-blue-700 text-white px-5 py-2 rounded-lg text-sm font-medium transition-colors shadow-sm disabled:opacity-60">{submitLabel}</button>
			</div>
		</form>
	</div>
</div>
