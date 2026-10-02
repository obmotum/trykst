<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import { get } from 'svelte/store';
	import { page } from '$app/stores';
	import Icon from '@iconify/svelte';
	import { userStore, redirectToLogin } from '$lib/ts/auth';
	import Editor from '$lib/components/Editor.svelte';
	import Preview from '$lib/components/Preview.svelte';
	import ErrorBanner from '$lib/components/ErrorBanner.svelte';
	import DocFooter from '$lib/components/DocFooter.svelte';
	import FileTree from '$lib/components/document/FileTree.svelte';
	import DocToolbar from '$lib/components/document/DocToolbar.svelte';
	import PublishPackageModal from '$lib/components/PublishPackageModal.svelte';
	import { api, canWrite } from '$lib/ts/api';
	import type { Doc, Tree, TreeNode } from '$lib/ts/api';
	import { compileDocument } from '$lib/ts/typst-api';
	import type { Diagnostic } from '$lib/ts/typst-api';
	import { editorErrors, documentStatsStore, previewOpenStore, editorViewStore, commentsSidebarOpen, commentReference, versionHistoryOpen } from '$lib/ts/store';
	import { setDocument, openFile, closeFile, closeAllFiles, setActiveFile, openTexts, cleanupDocument } from '$lib/ts/yjs-document';
	import type { OpenFile } from '$lib/ts/yjs-document';

	const docId = $page.params.id as string;
	// How often collaborators' changes to the file tree are picked up.
	const REFRESH_MS = 10_000;

	let doc = $state<Doc | null>(null);
	let nodes = $state<TreeNode[]>([]);
	let activeId = $state('');
	let activeEntry = $state.raw<OpenFile | undefined>(undefined);
	let svgs = $state<string[]>([]);
	let errors = $state<Diagnostic[]>([]);
	let showPublish = $state(false);
	let loadError = $state('');
	let treeError = $state('');
	let compileTimer: number | undefined;
	let errorTimer: number | undefined;
	let observed = new Set<string>();

	let contextMenu = $state({ show: false, x: 0, y: 0, text: '' });

	let readOnly = $derived(!canWrite(doc?.role));
	let activeNode = $derived(nodes.find((n) => n.id === activeId) ?? null);
	let isImage = $derived(!!activeNode && activeNode.kind === 'binary' && (activeNode.mime_type ?? '').startsWith('image/'));

	// Squiggles belong to the file they were reported for.
	$effect(() => {
		const path = activeNode?.path;
		$editorErrors = errors.filter((e) => e.from != null && (!e.path || e.path === path));
	});

	function contentUrl(node: TreeNode) {
		return `/api/documents/${docId}/nodes/${node.id}/content`;
	}

	function showTreeError(e: unknown) {
		treeError = e instanceof Error ? e.message : String(e);
		clearTimeout(errorTimer);
		errorTimer = window.setTimeout(() => (treeError = ''), 6000);
	}

	// --- Compiling -------------------------------------------------------------------

	/** Text of the open files by path: what the editor shows wins over what is stored. */
	function getFiles(): Record<string, string> {
		const files: Record<string, string> = {};
		for (const [nodeId, text] of openTexts()) {
			const node = nodes.find((n) => n.id === nodeId);
			if (node) files[node.path] = text;
		}
		return files;
	}

	function scheduleCompile() {
		clearTimeout(compileTimer);
		compileTimer = window.setTimeout(triggerCompile, 500);
	}

	function triggerCompile() {
		if (!doc || (!$previewOpenStore && !readOnly)) return;
		compileDocument(docId, getFiles())
			.then((res) => {
				if (res.stats) $documentStatsStore = res.stats;
				if (res.svgs) {
					svgs = res.svgs;
					errors = [];
				} else if (res.errors) {
					errors = res.errors;
				}
			})
			.catch(() => {
				errors = [{ message: 'Network or server error while compiling the document.', severity: 'Error' }];
			});
	}

	// Reopening the preview shows the current state right away.
	$effect(() => {
		if ($previewOpenStore) untrack(triggerCompile);
	});

	// --- Files -----------------------------------------------------------------------

	function activate(node: TreeNode | undefined) {
		if (!node || node.kind === 'folder') {
			activeId = '';
			activeEntry = undefined;
			setActiveFile(null);
			return;
		}
		activeId = node.id;
		if (node.kind === 'text' && !readOnly) {
			const entry = openFile(node.id);
			if (!observed.has(node.id)) {
				observed.add(node.id);
				entry.text.observe(scheduleCompile);
			}
			activeEntry = entry;
			setActiveFile(node.id);
		} else {
			activeEntry = undefined;
			setActiveFile(null);
		}
	}

	/** Applies a tree from the server; closes rooms of files that no longer exist. */
	function applyTree(tree: Tree) {
		const ids = new Set(tree.nodes.map((n) => n.id));
		for (const id of observed) {
			if (!ids.has(id)) {
				closeFile(id);
				observed.delete(id);
			}
		}
		nodes = tree.nodes;
		if (doc) doc.entrypoint_id = tree.entrypoint_id;
		if (!ids.has(activeId)) {
			activate(tree.nodes.find((n) => n.id === tree.entrypoint_id) ?? tree.nodes.find((n) => n.kind === 'text'));
		}
	}

	async function loadTree() {
		applyTree(await api<Tree>('GET', `/api/documents/${docId}/tree`));
	}

	async function refresh() {
		if (document.hidden || !doc) return;
		try {
			const [fresh, tree] = await Promise.all([
				api<Doc>('GET', `/api/documents/${docId}`),
				api<Tree>('GET', `/api/documents/${docId}/tree`)
			]);
			const changed = fresh.updated_at !== doc.updated_at || JSON.stringify(tree.nodes) !== JSON.stringify(nodes);
			doc = fresh;
			applyTree(tree);
			if (changed) scheduleCompile();
		} catch {
			// Access may have been withdrawn; the next action reports it.
		}
	}

	async function run(action: () => Promise<unknown>) {
		try {
			await action();
			await loadTree();
			scheduleCompile();
		} catch (e) {
			showTreeError(e);
		}
	}

	function createNode(parentId: string | null, name: string, kind: 'text' | 'folder') {
		run(async () => {
			const node = await api<TreeNode>('POST', `/api/documents/${docId}/nodes`, { parent_id: parentId, name, kind });
			await loadTree();
			if (kind === 'text') activate(nodes.find((n) => n.id === node.id));
		});
	}

	function uploadFiles(parentId: string | null, files: File[]) {
		run(async () => {
			const form = new FormData();
			form.append('parent_id', parentId ?? '');
			for (const file of files) form.append('files', file);
			const res = await fetch(`/api/documents/${docId}/upload`, { method: 'POST', body: form });
			if (!res.ok) throw new Error((await res.text()) || 'Upload failed');
			// An upload may replace a text file that is open: reconnect to its new content.
			for (const id of Array.from(observed)) {
				const node = nodes.find((n) => n.id === id);
				if (node && node.parent_id === parentId && files.some((f) => f.name === node.name)) {
					closeFile(id);
					observed.delete(id);
					if (activeId === id) activeId = '';
				}
			}
		});
	}

	function renameNode(node: TreeNode, name: string) {
		run(() => api('PATCH', `/api/documents/${docId}/nodes/${node.id}`, { name }));
	}

	function moveNode(node: TreeNode, parentId: string | null) {
		run(() => api('PATCH', `/api/documents/${docId}/nodes/${node.id}`, { parent_id: parentId }));
	}

	function deleteNode(node: TreeNode) {
		const inside = nodes.filter((n) => n.path.startsWith(node.path + '/')).length;
		const what = node.kind === 'folder' && inside ? `the folder “${node.path}” and the ${inside} item(s) in it` : `“${node.path}”`;
		if (!confirm(`Delete ${what}?`)) return;
		run(() => api('DELETE', `/api/documents/${docId}/nodes/${node.id}`));
	}

	function setEntry(node: TreeNode) {
		run(() => api('PUT', `/api/documents/${docId}/entrypoint`, { node_id: node.id }));
	}

	/** Every file was replaced (a version was restored): start over with the new tree. */
	async function reloadEverything() {
		closeAllFiles();
		observed.clear();
		activeId = '';
		activeEntry = undefined;
		try {
			doc = await api<Doc>('GET', `/api/documents/${docId}`);
			await loadTree();
			triggerCompile();
		} catch (e) {
			showTreeError(e);
		}
	}

	// --- Editor context menu -----------------------------------------------------------

	function handleContextMenu(e: MouseEvent) {
		const view = $editorViewStore;
		if (!view) return;
		const target = e.target as HTMLElement;
		if (!target.closest('.cm-editor') && !target.closest('.cm-content')) return;
		const selection = view.state.selection.main;
		const selectedText = view.state.doc.sliceString(selection.from, selection.to);
		if (selectedText.trim()) {
			e.preventDefault();
			contextMenu = { show: true, x: e.clientX, y: e.clientY, text: selectedText.trim() };
		}
	}

	function closeContextMenu() {
		contextMenu.show = false;
	}

	function handleAddComment() {
		$commentReference = contextMenu.text;
		$commentsSidebarOpen = true;
		closeContextMenu();
	}

	onMount(() => {
		setDocument(docId);
		$commentsSidebarOpen = false;
		$versionHistoryOpen = false;

		api<Doc>('GET', `/api/documents/${docId}`)
			.then(async (loaded) => {
				doc = loaded;
				await loadTree();
				triggerCompile();
			})
			.catch((e) => {
				// Without access the server answers 404; signing in may be all that is missing.
				if (!get(userStore)) redirectToLogin();
				else loadError = e instanceof Error ? e.message : 'Failed to load the document';
			});

		const interval = window.setInterval(refresh, REFRESH_MS);
		const onVisible = () => { if (!document.hidden) refresh(); };
		document.addEventListener('visibilitychange', onVisible);

		return () => {
			clearInterval(interval);
			clearTimeout(compileTimer);
			clearTimeout(errorTimer);
			document.removeEventListener('visibilitychange', onVisible);
			cleanupDocument();
			$editorErrors = [];
		};
	});
</script>

<svelte:head>
	<title>{doc?.title ?? 'Document'} - Trykst</title>
</svelte:head>

<svelte:window onclick={closeContextMenu} />

{#if doc}
	<div class="flex flex-col h-screen relative">
		<DocToolbar
			{doc}
			{nodes}
			{activeNode}
			activeText={activeEntry?.text ?? null}
			{getFiles}
			onPublish={() => (showPublish = true)}
			onFilesChanged={() => run(async () => {})}
			onDocChanged={(updated) => (doc = updated)}
			onRestored={reloadEverything}
		/>

		<main class="flex-1 flex overflow-hidden relative" oncontextmenu={handleContextMenu}>
			{#if !readOnly}
				<aside class="w-60 flex-shrink-0 hidden md:block">
					<FileTree
						{nodes}
						{activeId}
						entrypointId={doc.entrypoint_id}
						error={treeError}
						{contentUrl}
						onSelect={activate}
						onCreate={createNode}
						onUpload={uploadFiles}
						onRename={renameNode}
						onMove={moveNode}
						onDelete={deleteNode}
						onSetEntry={setEntry}
					/>
				</aside>

				<div class="flex flex-col min-h-0 min-w-0 {$previewOpenStore ? 'w-full md:w-1/2 border-r border-gray-200 dark:border-white/10' : 'flex-1'}">
					{#if activeEntry && activeNode}
						{#key activeId}
							<Editor
								ytext={activeEntry.text}
								awarenessProvider={activeEntry.provider}
								filePath={activeNode.path}
								lspDocId={docId}
								enableLsp={activeNode.name.toLowerCase().endsWith('.typ')}
							/>
						{/key}
					{:else if activeNode && isImage}
						<div class="flex-1 flex flex-col items-center justify-center gap-3 p-6 overflow-auto">
							<img src={contentUrl(activeNode)} alt={activeNode.name} class="max-w-full max-h-[70%] object-contain border border-gray-200 dark:border-white/10 bg-white" />
							<p class="text-xs font-mono text-gray-500 dark:text-gray-400">{activeNode.path}</p>
						</div>
					{:else if activeNode}
						<div class="flex-1 flex flex-col items-center justify-center gap-3 p-6 text-gray-500 dark:text-gray-400">
							<Icon icon="mdi:file-outline" class="text-4xl" />
							<p class="text-sm font-mono">{activeNode.path}</p>
							<a href={contentUrl(activeNode)} download={activeNode.name} class="text-sm font-medium text-blue-600 dark:text-blue-400 hover:underline">Download</a>
						</div>
					{:else}
						<div class="flex-1 flex items-center justify-center text-sm text-gray-500 dark:text-gray-400 p-6 text-center">
							{nodes.length ? 'Select a file to edit it.' : 'This document has no files yet. Create one in the file list.'}
						</div>
					{/if}
				</div>
			{/if}

			{#if $previewOpenStore || readOnly}
				<div class="{readOnly ? 'flex-1' : 'w-full md:w-1/2'} min-w-0 relative bg-white/50 dark:bg-black/20 flex flex-col">
					<Preview {svgs} />
					<ErrorBanner {errors} />
				</div>
			{/if}
		</main>

		<DocFooter />
	</div>
{:else if loadError}
	<div class="h-screen flex flex-col items-center justify-center gap-4 text-center p-6">
		<Icon icon="mdi:file-hidden" class="text-5xl text-gray-400" />
		<h1 class="text-xl font-semibold">This document is not available</h1>
		<p class="text-sm text-gray-500 dark:text-gray-400 max-w-md">It may have been deleted, or you are not a member of its project.</p>
		<a href="/dashboard" class="text-sm font-medium text-blue-600 dark:text-blue-400 hover:underline">Back to your projects</a>
	</div>
{:else}
	<div class="h-screen flex items-center justify-center text-gray-500 dark:text-gray-400 animate-pulse">Loading document...</div>
{/if}

{#if contextMenu.show}
	<div class="fixed z-[9999] bg-[var(--theme-bg)] text-[var(--theme-text)] rounded-lg shadow-xl border border-[var(--theme-border)] py-1 min-w-[200px] overflow-hidden" style="left: {contextMenu.x}px; top: {contextMenu.y}px;">
		{#if $userStore}
			<button onclick={handleAddComment} class="w-full text-left px-4 py-2 text-sm hover:bg-[var(--theme-border)] flex items-center gap-2">
				<Icon icon="mdi:comment-plus-outline" class="text-blue-500" /> Add comment on selection
			</button>
			<div class="h-px bg-[var(--theme-border)] my-1"></div>
		{/if}
		<button onclick={() => { navigator.clipboard.writeText(contextMenu.text); closeContextMenu(); }} class="w-full text-left px-4 py-2 text-sm hover:bg-[var(--theme-border)] flex items-center gap-2">
			<Icon icon="mdi:content-copy" class="text-gray-500" /> Copy text
		</button>
	</div>
{/if}

{#if showPublish && doc}
	<PublishPackageModal {docId} onClose={() => (showPublish = false)} />
{/if}
