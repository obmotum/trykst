<script lang="ts">
	import { onMount } from 'svelte';
	import Icon from '@iconify/svelte';
	import Avatar from '$lib/components/Avatar.svelte';
	import { api, formatDateTime } from '../ts/api';

	// A version is a snapshot of the whole document: every folder and file.
	let {
		docId,
		canSave = false,
		canRestore = false,
		onRestored,
		onClose
	}: {
		docId: string;
		canSave?: boolean;
		canRestore?: boolean;
		onRestored: () => void;
		onClose: () => void;
	} = $props();

	type Version = {
		id: string;
		user_id: string | null;
		label: string | null;
		created_at: string;
		author_name?: string | null;
		author_avatar_url?: string | null;
	};
	type VersionFile = { path: string; kind: string; content: string | null };
	type VersionDetail = { entrypoint: string | null; folders: string[]; files: VersionFile[] };

	let versions = $state<Version[]>([]);
	let loading = $state(true);
	let error = $state('');
	let label = $state('');
	let saving = $state(false);
	let restoringId = $state<string | null>(null);

	let preview = $state<{ version: Version; detail: VersionDetail } | null>(null);
	let previewPath = $state('');
	let previewFile = $derived(preview?.detail.files.find((f) => f.path === previewPath));

	async function fetchVersions() {
		try {
			versions = await api<Version[]>('GET', `/api/documents/${docId}/versions`);
			error = '';
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load versions';
		}
		loading = false;
	}

	async function saveVersion(e: Event) {
		e.preventDefault();
		saving = true;
		try {
			await api('POST', `/api/documents/${docId}/versions`, { label: label.trim() || null });
			label = '';
			await fetchVersions();
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed to save the version';
		}
		saving = false;
	}

	async function openPreview(version: Version) {
		try {
			const detail = await api<VersionDetail>('GET', `/api/documents/${docId}/versions/${version.id}`);
			detail.files.sort((a, b) => a.path.localeCompare(b.path));
			previewPath = detail.entrypoint ?? detail.files.find((f) => f.kind === 'text')?.path ?? '';
			preview = { version, detail };
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load the version';
		}
	}

	async function restoreVersion(version: Version) {
		if (!confirm('Restore this version? All files of the document are replaced by the files of this version. Save the current state as a version first if you want to keep it.')) return;
		restoringId = version.id;
		try {
			await api('POST', `/api/documents/${docId}/versions/${version.id}/restore`);
			preview = null;
			onRestored();
			onClose();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to restore the version';
		}
		restoringId = null;
	}

	onMount(fetchVersions);
</script>

<div class="fixed right-0 top-0 bottom-0 w-80 bg-[var(--theme-bg)] text-[var(--theme-text)] border-l shadow-2xl flex flex-col z-[70] border-[var(--theme-border)]">
	<div class="flex items-center justify-between px-4 py-3 border-b border-[var(--theme-border)]">
		<div class="flex items-center gap-2">
			<Icon icon="mdi:history" class="text-lg" />
			<h2 class="text-sm font-semibold">Version history</h2>
			<span class="text-[10px] font-bold px-2 py-0.5 rounded-full">{versions.length}</span>
		</div>
		<button onclick={onClose} class="p-1.5 hover:bg-gray-200 dark:hover:bg-white/10 rounded-md transition-colors" title="Close version history" aria-label="Close version history">
			<Icon icon="mdi:close" class="text-lg" />
		</button>
	</div>

	{#if canSave}
		<form onsubmit={saveVersion} class="p-3 border-b border-[var(--theme-border)] flex gap-2">
			<input bind:value={label} maxlength="200" placeholder="Name this version (optional)" aria-label="Version name" class="flex-1 min-w-0 text-sm px-3 py-1.5 rounded-lg border border-[var(--theme-border)] bg-transparent focus:outline-none focus:ring-2 focus:ring-blue-500/50" />
			<button type="submit" disabled={saving} class="flex-shrink-0 px-3 py-1.5 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-lg disabled:opacity-60">{saving ? 'Saving...' : 'Save'}</button>
		</form>
	{/if}

	<div class="flex-1 overflow-y-auto p-4 space-y-3">
		{#if error}
			<div class="text-red-500 text-sm p-3 bg-red-50 dark:bg-red-900/20 rounded-lg border border-red-200 dark:border-red-900/30" role="alert">{error}</div>
		{/if}
		{#if loading}
			<div class="flex justify-center py-8"><Icon icon="mdi:loading" class="animate-spin text-2xl" /></div>
		{:else if versions.length === 0}
			<div class="flex flex-col items-center justify-center py-12 space-y-2">
				<Icon icon="mdi:history" class="text-4xl opacity-50" />
				<p class="text-sm">No versions saved yet</p>
			</div>
		{:else}
			{#each versions as version (version.id)}
				<div class="flex flex-col gap-2 p-3 border rounded-xl shadow-sm border-[var(--theme-border)]">
					<div class="flex items-center gap-2">
						<Avatar name={version.author_name ?? undefined} url={version.author_avatar_url} seed={version.user_id ?? version.id} size={24} />
						<div class="min-w-0">
							<p class="text-xs font-semibold truncate">{version.label || 'Unnamed version'}</p>
							<p class="text-[10px] opacity-70 truncate">{version.author_name || 'Unknown'} · {formatDateTime(version.created_at)}</p>
						</div>
					</div>
					<div class="flex gap-2">
						<button onclick={() => openPreview(version)} class="flex-1 px-3 py-1.5 hover:bg-gray-200 dark:hover:bg-white/20 text-xs font-medium rounded-lg transition-colors flex items-center justify-center gap-1.5">
							<Icon icon="mdi:eye" class="text-sm" /> Files
						</button>
						{#if canRestore}
							<button onclick={() => restoreVersion(version)} disabled={restoringId === version.id} class="flex-1 px-3 py-1.5 bg-purple-50 text-purple-700 hover:bg-purple-100 dark:bg-purple-900/20 dark:text-purple-400 dark:hover:bg-purple-900/40 text-xs font-medium rounded-lg transition-colors flex items-center justify-center gap-1.5 disabled:opacity-60">
								<Icon icon={restoringId === version.id ? 'mdi:loading' : 'mdi:restore'} class="text-sm {restoringId === version.id ? 'animate-spin' : ''}" /> Restore
							</button>
						{/if}
					</div>
				</div>
			{/each}
		{/if}
	</div>
</div>

{#if preview}
	<div class="fixed inset-0 bg-black/60 backdrop-blur-sm z-[100] flex items-center justify-center p-4" role="presentation" onclick={() => (preview = null)}>
		<div class="bg-[var(--theme-bg)] text-[var(--theme-text)] rounded-2xl shadow-2xl border border-[var(--theme-border)] w-full max-w-5xl h-[80vh] flex flex-col" role="dialog" tabindex="-1" aria-modal="true" aria-label="Files of this version" onclick={(e) => e.stopPropagation()} onkeydown={(e) => { if (e.key === 'Escape') preview = null; }}>
			<div class="flex items-center justify-between p-4 border-b border-[var(--theme-border)]">
				<div class="flex items-center gap-3 min-w-0">
					<Icon icon="mdi:history" class="text-blue-500 text-xl flex-shrink-0" />
					<h3 class="text-lg font-semibold truncate">{preview.version.label || 'Unnamed version'}</h3>
					<span class="text-sm opacity-70 flex-shrink-0">{formatDateTime(preview.version.created_at)}</span>
				</div>
				<button onclick={() => (preview = null)} class="p-1.5 hover:bg-gray-200 dark:hover:bg-white/10 rounded-md transition-colors" title="Close" aria-label="Close">
					<Icon icon="mdi:close" class="text-xl" />
				</button>
			</div>

			<div class="flex-1 flex min-h-0">
				<div class="w-60 flex-shrink-0 border-r border-[var(--theme-border)] overflow-y-auto py-1">
					{#each preview.detail.files as file (file.path)}
						<button onclick={() => (previewPath = file.path)} class="w-full text-left px-3 py-1.5 text-sm font-mono truncate {previewPath === file.path ? 'bg-blue-50 dark:bg-blue-900/20 text-blue-700 dark:text-blue-300' : 'hover:bg-gray-100 dark:hover:bg-white/5'}" title={file.path}>
							{file.path}{#if file.path === preview.detail.entrypoint}<Icon icon="mdi:star" class="inline text-amber-500 text-xs ml-1" />{/if}
						</button>
					{/each}
				</div>
				<div class="flex-1 overflow-auto p-5 min-w-0">
					{#if previewFile?.kind === 'text'}
						<pre class="text-sm font-mono whitespace-pre-wrap break-words">{previewFile.content}</pre>
					{:else if previewFile}
						<p class="text-sm opacity-70">{previewFile.path} is not a text file. It is restored together with the version.</p>
					{:else}
						<p class="text-sm opacity-70">Select a file.</p>
					{/if}
				</div>
			</div>

			<div class="p-4 border-t border-[var(--theme-border)] flex justify-end gap-3">
				<button onclick={() => (preview = null)} class="px-4 py-2 text-sm font-medium hover:bg-gray-100 dark:hover:bg-white/10 rounded-lg transition-colors">Close</button>
				{#if canRestore}
					<button onclick={() => restoreVersion(preview!.version)} class="bg-purple-600 hover:bg-purple-700 text-white px-5 py-2 rounded-lg text-sm font-medium transition-colors shadow-sm flex items-center gap-2">
						<Icon icon="mdi:restore" class="text-lg" /> Restore this version
					</button>
				{/if}
			</div>
		</div>
	</div>
{/if}
