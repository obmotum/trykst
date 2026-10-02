<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { onMount } from 'svelte';
	import Icon from '@iconify/svelte';
	import { userStore } from '$lib/ts/auth';
	import { api, canWrite } from '$lib/ts/api';
	import type { Doc, Package, Project } from '$lib/ts/api';
	import Navbar from '$lib/components/dashboard/Navbar.svelte';
	import Footer from '$lib/components/Footer.svelte';
	import DocCard from '$lib/components/dashboard/DocCard.svelte';
	import CreateDocModal from '$lib/components/dashboard/CreateDocModal.svelte';
	import RenameModal from '$lib/components/dashboard/RenameModal.svelte';
	import DeleteModal from '$lib/components/dashboard/DeleteModal.svelte';
	import ShareModal from '$lib/components/ShareModal.svelte';
	import MembersModal from '$lib/components/project/MembersModal.svelte';
	import ProjectModal from '$lib/components/project/ProjectModal.svelte';

	const projectId = $page.params.id as string;

	let project = $state<Project | null>(null);
	let documents = $state<Doc[]>([]);
	let packages = $state<Package[]>([]);
	let loadError = $state('');
	let actionError = $state('');

	let isOwner = $derived(project?.role === 'owner');
	let canEdit = $derived(canWrite(project?.role));

	let showPlusDropdown = $state(false);
	let showProjectMenu = $state(false);
	let activeMenu = $state<string | null>(null);
	let showCreate = $state(false);
	let showMembers = $state(false);
	let showEditProject = $state(false);
	let renameTarget = $state<Doc | null>(null);
	let shareTarget = $state<Doc | null>(null);
	let deleteTarget = $state<{ id: string; type: string; name: string } | null>(null);
	let copied = $state('');
	let isImporting = $state(false);
	let typInput = $state<HTMLInputElement | null>(null);
	let importInput = $state<HTMLInputElement | null>(null);

	function fail(e: unknown) {
		actionError = e instanceof Error ? e.message : String(e);
	}

	async function loadProject() {
		project = await api<Project>('GET', `/api/projects/${projectId}`);
	}

	async function loadDocuments() {
		documents = await api<Doc[]>('GET', `/api/projects/${projectId}/documents`);
	}

	async function loadPackages() {
		packages = await api<Package[]>('GET', `/api/projects/${projectId}/packages`);
	}

	async function createDoc(title: string, content?: string) {
		if (!title.trim()) return;
		try {
			const doc = await api<Doc>('POST', `/api/projects/${projectId}/documents`, { title: title.trim(), content });
			goto(`/doc/${doc.id}`);
		} catch (e) {
			showCreate = false;
			fail(e);
		}
	}

	/** Each .typ file becomes a document with that file as its main file. */
	async function handleTypUpload(e: Event) {
		const input = e.target as HTMLInputElement;
		const files = Array.from(input.files ?? []);
		input.value = '';
		try {
			for (const file of files) {
				await api('POST', `/api/projects/${projectId}/documents`, {
					title: file.name.replace(/\.typ$/i, ''),
					content: await file.text()
				});
			}
			await loadDocuments();
		} catch (err) {
			fail(err);
		}
	}

	async function handleImport(e: Event) {
		const input = e.target as HTMLInputElement;
		const file = input.files?.[0];
		input.value = '';
		if (!file) return;
		isImporting = true;
		try {
			const form = new FormData();
			form.append('file', file);
			const res = await fetch('/api/import/pandoc', { method: 'POST', body: form });
			if (!res.ok) throw new Error(`Import failed: ${await res.text()}`);
			await createDoc(file.name.replace(/\.[^/.]+$/, ''), await res.text());
		} catch (err) {
			fail(err);
		}
		isImporting = false;
	}

	async function renameDoc(title: string) {
		if (!renameTarget || !title.trim()) return;
		try {
			await api('PATCH', `/api/documents/${renameTarget.id}`, { title: title.trim() });
			await loadDocuments();
		} catch (e) {
			fail(e);
		}
		renameTarget = null;
	}

	async function confirmDelete() {
		if (!deleteTarget) return;
		const { id, type } = deleteTarget;
		deleteTarget = null;
		try {
			if (type === 'project') {
				await api('DELETE', `/api/projects/${id}`);
				goto('/dashboard');
			} else if (type === 'package') {
				await api('DELETE', `/api/packages/${id}`);
				await loadPackages();
			} else {
				await api('DELETE', `/api/documents/${id}`);
				await Promise.all([loadDocuments(), loadProject()]);
			}
		} catch (e) {
			fail(e);
		}
	}

	async function saveProject(name: string, description: string) {
		project = await api<Project>('PATCH', `/api/projects/${projectId}`, { name, description });
		showEditProject = false;
	}

	function importSnippet(pkg: Package): string {
		return `#import "@project/${pkg.name}:${pkg.latest_version ?? '0.1.0'}": *`;
	}

	async function copySnippet(pkg: Package) {
		await navigator.clipboard.writeText(importSnippet(pkg));
		copied = pkg.id;
		setTimeout(() => (copied = ''), 2000);
	}

	function handleWindowClick(e: MouseEvent) {
		const target = e.target as HTMLElement;
		if (!target.closest('.plus-dropdown-container')) showPlusDropdown = false;
		if (!target.closest('.project-menu-container')) showProjectMenu = false;
		if (!target.closest('.action-menu-container')) activeMenu = null;
	}

	onMount(async () => {
		try {
			await loadProject();
			await Promise.all([loadDocuments(), loadPackages()]);
			if ($page.url.searchParams.has('members')) showMembers = true;
		} catch (e) {
			loadError = e instanceof Error ? e.message : 'Failed to load the project';
		}
	});
</script>

<svelte:head>
	<title>{project?.name ?? 'Project'} - Trykst</title>
</svelte:head>

<svelte:window onclick={handleWindowClick} />

<div class="min-h-screen flex flex-col">
	<Navbar />

	<main class="max-w-7xl w-full mx-auto py-8 px-4 sm:px-6 lg:px-8 grow">
		<a href="/dashboard" class="text-sm font-medium text-gray-600 hover:text-gray-900 dark:text-gray-300 dark:hover:text-white transition-colors mb-4 inline-flex items-center gap-1.5">
			<Icon icon="mdi:arrow-left" class="text-lg" /> All projects
		</a>

		{#if loadError}
			<div class="text-center py-16">
				<Icon icon="mdi:folder-hidden" class="text-5xl text-gray-400 mx-auto mb-3" />
				<h2 class="text-xl font-semibold mb-1">This project is not available</h2>
				<p class="text-sm text-gray-500 dark:text-gray-400">It may have been deleted, or you are not a member of it.</p>
			</div>
		{:else if !project}
			<div class="min-h-[40vh] flex items-center justify-center text-gray-500 dark:text-gray-400">
				<Icon icon="mdi:loading" class="text-4xl animate-spin" />
			</div>
		{:else}
			<div class="flex flex-wrap justify-between items-start gap-4 mb-8">
				<div class="min-w-0">
					<div class="flex items-center gap-3">
						<h2 class="text-3xl font-bold text-gray-900 dark:text-white tracking-tight truncate">{project.name}</h2>
						<span class="flex-shrink-0 text-xs font-semibold px-2 py-0.5 rounded-full capitalize {project.role === 'owner' ? 'bg-amber-100 dark:bg-amber-500/20 text-amber-700 dark:text-amber-300' : project.role === 'editor' ? 'bg-blue-100 dark:bg-blue-500/20 text-blue-700 dark:text-blue-300' : 'bg-gray-100 dark:bg-white/10 text-gray-600 dark:text-gray-400'}">{project.role}</span>
					</div>
					{#if project.description}
						<p class="text-sm text-gray-500 dark:text-gray-400 mt-1 max-w-2xl">{project.description}</p>
					{/if}
				</div>

				<div class="flex items-center gap-2">
					<button onclick={() => (showMembers = true)} class="inline-flex items-center gap-2 px-4 py-2.5 text-sm font-medium rounded-lg border border-gray-200 dark:border-white/10 bg-white/50 dark:bg-white/5 hover:bg-gray-100 dark:hover:bg-white/10 transition-colors">
						<Icon icon="mdi:account-multiple-outline" class="text-lg" /> Members <span class="text-gray-500 dark:text-gray-400">{project.member_count}</span>
					</button>

					{#if canEdit}
						<div class="relative plus-dropdown-container">
							<button onclick={() => (showPlusDropdown = !showPlusDropdown)} aria-haspopup="menu" aria-expanded={showPlusDropdown} class="inline-flex items-center gap-2 bg-blue-600 hover:bg-blue-700 text-white px-4 py-2.5 rounded-lg shadow-sm text-sm font-medium transition-colors">
								<Icon icon="mdi:plus" class="text-lg" /> New <Icon icon="mdi:chevron-down" class="text-sm opacity-80" />
							</button>
							{#if showPlusDropdown}
								<div class="absolute right-0 mt-2 w-60 bg-[var(--theme-bg)] rounded-lg shadow-xl border border-gray-200 dark:border-white/10 py-1 z-20" role="menu">
									<button role="menuitem" onclick={() => { showPlusDropdown = false; showCreate = true; }} class="w-full text-left px-4 py-2 text-sm hover:bg-black/5 dark:hover:bg-white/5 flex items-center gap-2">
										<Icon icon="mdi:file-document-plus" class="text-lg text-blue-500" /> New document
									</button>
									<button role="menuitem" onclick={() => { showPlusDropdown = false; typInput?.click(); }} class="w-full text-left px-4 py-2 text-sm hover:bg-black/5 dark:hover:bg-white/5 flex items-center gap-2">
										<Icon icon="mdi:upload" class="text-lg text-green-500" /> Upload .typ files
									</button>
									<button role="menuitem" disabled={isImporting} onclick={() => { showPlusDropdown = false; importInput?.click(); }} class="w-full text-left px-4 py-2 text-sm hover:bg-black/5 dark:hover:bg-white/5 flex items-center gap-2">
										<Icon icon="mdi:file-import" class="text-lg text-purple-500" /> Import (.docx, .tex, .md)
									</button>
								</div>
							{/if}
							<input type="file" bind:this={typInput} accept=".typ" multiple onchange={handleTypUpload} class="hidden" />
							<input type="file" bind:this={importInput} accept=".docx,.tex,.md,.html" onchange={handleImport} class="hidden" />
						</div>
					{/if}

					{#if isOwner}
						<div class="relative project-menu-container">
							<button onclick={() => (showProjectMenu = !showProjectMenu)} aria-label="Project actions" aria-haspopup="menu" aria-expanded={showProjectMenu} class="p-2.5 rounded-lg border border-gray-200 dark:border-white/10 bg-white/50 dark:bg-white/5 hover:bg-gray-100 dark:hover:bg-white/10 transition-colors">
								<Icon icon="mdi:dots-vertical" class="text-lg" />
							</button>
							{#if showProjectMenu}
								<div class="absolute right-0 mt-2 w-48 bg-[var(--theme-bg)] rounded-lg shadow-xl border border-gray-200 dark:border-white/10 py-1 z-20" role="menu">
									<button role="menuitem" onclick={() => { showProjectMenu = false; showEditProject = true; }} class="w-full text-left px-4 py-2 text-sm hover:bg-black/5 dark:hover:bg-white/5 flex items-center gap-2">
										<Icon icon="mdi:pencil-outline" class="text-lg text-yellow-500" /> Edit project
									</button>
									<button role="menuitem" onclick={() => { showProjectMenu = false; deleteTarget = { id: projectId, type: 'project', name: project!.name }; }} class="w-full text-left px-4 py-2 text-sm text-red-600 dark:text-red-400 hover:bg-red-500/10 flex items-center gap-2">
										<Icon icon="mdi:trash-can-outline" class="text-lg" /> Delete project
									</button>
								</div>
							{/if}
						</div>
					{/if}
				</div>
			</div>

			{#if actionError}
				<div class="mb-6 flex items-start gap-2 text-sm text-red-700 dark:text-red-300 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-900/40 rounded-lg px-4 py-3" role="alert">
					<Icon icon="mdi:alert-circle" class="text-lg flex-shrink-0" />
					<span class="flex-1">{actionError}</span>
					<button onclick={() => (actionError = '')} aria-label="Dismiss"><Icon icon="mdi:close" /></button>
				</div>
			{/if}
			{#if isImporting}
				<p class="mb-6 text-sm text-gray-500 dark:text-gray-400 flex items-center gap-2"><Icon icon="mdi:loading" class="animate-spin" /> Importing...</p>
			{/if}

			{#if documents.length === 0}
				<div class="text-center py-16">
					<div class="inline-flex items-center justify-center w-16 h-16 rounded-full bg-blue-100/50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 mb-4">
						<Icon icon="mdi:file-document-outline" class="text-3xl" />
					</div>
					<h3 class="text-lg font-bold text-gray-900 dark:text-white mb-1">No documents yet</h3>
					{#if canEdit}
						<p class="text-gray-500 dark:text-gray-400 mb-6 text-sm">A document holds its own files: Typst sources, images, fonts and bibliographies.</p>
						<button onclick={() => (showCreate = true)} class="inline-flex items-center gap-2 bg-blue-600 hover:bg-blue-700 text-white px-5 py-2.5 rounded-lg shadow-sm text-sm font-medium transition-colors">
							<Icon icon="mdi:plus" class="text-lg" /> Create document
						</button>
					{:else}
						<p class="text-gray-500 dark:text-gray-400 text-sm">Documents of this project appear here.</p>
					{/if}
				</div>
			{:else}
				<div class="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-6">
					{#each documents as doc (doc.id)}
						<DocCard
							{doc}
							canManage={canEdit}
							canDelete={isOwner || doc.created_by === $userStore?.id}
							{activeMenu}
							setActiveMenu={(id) => (activeMenu = id)}
							onRename={(d) => (renameTarget = d)}
							onShare={(d) => (shareTarget = d)}
							onDelete={(d) => (deleteTarget = { id: d.id, type: 'document', name: d.title })}
						/>
					{/each}
				</div>
			{/if}

			{#if packages.length > 0}
				<section class="mt-12">
					<h3 class="text-lg font-semibold text-gray-900 dark:text-white flex items-center gap-2 mb-1">
						<Icon icon="mdi:package-variant-closed" class="text-purple-500" /> Project packages
					</h3>
					<p class="text-sm text-gray-500 dark:text-gray-400 mb-4">Published from documents of this project and importable by all of them.</p>
					<div class="space-y-3">
						{#each packages as pkg (pkg.id)}
							<div class="bg-white dark:bg-black/20 rounded-xl border border-gray-200 dark:border-white/10 p-4 flex items-start justify-between gap-4">
								<div class="min-w-0">
									<div class="flex items-center gap-2">
										<p class="font-semibold text-gray-900 dark:text-white truncate">@project/{pkg.name}</p>
										{#if pkg.latest_version}
											<span class="text-xs font-mono bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300 px-1.5 py-0.5 rounded">v{pkg.latest_version}</span>
										{/if}
									</div>
									{#if pkg.description}<p class="text-sm text-gray-500 dark:text-gray-400 mt-1 truncate">{pkg.description}</p>{/if}
									<pre class="mt-2 text-xs font-mono bg-gray-50 dark:bg-black/30 border border-gray-100 dark:border-white/5 rounded px-2 py-1 overflow-x-auto">{importSnippet(pkg)}</pre>
								</div>
								<div class="flex flex-col items-end gap-2 flex-shrink-0">
									<button onclick={() => copySnippet(pkg)} class="text-xs px-2 py-1 rounded-md bg-gray-100 dark:bg-white/5 hover:bg-gray-200 dark:hover:bg-white/10 flex items-center gap-1">
										<Icon icon={copied === pkg.id ? 'mdi:check' : 'mdi:content-copy'} class="text-sm" /> {copied === pkg.id ? 'Copied' : 'Copy'}
									</button>
									{#if isOwner}
										<button onclick={() => (deleteTarget = { id: pkg.id, type: 'package', name: `@project/${pkg.name}` })} class="text-xs px-2 py-1 rounded-md text-gray-500 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-900/20 flex items-center gap-1">
											<Icon icon="mdi:trash-can-outline" class="text-sm" /> Delete
										</button>
									{/if}
								</div>
							</div>
						{/each}
					</div>
				</section>
			{/if}
		{/if}
	</main>

	<Footer sticky={false} />
</div>

{#if showCreate}
	<CreateDocModal {createDoc} onClose={() => (showCreate = false)} />
{/if}

{#if renameTarget}
	<RenameModal initialTitle={renameTarget.title} handleRename={renameDoc} onClose={() => (renameTarget = null)} />
{/if}

{#if shareTarget}
	<ShareModal doc={shareTarget} onDocChanged={(updated) => { shareTarget = updated; documents = documents.map((d) => (d.id === updated.id ? updated : d)); }} onClose={() => (shareTarget = null)} />
{/if}

{#if deleteTarget}
	<DeleteModal {deleteTarget} {confirmDelete} onClose={() => (deleteTarget = null)} />
{/if}

{#if showMembers && project}
	<MembersModal {project} onClose={() => (showMembers = false)} onChanged={loadProject} onLeft={() => goto('/dashboard')} />
{/if}

{#if showEditProject && project}
	<ProjectModal title="Edit project" submitLabel="Save" name={project.name} description={project.description ?? ''} onSubmit={saveProject} onClose={() => (showEditProject = false)} />
{/if}
