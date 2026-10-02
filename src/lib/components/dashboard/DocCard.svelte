<script lang="ts">
	import Icon from '@iconify/svelte';
	import { formatDate } from '$lib/ts/api';
	import type { Doc } from '$lib/ts/api';

	let {
		doc,
		canManage = false,
		canDelete = false,
		activeMenu,
		setActiveMenu,
		onRename,
		onShare,
		onDelete
	}: {
		doc: Doc;
		/** Project editors and owners rename and share documents. */
		canManage?: boolean;
		canDelete?: boolean;
		activeMenu: string | null;
		setActiveMenu: (id: string | null) => void;
		onRename: (doc: Doc) => void;
		onShare: (doc: Doc) => void;
		onDelete: (doc: Doc) => void;
	} = $props();

	let dropUp = $state(false);

	function toggleMenu(e: MouseEvent) {
		e.preventDefault();
		e.stopPropagation();
		if (activeMenu === doc.id) {
			setActiveMenu(null);
		} else {
			const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
			dropUp = window.innerHeight - rect.bottom < 160;
			setActiveMenu(doc.id);
		}
	}

	function act(e: MouseEvent, action: (doc: Doc) => void) {
		e.preventDefault();
		e.stopPropagation();
		setActiveMenu(null);
		action(doc);
	}
</script>

<a
	href="/doc/{doc.id}"
	class="bg-white dark:bg-black/20 rounded-xl shadow-sm border border-gray-200 dark:border-white/10 flex flex-col hover:shadow-lg hover:border-blue-400 dark:hover:border-blue-500/50 transition-all duration-200 relative group hover:-translate-y-1 overflow-visible"
>
	<div class="h-40 w-full bg-gray-50 dark:bg-black/40 rounded-t-xl overflow-hidden flex items-center justify-center border-b border-gray-100 dark:border-white/10 relative pointer-events-none">
		{#if doc.thumbnail_svg}
			<div class="w-full h-full flex items-start justify-center bg-white transition-transform duration-300 group-hover:scale-110">
				<img src={`data:image/svg+xml;base64,${btoa(unescape(encodeURIComponent(doc.thumbnail_svg)))}`} class="w-full h-auto shadow-sm border border-gray-200" alt="" draggable="false" />
			</div>
		{:else}
			<div class="p-4 bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400 rounded-full transition-transform duration-300 group-hover:scale-110">
				<Icon icon="mdi:file-document" class="text-4xl" />
			</div>
		{/if}
	</div>

	<div class="p-4 flex flex-col flex-grow">
		<div class="flex items-start justify-between">
			<h3 class="text-lg font-semibold text-gray-900 dark:text-white truncate pr-2" title={doc.title}>{doc.title}</h3>

			{#if canManage || canDelete}
				<div class="relative action-menu-container">
					<button aria-label="Actions for {doc.title}" onclick={toggleMenu} class="text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 transition-colors p-1 rounded-full hover:bg-gray-100 dark:hover:bg-white/10">
						<Icon icon="mdi:dots-vertical" class="text-xl" />
					</button>

					{#if activeMenu === doc.id}
						<div class="absolute right-0 {dropUp ? 'bottom-full mb-1' : 'top-full mt-1'} w-44 bg-[var(--theme-bg)] rounded-xl shadow-xl border border-gray-200 dark:border-white/10 py-1 z-[100]">
							{#if canManage}
								<button onclick={(e) => act(e, onRename)} class="w-full text-left px-4 py-2 text-sm text-gray-700 dark:text-gray-200 hover:bg-black/5 dark:hover:bg-white/5 flex items-center gap-2">
									<Icon icon="mdi:pencil-outline" class="text-lg text-yellow-500" /> Rename
								</button>
								<button onclick={(e) => act(e, onShare)} class="w-full text-left px-4 py-2 text-sm text-gray-700 dark:text-gray-200 hover:bg-black/5 dark:hover:bg-white/5 flex items-center gap-2">
									<Icon icon="mdi:share-variant-outline" class="text-lg text-green-500" /> Share
								</button>
							{/if}
							{#if canDelete}
								{#if canManage}<div class="h-px bg-gray-200 dark:bg-white/10 my-1"></div>{/if}
								<button onclick={(e) => act(e, onDelete)} class="w-full text-left px-4 py-2 text-sm text-red-600 dark:text-red-400 hover:bg-red-500/10 flex items-center gap-2">
									<Icon icon="mdi:trash-can-outline" class="text-lg" /> Delete
								</button>
							{/if}
						</div>
					{/if}
				</div>
			{/if}
		</div>
		<p class="text-xs text-gray-500 dark:text-gray-400 flex items-center gap-1 mt-2">
			<Icon icon="mdi:clock-outline" class="text-sm" />
			Edited {formatDate(doc.updated_at)}
			{#if doc.public_role}
				<span class="ml-auto flex items-center gap-1 text-emerald-600 dark:text-emerald-400" title="Anyone with the link: {doc.public_role}"><Icon icon="mdi:earth" class="text-sm" /> Link</span>
			{/if}
		</p>
	</div>
</a>
