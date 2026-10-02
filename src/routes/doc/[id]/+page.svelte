<script lang="ts">
	import { onMount } from 'svelte';
	import { get } from 'svelte/store';
	import { page } from '$app/stores';
	import Icon from '@iconify/svelte';
	import { userStore, redirectToLogin } from '$lib/ts/auth';
	import Editor from '$lib/components/Editor.svelte';
	import ClientPreview from '$lib/components/ClientPreview.svelte';
	import ErrorBanner from '$lib/components/ErrorBanner.svelte';
	import DocFooter from '$lib/components/DocFooter.svelte';
	import FileTree from '$lib/components/document/FileTree.svelte';
	import DocToolbar from '$lib/components/document/DocToolbar.svelte';
	import PublishPackageModal from '$lib/components/PublishPackageModal.svelte';
	import { api, canWrite } from '$lib/ts/api';
	import type { Doc, Tree, TreeNode } from '$lib/ts/api';
	import type { Diagnostic } from '$lib/ts/typst-api';
	import { ClientCompiler, toEditorDiagnostics } from '$lib/ts/client-compiler';
	import type { WorkerFile } from '$lib/ts/client-compiler';
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

	// The preview is compiled here in the browser, by a compiler in a Web Worker.
	// The server only stores the files; it compiles for exports, not for the preview.
	let compiler: ClientCompiler | undefined;
	/** Font files the running compiler was started with; fonts cannot be added later. */
	let compilerFonts = '';
	let preview = $state<ClientPreview | undefined>();
	let previewReady = false;
	/** True while the preview has received everything the compiler produced. */
	let previewInSync = false;
	/** What the compiler holds, by node id: the path and the version it was loaded in. */
	const loaded = new Map<string, { path: string; stamp: string }>();
	/** Text of the text files by absolute path, to turn line:column into editor positions. */
	const texts = new Map<string, string>();
	/** Last text sent from the editor, by node id. */
	const pushed = new Map<string, string>();

	const isFont = (node: TreeNode) => /\.(ttf|otf)$/i.test(node.name);

	async function fetchFile(node: TreeNode): Promise<WorkerFile> {
		const res = await fetch(contentUrl(node));
		if (!res.ok) throw new Error(`Could not load ${node.path}`);
		const path = '/' + node.path;
		return node.kind === 'text' ? { path, text: await res.text() } : { path, bytes: new Uint8Array(await res.arrayBuffer()) };
	}

	/** Brings the compiler's files in line with the tree: loads new and changed files, drops removed ones. */
	async function syncFiles() {
		const files = nodes.filter((n) => n.kind !== 'folder');
		const fontKey = files.filter(isFont).map((n) => `${n.id}:${n.updated_at}`).join(',');
		if (!compiler || fontKey !== compilerFonts) {
			compiler?.dispose();
			loaded.clear();
			texts.clear();
			pushed.clear();
			const fonts = await Promise.all(files.filter(isFont).map(async (n) => (await fetchFile(n)).bytes!));
			compiler = new ClientCompiler(docId, fonts);
			compilerFonts = fontKey;
			previewInSync = false;
		}

		const wanted = new Map(files.map((n) => [n.id, n]));
		const remove: string[] = [];
		for (const [id, entry] of loaded) {
			const node = wanted.get(id);
			if (!node || '/' + node.path !== entry.path) {
				remove.push(entry.path);
				texts.delete(entry.path);
				loaded.delete(id);
				pushed.delete(id);
			}
		}
		// Files open in the editor are kept current from there, not from the server.
		const open = openTexts();
		const stale = files.filter((n) => {
			const have = loaded.get(n.id);
			return !have || (have.stamp !== n.updated_at && !open.has(n.id));
		});
		const set = await Promise.all(stale.map(fetchFile));
		stale.forEach((node, i) => {
			loaded.set(node.id, { path: set[i].path, stamp: node.updated_at });
			if (set[i].text !== undefined) texts.set(set[i].path, set[i].text!);
		});
		compiler.update(set, remove);
	}

	let syncing: Promise<void> = Promise.resolve();
	function queueSync() {
		syncing = syncing.then(syncFiles).catch(showTreeError);
	}

	/** Sends what the editor shows for the open files, if it changed. */
	function pushOpenTexts() {
		const set: WorkerFile[] = [];
		for (const [id, text] of openTexts()) {
			const entry = loaded.get(id);
			if (!entry || pushed.get(id) === text) continue;
			pushed.set(id, text);
			texts.set(entry.path, text);
			set.push({ path: entry.path, text });
		}
		compiler?.update(set);
	}

	function scheduleCompile() {
		clearTimeout(compileTimer);
		compileTimer = window.setTimeout(triggerCompile, 60);
	}

	// Never two compilations at once: changes made while one is running are
	// picked up by a single follow-up run.
	let compiling = false;
	let compileAgain = false;

	async function triggerCompile() {
		if (!doc) return;
		if (compiling) {
			compileAgain = true;
			return;
		}
		compiling = true;
		try {
			await syncing;
			if (!compiler) return;
			await compiler.ready;
			pushOpenTexts();
			const main = nodes.find((n) => n.id === doc!.entrypoint_id);
			if (!main) {
				errors = [{ message: 'The document has no main file; mark a .typ file as main file.', severity: 'Error' }];
				return;
			}
			// A preview that missed changes (closed, or just opened) needs the whole document again.
			const target = previewReady ? preview : undefined;
			const output = await compiler.compile('/' + main.path, !!target && !previewInSync);
			if (output.action && output.data) {
				if (target && (previewInSync || output.action === 'reset')) {
					await target.apply(output.action, output.data);
					previewInSync = true;
				} else {
					previewInSync = false;
				}
			}
			// A successful compilation may still carry warnings; the banner is for failures.
			errors = output.action ? [] : toEditorDiagnostics(output.diagnostics, (path) => texts.get(path));
		} catch (e) {
			errors = [{ message: `The preview could not be compiled: ${e instanceof Error ? e.message : e}`, severity: 'Error' }];
		} finally {
			compiling = false;
			if (compileAgain) {
				compileAgain = false;
				scheduleCompile();
			}
		}
	}

	function previewIsReady() {
		previewReady = true;
		previewInSync = false;
		triggerCompile();
	}

	// A closed preview takes nothing; when it comes back it starts empty.
	$effect(() => {
		if (!preview) previewReady = false;
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
		queueSync();
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
		$documentStatsStore = null;
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
			compiler?.dispose();
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

				<div class="flex flex-col min-h-0 min-w-0 [contain:strict] bg-[var(--theme-surface)] {$previewOpenStore ? 'w-full md:w-1/2 border-r border-gray-200 dark:border-white/10' : 'flex-1'}">
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
				<div class="{readOnly ? 'flex-1' : 'w-full md:w-1/2'} min-w-0 relative [contain:strict] bg-[var(--theme-panel)] flex flex-col">
					<ClientPreview bind:this={preview} onReady={previewIsReady} onLost={previewIsReady} />
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
