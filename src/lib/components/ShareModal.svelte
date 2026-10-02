<script lang="ts">
	import Icon from '@iconify/svelte';
	import { api, canWrite } from '$lib/ts/api';
	import type { Doc } from '$lib/ts/api';

	// Link sharing of one document. Who is in the project is managed on the
	// project page; members reach every document through their project role.
	let { doc, onDocChanged, onClose }: { doc: Doc; onDocChanged: (doc: Doc) => void; onClose: () => void } = $props();

	let canManage = $derived(doc.is_member && canWrite(doc.role));
	let link = $derived(`${window.location.origin}/doc/${doc.id}`);
	let copied = $state(false);
	let saving = $state(false);
	let error = $state('');

	async function setPublicRole(select: HTMLSelectElement) {
		saving = true;
		error = '';
		try {
			onDocChanged(await api<Doc>('PATCH', `/api/documents/${doc.id}`, { public_role: select.value === 'off' ? null : select.value }));
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to change link sharing';
			select.value = doc.public_role ?? 'off';
		}
		saving = false;
	}

	function copyLink() {
		if (navigator.clipboard && window.isSecureContext) {
			navigator.clipboard.writeText(link).catch(console.error);
		} else {
			const input = document.getElementById('share-link-input') as HTMLInputElement | null;
			input?.select();
			try {
				document.execCommand('copy');
			} catch (err) {
				console.error('Fallback copy failed', err);
			}
		}
		copied = true;
		setTimeout(() => (copied = false), 2000);
	}
</script>

<div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 backdrop-blur-sm p-4" role="presentation" onclick={onClose}>
	<div tabindex="-1" class="rounded-xl shadow-2xl border w-full max-w-[500px] bg-[var(--theme-bg)] text-[var(--theme-text)] border-[var(--theme-border)]" role="dialog" aria-modal="true" aria-labelledby="share-dialog-title" onclick={(e) => e.stopPropagation()} onkeydown={(e) => { if (e.key === 'Escape') onClose(); }}>
		<div class="flex justify-between items-center p-4 border-b border-[var(--theme-border)]">
			<h2 id="share-dialog-title" class="text-lg font-semibold">Share “{doc.title}”</h2>
			<button onclick={onClose} aria-label="Close" class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 rounded-full p-1 transition-colors">
				<Icon icon="mdi:close" class="text-xl" />
			</button>
		</div>

		<div class="p-6 space-y-5">
			<div class="flex items-start gap-4 p-3 rounded-xl border border-gray-200 dark:border-zinc-800/50 bg-gray-50/50 dark:bg-zinc-950/30">
				<div class="bg-gray-200 dark:bg-zinc-800 p-2.5 rounded-full text-gray-600 dark:text-gray-300">
					<Icon icon="mdi:account-group-outline" class="text-xl" />
				</div>
				<div class="flex-1 min-w-0">
					<h4 class="text-sm font-medium text-gray-900 dark:text-white">Project members</h4>
					<p class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">Everyone in the project can open this document with their project role.</p>
				</div>
				{#if doc.is_member}
					<a href="/project/{doc.project_id}?members=1" class="flex-shrink-0 text-sm font-medium text-blue-600 dark:text-blue-400 hover:underline py-1.5">Manage</a>
				{/if}
			</div>

			<div class="flex items-start gap-4 p-3 rounded-xl border border-gray-200 dark:border-zinc-800/50 bg-gray-50/50 dark:bg-zinc-950/30">
				<div class="p-2.5 rounded-full {doc.public_role ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/20 dark:text-emerald-300' : 'bg-gray-200 dark:bg-zinc-800 text-gray-600 dark:text-gray-300'}">
					<Icon icon={doc.public_role ? 'mdi:earth' : 'mdi:lock-outline'} class="text-xl" />
				</div>
				<div class="flex-1 min-w-0">
					<h4 class="text-sm font-medium text-gray-900 dark:text-white">Anyone with the link</h4>
					<p class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
						{#if doc.public_role === 'editor'}Can open and edit this document, without signing in.
						{:else if doc.public_role === 'viewer'}Can read this document, without signing in.
						{:else}Only project members can open this document.{/if}
					</p>
				</div>
				{#if canManage}
					<select
						value={doc.public_role ?? 'off'}
						disabled={saving}
						onchange={(e) => setPublicRole(e.currentTarget)}
						aria-label="Link sharing"
						class="flex-shrink-0 bg-gray-100 dark:bg-zinc-800 border border-gray-200 dark:border-zinc-700 text-sm font-medium text-gray-700 dark:text-gray-300 rounded-md px-3 py-1.5 focus:outline-none cursor-pointer focus:ring-2 focus:ring-blue-500/20 disabled:opacity-60"
					>
						<option value="off">Off</option>
						<option value="viewer">Viewer</option>
						<option value="editor">Editor</option>
					</select>
				{:else}
					<span class="flex-shrink-0 text-sm font-medium capitalize py-1.5">{doc.public_role ?? 'Off'}</span>
				{/if}
			</div>

			{#if error}
				<p class="text-sm text-red-600 dark:text-red-400" role="alert">{error}</p>
			{/if}

			<input id="share-link-input" readonly value={link} onfocus={(e) => e.currentTarget.select()} aria-label="Link to this document" class="w-full text-sm font-mono px-3 py-2 rounded-lg border border-gray-300 dark:border-zinc-700 bg-transparent" />
		</div>

		<div class="p-4 rounded-b-xl bg-gray-50 dark:bg-zinc-950/50 border-t border-[var(--theme-border)] flex items-center justify-between">
			<button onclick={copyLink} class="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium text-blue-600 hover:bg-blue-50 dark:text-blue-400 dark:hover:bg-blue-500/10 transition-colors">
				<Icon icon={copied ? 'mdi:check' : 'mdi:link-variant'} class="text-lg" />
				<span>{copied ? 'Link copied!' : 'Copy link'}</span>
			</button>
			<button onclick={onClose} class="px-6 py-2 text-sm font-semibold text-white bg-gray-800 hover:bg-gray-900 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-white rounded-lg shadow-sm transition-colors">Done</button>
		</div>
	</div>
</div>
