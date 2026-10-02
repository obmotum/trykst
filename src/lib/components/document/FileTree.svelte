<script lang="ts">
	import Icon from '@iconify/svelte';
	import type { TreeNode } from '$lib/ts/api';

	const NODE_MIME = 'application/x-trykst-node';

	let {
		nodes = [],
		activeId = '',
		entrypointId = null,
		readOnly = false,
		error = '',
		contentUrl,
		onSelect,
		onCreate,
		onUpload,
		onRename,
		onMove,
		onDelete,
		onSetEntry
	}: {
		nodes?: TreeNode[];
		activeId?: string;
		entrypointId?: string | null;
		readOnly?: boolean;
		error?: string;
		contentUrl: (node: TreeNode) => string;
		onSelect: (node: TreeNode) => void;
		onCreate: (parentId: string | null, name: string, kind: 'text' | 'folder') => void;
		onUpload: (parentId: string | null, files: File[]) => void;
		onRename: (node: TreeNode, name: string) => void;
		onMove: (node: TreeNode, parentId: string | null) => void;
		onDelete: (node: TreeNode) => void;
		onSetEntry: (node: TreeNode) => void;
	} = $props();

	let collapsed = $state<Record<string, boolean>>({});
	let creating = $state<{ parentId: string | null; kind: 'text' | 'folder' } | null>(null);
	let renamingId = $state<string | null>(null);
	let inputValue = $state('');
	let dropTarget = $state<string | null>(null); // folder id, or 'root'
	let menu = $state<{ x: number; y: number; node: TreeNode | null } | null>(null);
	let fileInput: HTMLInputElement = $state()!;
	let uploadParent: string | null = null;

	// The server sends each folder's children in display order (folders first).
	let childrenByParent = $derived.by(() => {
		const map = new Map<string, TreeNode[]>();
		for (const node of nodes) {
			const key = node.parent_id ?? '';
			if (!map.has(key)) map.set(key, []);
			map.get(key)!.push(node);
		}
		return map;
	});

	function iconFor(node: TreeNode): string {
		if (node.kind === 'folder') return collapsed[node.id] ? 'mdi:folder' : 'mdi:folder-open';
		const lower = node.name.toLowerCase();
		if (lower.endsWith('.typ')) return 'mdi:file-document-outline';
		if (lower.endsWith('.toml') || lower.endsWith('.yml') || lower.endsWith('.yaml') || lower.endsWith('.json')) return 'mdi:cog-outline';
		if (lower.endsWith('.bib')) return 'mdi:book-open-variant';
		if (/\.(png|jpe?g|gif|webp|svg)$/.test(lower)) return 'mdi:image-outline';
		if (/\.(ttf|otf|woff2?)$/.test(lower)) return 'mdi:format-font';
		if (lower.endsWith('.pdf')) return 'mdi:file-pdf-box';
		return 'mdi:file-outline';
	}

	/** Focuses the name field with the name preselected (without the extension), ready to type over. */
	function focusAndSelect(el: HTMLInputElement) {
		// The binding fills the field only after this runs; without the value there is nothing to select.
		el.value = inputValue;
		el.focus();
		const dot = el.value.lastIndexOf('.');
		el.setSelectionRange(0, dot > 0 ? dot : el.value.length);
	}

	function startCreate(parentId: string | null, kind: 'text' | 'folder') {
		menu = null;
		renamingId = null;
		if (parentId) collapsed[parentId] = false;
		inputValue = kind === 'text' ? 'untitled.typ' : 'New folder';
		creating = { parentId, kind };
	}

	function startRename(node: TreeNode) {
		menu = null;
		creating = null;
		inputValue = node.name;
		renamingId = node.id;
	}

	function commitInput() {
		const name = inputValue.trim();
		if (creating && name) onCreate(creating.parentId, name, creating.kind);
		if (renamingId && name) {
			const node = nodes.find((n) => n.id === renamingId);
			if (node && name !== node.name) onRename(node, name);
		}
		creating = null;
		renamingId = null;
	}

	function cancelInput() {
		creating = null;
		renamingId = null;
	}

	function inputKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			e.preventDefault();
			commitInput();
		} else if (e.key === 'Escape') {
			e.preventDefault();
			cancelInput();
		}
	}

	function pickUpload(parentId: string | null) {
		menu = null;
		uploadParent = parentId;
		fileInput.click();
	}

	/** Runs a menu action on the menu's node; the menu closes first. */
	function pick(action: (node: TreeNode) => void) {
		const node = menu?.node;
		menu = null;
		if (node) action(node);
	}

	function click(node: TreeNode) {
		if (node.kind === 'folder') collapsed[node.id] = !collapsed[node.id];
		else onSelect(node);
	}

	function openMenu(e: MouseEvent, node: TreeNode | null) {
		if (readOnly && (!node || node.kind === 'folder')) return;
		e.preventDefault();
		e.stopPropagation();
		const x = Math.min(e.clientX, window.innerWidth - 200);
		const y = Math.min(e.clientY, window.innerHeight - 220);
		menu = { x, y, node };
	}

	// --- Drag and drop: move nodes, or drop files from the desktop to upload them ---

	function accepts(e: DragEvent): boolean {
		const types = e.dataTransfer?.types ?? [];
		return !readOnly && (types.includes(NODE_MIME) || types.includes('Files'));
	}

	/** The folder a drop on `node` goes into: the folder itself, or a file's parent. */
	function folderOf(node: TreeNode | null): string | null {
		if (!node) return null;
		return node.kind === 'folder' ? node.id : node.parent_id;
	}

	function dragOver(e: DragEvent, node: TreeNode | null) {
		if (!accepts(e)) return;
		e.preventDefault();
		e.stopPropagation();
		dropTarget = folderOf(node) ?? 'root';
	}

	function drop(e: DragEvent, node: TreeNode | null) {
		if (!accepts(e)) return;
		e.preventDefault();
		e.stopPropagation();
		dropTarget = null;
		const target = folderOf(node);

		const movedId = e.dataTransfer?.getData(NODE_MIME);
		if (movedId) {
			const moved = nodes.find((n) => n.id === movedId);
			if (!moved || moved.parent_id === target || moved.id === target) return;
			// A folder cannot go into itself or one of its own subfolders.
			const targetPath = target ? nodes.find((n) => n.id === target)?.path ?? '' : '';
			if (moved.kind === 'folder' && (targetPath + '/').startsWith(moved.path + '/')) return;
			onMove(moved, target);
			return;
		}
		const files = Array.from(e.dataTransfer?.files ?? []);
		if (files.length) {
			if (target) collapsed[target] = false;
			onUpload(target, files);
		}
	}
</script>

<svelte:window onclick={() => (menu = null)} />

{#snippet nameInput(depth: number, icon: string)}
	<div class="flex items-center gap-1.5 py-1 pr-2" style="padding-left: {depth * 14 + 22}px">
		<Icon {icon} class="text-base flex-shrink-0 text-gray-400" />
		<input
			use:focusAndSelect
			bind:value={inputValue}
			onkeydown={inputKeydown}
			onblur={cancelInput}
			class="flex-1 min-w-0 text-sm px-1.5 py-0.5 rounded border border-blue-500 bg-[var(--theme-bg)] text-[var(--theme-text)] focus:outline-none focus:ring-1 focus:ring-blue-500"
		/>
	</div>
{/snippet}

{#snippet branch(parentId: string | null, depth: number)}
	{#each childrenByParent.get(parentId ?? '') ?? [] as node (node.id)}
		{#if renamingId === node.id}
			{@render nameInput(depth, iconFor(node))}
		{:else}
			<div
				role="treeitem"
				aria-selected={activeId === node.id}
				aria-expanded={node.kind === 'folder' ? !collapsed[node.id] : undefined}
				tabindex="0"
				draggable={!readOnly}
				ondragstart={(e) => {
					e.dataTransfer?.setData(NODE_MIME, node.id);
					if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
				}}
				ondragover={(e) => dragOver(e, node)}
				ondrop={(e) => drop(e, node)}
				onclick={() => click(node)}
				onkeydown={(e) => {
					if (e.key === 'Enter') click(node);
					if (e.key === 'F2' && !readOnly) startRename(node);
				}}
				oncontextmenu={(e) => openMenu(e, node)}
				class="group flex items-center gap-1.5 py-1 pr-1.5 text-sm cursor-pointer select-none outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-blue-500
					{activeId === node.id
					? 'bg-blue-50 dark:bg-blue-900/20 text-blue-700 dark:text-blue-300'
					: 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-white/5'}
					{node.kind === 'folder' && dropTarget === node.id ? 'ring-1 ring-inset ring-blue-500 bg-blue-50 dark:bg-blue-900/20' : ''}"
				style="padding-left: {depth * 14 + 6}px"
			>
				<Icon
					icon="mdi:chevron-right"
					class="text-sm flex-shrink-0 text-gray-400 transition-transform {node.kind === 'folder' ? '' : 'invisible'} {collapsed[node.id] ? '' : 'rotate-90'}"
				/>
				<Icon icon={iconFor(node)} class="text-base flex-shrink-0 {node.kind === 'folder' ? 'text-amber-500' : ''}" />
				<span class="truncate flex-1 min-w-0" title={node.path}>{node.name}</span>
				{#if node.id === entrypointId}
					<span title="Main file: the document is compiled from here" class="flex-shrink-0">
						<Icon icon="mdi:star" class="text-amber-500 text-xs" />
					</span>
				{/if}
				<button
					onclick={(e) => openMenu(e, node)}
					title="Actions"
					aria-label="Actions for {node.name}"
					class="flex-shrink-0 p-0.5 rounded opacity-0 group-hover:opacity-100 focus:opacity-100 hover:bg-gray-200 dark:hover:bg-white/10 text-gray-500 {readOnly && node.kind === 'folder' ? 'hidden' : ''}"
				>
					<Icon icon="mdi:dots-horizontal" class="text-sm" />
				</button>
			</div>
		{/if}
		{#if node.kind === 'folder' && !collapsed[node.id]}
			{#if creating && creating.parentId === node.id}
				{@render nameInput(depth + 1, creating.kind === 'folder' ? 'mdi:folder' : 'mdi:file-outline')}
			{/if}
			{@render branch(node.id, depth + 1)}
		{/if}
	{/each}
{/snippet}

<div class="h-full flex flex-col bg-[var(--theme-panel)] border-r border-gray-200 dark:border-white/10">
	<div class="flex items-center justify-between px-3 py-2 border-b border-gray-200 dark:border-white/10">
		<span class="text-xs font-semibold uppercase tracking-wide text-gray-500 dark:text-gray-400">Files</span>
		{#if !readOnly}
			<div class="flex items-center gap-0.5">
				<button onclick={() => startCreate(null, 'text')} title="New file" class="p-1 rounded hover:bg-gray-100 dark:hover:bg-white/10 text-gray-600 dark:text-gray-300">
					<Icon icon="mdi:file-plus-outline" class="text-lg" />
				</button>
				<button onclick={() => startCreate(null, 'folder')} title="New folder" class="p-1 rounded hover:bg-gray-100 dark:hover:bg-white/10 text-gray-600 dark:text-gray-300">
					<Icon icon="mdi:folder-plus-outline" class="text-lg" />
				</button>
				<button onclick={() => pickUpload(null)} title="Upload files" class="p-1 rounded hover:bg-gray-100 dark:hover:bg-white/10 text-gray-600 dark:text-gray-300">
					<Icon icon="mdi:upload" class="text-lg" />
				</button>
			</div>
		{/if}
	</div>
	<input
		bind:this={fileInput}
		type="file"
		multiple
		class="hidden"
		onchange={(e) => {
			const input = e.currentTarget;
			if (input.files?.length) onUpload(uploadParent, Array.from(input.files));
			input.value = '';
		}}
	/>

	<div
		role="tree"
		tabindex="-1"
		aria-label="Files of this document"
		class="flex-1 overflow-y-auto py-1 {dropTarget === 'root' ? 'bg-blue-50/60 dark:bg-blue-900/10' : ''}"
		ondragover={(e) => dragOver(e, null)}
		ondragleave={() => (dropTarget = null)}
		ondrop={(e) => drop(e, null)}
		oncontextmenu={(e) => openMenu(e, null)}
	>
		{#if creating && creating.parentId === null}
			{@render nameInput(0, creating.kind === 'folder' ? 'mdi:folder' : 'mdi:file-outline')}
		{/if}
		{@render branch(null, 0)}
		{#if nodes.length === 0 && !creating}
			<p class="px-3 py-4 text-xs text-gray-500 dark:text-gray-400">No files yet.</p>
		{/if}
	</div>

	{#if error}
		<div class="px-3 py-2 text-xs text-red-600 dark:text-red-400 border-t border-gray-200 dark:border-white/10 break-words" role="alert">{error}</div>
	{/if}
</div>

{#if menu}
	{@const node = menu.node}
	{@const folderId = node?.kind === 'folder' ? node.id : null}
	<div
		role="menu"
		tabindex="-1"
		class="fixed z-[9999] min-w-[180px] py-1 rounded-lg shadow-xl border border-[var(--theme-border)] bg-[var(--theme-bg)] text-[var(--theme-text)] text-sm"
		style="left: {menu.x}px; top: {menu.y}px"
		onclick={(e) => e.stopPropagation()}
		onkeydown={(e) => e.key === 'Escape' && (menu = null)}
	>
		{#if !readOnly && (!node || node.kind === 'folder')}
			<button role="menuitem" onclick={() => startCreate(folderId, 'text')} class="w-full text-left px-3 py-1.5 hover:bg-[var(--theme-border)] flex items-center gap-2"><Icon icon="mdi:file-plus-outline" /> New file</button>
			<button role="menuitem" onclick={() => startCreate(folderId, 'folder')} class="w-full text-left px-3 py-1.5 hover:bg-[var(--theme-border)] flex items-center gap-2"><Icon icon="mdi:folder-plus-outline" /> New folder</button>
			<button role="menuitem" onclick={() => pickUpload(folderId)} class="w-full text-left px-3 py-1.5 hover:bg-[var(--theme-border)] flex items-center gap-2"><Icon icon="mdi:upload" /> Upload files</button>
		{/if}
		{#if node}
			{#if node.kind !== 'folder'}
				{#if !readOnly && node.kind === 'text' && node.name.toLowerCase().endsWith('.typ') && node.id !== entrypointId}
					<button role="menuitem" onclick={() => pick(onSetEntry)} class="w-full text-left px-3 py-1.5 hover:bg-[var(--theme-border)] flex items-center gap-2"><Icon icon="mdi:star-outline" /> Set as main file</button>
				{/if}
				<a role="menuitem" href={contentUrl(node)} download={node.name} onclick={() => (menu = null)} class="w-full text-left px-3 py-1.5 hover:bg-[var(--theme-border)] flex items-center gap-2"><Icon icon="mdi:download" /> Download</a>
			{/if}
			{#if !readOnly}
				{#if node.kind === 'folder'}<div class="h-px bg-[var(--theme-border)] my-1"></div>{/if}
				<button role="menuitem" onclick={() => startRename(node)} class="w-full text-left px-3 py-1.5 hover:bg-[var(--theme-border)] flex items-center gap-2"><Icon icon="mdi:pencil-outline" /> Rename</button>
				<button role="menuitem" onclick={() => pick(onDelete)} class="w-full text-left px-3 py-1.5 text-red-500 hover:bg-red-500/10 flex items-center gap-2"><Icon icon="mdi:trash-can-outline" /> Delete</button>
			{/if}
		{/if}
	</div>
{/if}
