/** Types and small helpers for the projects / documents / file tree API. */

export type Role = 'owner' | 'editor' | 'viewer';

export interface Project {
	id: string;
	name: string;
	description: string | null;
	created_by: string | null;
	created_at: string;
	updated_at: string;
	role: Role;
	document_count: number;
	member_count: number;
}

export interface ProjectMember {
	user_id: string;
	username: string;
	email: string | null;
	avatar_url: string | null;
	is_guest: boolean;
	role: Role;
	created_at: string;
}

export interface Doc {
	id: string;
	project_id: string;
	title: string;
	entrypoint_id: string | null;
	thumbnail_svg: string | null;
	public_role: 'viewer' | 'editor' | null;
	created_by: string | null;
	created_at: string;
	updated_at: string;
	role: Role;
	/** False when access comes from link sharing instead of project membership. */
	is_member: boolean;
}

export interface TreeNode {
	id: string;
	parent_id: string | null;
	name: string;
	kind: 'folder' | 'text' | 'binary';
	mime_type: string | null;
	size: number;
	/** Path from the document root, e.g. `chapters/intro.typ`. */
	path: string;
	updated_at: string;
}

export interface Tree {
	entrypoint_id: string | null;
	nodes: TreeNode[];
}

export interface Package {
	id: string;
	project_id: string | null;
	name: string;
	description: string | null;
	owner_name: string | null;
	latest_version: string | null;
}

export function canWrite(role: Role | undefined | null): boolean {
	return role === 'owner' || role === 'editor';
}

/** Sends JSON and returns the parsed response; throws the server's message on failure. */
export async function api<T = unknown>(method: string, path: string, body?: unknown): Promise<T> {
	const res = await fetch(path, {
		method,
		headers: body === undefined ? undefined : { 'Content-Type': 'application/json' },
		body: body === undefined ? undefined : JSON.stringify(body)
	});
	if (!res.ok) {
		throw new Error((await res.text()) || `Request failed (${res.status})`);
	}
	const text = await res.text();
	return (text ? JSON.parse(text) : undefined) as T;
}

/** Server timestamps are UTC without a zone suffix. */
export function parseTimestamp(value: string): Date {
	return new Date(/[zZ]|[+-]\d\d:\d\d$/.test(value) ? value : value.replace(' ', 'T') + 'Z');
}

export function formatDate(value: string): string {
	return parseTimestamp(value).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}

export function formatDateTime(value: string): string {
	return parseTimestamp(value).toLocaleString(undefined, {
		month: 'short',
		day: 'numeric',
		hour: 'numeric',
		minute: '2-digit'
	});
}
