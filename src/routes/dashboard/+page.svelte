<script lang="ts">
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import Icon from '@iconify/svelte';
	import { userStore } from '$lib/ts/auth';
	import { api, formatDate } from '$lib/ts/api';
	import type { Project } from '$lib/ts/api';
	import Navbar from '$lib/components/dashboard/Navbar.svelte';
	import Footer from '$lib/components/Footer.svelte';
	import ProjectModal from '$lib/components/project/ProjectModal.svelte';

	let projects = $state<Project[]>([]);
	let loading = $state(true);
	let error = $state('');
	let showCreate = $state(false);

	// Guests work in the projects they were added to; they cannot open their own.
	const isGuest = $derived(!!$userStore?.is_guest);

	async function createProject(name: string, description: string) {
		const project = await api<Project>('POST', '/api/projects', { name, description });
		goto(`/project/${project.id}`);
	}

	onMount(async () => {
		try {
			projects = await api<Project[]>('GET', '/api/projects');
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load projects';
		}
		loading = false;
	});
</script>

<svelte:head>
	<title>Projects - Trykst</title>
	<meta name="description" content="Your Trykst projects." />
</svelte:head>

<div class="min-h-screen flex flex-col">
	<Navbar />

	<main class="max-w-7xl w-full mx-auto py-10 px-4 sm:px-6 lg:px-8 grow">
		<div class="flex justify-between items-center mb-8">
			<div>
				<h2 class="text-3xl font-bold text-gray-900 dark:text-white tracking-tight">Projects</h2>
				<p class="text-sm text-gray-500 dark:text-gray-400 mt-1">A project holds documents and the people who work on them.</p>
			</div>
			{#if !isGuest}
				<button onclick={() => (showCreate = true)} class="inline-flex items-center gap-2 bg-blue-600 hover:bg-blue-700 text-white px-4 py-2.5 rounded-lg shadow-sm text-sm font-medium transition-colors">
					<Icon icon="mdi:plus" class="text-lg" /> New project
				</button>
			{/if}
		</div>

		{#if loading}
			<div class="min-h-[40vh] flex items-center justify-center text-gray-500 dark:text-gray-400">
				<Icon icon="mdi:loading" class="text-4xl animate-spin" />
			</div>
		{:else if error}
			<p class="text-red-600 dark:text-red-400" role="alert">{error}</p>
		{:else if projects.length === 0}
			<div class="text-center py-16">
				<div class="inline-flex items-center justify-center w-16 h-16 rounded-full bg-blue-100/50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 mb-4">
					<Icon icon="mdi:folder-account-outline" class="text-3xl" />
				</div>
				<h3 class="text-lg font-bold text-gray-900 dark:text-white mb-1">No projects yet</h3>
				{#if isGuest}
					<p class="text-gray-500 dark:text-gray-400 text-sm">Projects you are added to appear here.</p>
				{:else}
					<p class="text-gray-500 dark:text-gray-400 mb-6 text-sm">Create a project, add documents to it and invite the people you work with.</p>
					<button onclick={() => (showCreate = true)} class="inline-flex items-center gap-2 bg-blue-600 hover:bg-blue-700 text-white px-5 py-2.5 rounded-lg shadow-sm text-sm font-medium transition-colors">
						<Icon icon="mdi:plus" class="text-lg" /> Create project
					</button>
				{/if}
			</div>
		{:else}
			<div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
				{#each projects as project (project.id)}
					<a href="/project/{project.id}" class="group bg-white dark:bg-black/20 rounded-xl shadow-sm border border-gray-200 dark:border-white/10 p-5 flex flex-col gap-3 hover:shadow-lg hover:border-blue-400 dark:hover:border-blue-500/50 transition-all duration-200 hover:-translate-y-0.5">
						<div class="flex items-start gap-3">
							<div class="p-2.5 bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400 rounded-lg flex-shrink-0">
								<Icon icon="mdi:folder-account" class="text-2xl" />
							</div>
							<div class="min-w-0 flex-1">
								<h3 class="text-lg font-semibold text-gray-900 dark:text-white truncate" title={project.name}>{project.name}</h3>
								<p class="text-sm text-gray-500 dark:text-gray-400 line-clamp-2 min-h-[2.5rem]">{project.description ?? ''}</p>
							</div>
							<span class="flex-shrink-0 text-xs font-semibold px-2 py-0.5 rounded-full capitalize {project.role === 'owner' ? 'bg-amber-100 dark:bg-amber-500/20 text-amber-700 dark:text-amber-300' : project.role === 'editor' ? 'bg-blue-100 dark:bg-blue-500/20 text-blue-700 dark:text-blue-300' : 'bg-gray-100 dark:bg-white/10 text-gray-600 dark:text-gray-400'}">{project.role}</span>
						</div>
						<div class="flex items-center gap-4 text-xs text-gray-500 dark:text-gray-400 mt-auto pt-2 border-t border-gray-100 dark:border-white/5">
							<span class="flex items-center gap-1"><Icon icon="mdi:file-document-multiple-outline" class="text-sm" /> {project.document_count} {project.document_count === 1 ? 'document' : 'documents'}</span>
							<span class="flex items-center gap-1"><Icon icon="mdi:account-multiple-outline" class="text-sm" /> {project.member_count}</span>
							<span class="flex items-center gap-1 ml-auto"><Icon icon="mdi:clock-outline" class="text-sm" /> {formatDate(project.updated_at)}</span>
						</div>
					</a>
				{/each}
			</div>
		{/if}
	</main>

	<Footer sticky={false} />
</div>

{#if showCreate}
	<ProjectModal title="New project" submitLabel="Create" onSubmit={createProject} onClose={() => (showCreate = false)} />
{/if}
