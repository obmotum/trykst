import * as Y from 'yjs';
import { WebsocketProvider } from 'y-websocket';
import { get } from 'svelte/store';
import { userStore } from './auth';
import { connectionStatus, connectedUsers } from './store';
import type { AwarenessUser } from './store';

/**
 * Collaboration rooms of the open document: one Yjs room per text file, opened
 * when the file is first selected and kept until the document is left.
 */
export interface OpenFile {
	nodeId: string;
	doc: Y.Doc;
	text: Y.Text;
	provider: WebsocketProvider;
}

const userColors = [
	'#30bced', '#6eeb83', '#ffbc42', '#ecd444', '#ee6352',
	'#9ac2c9', '#8acb88', '#1be7ff', '#ff0054', '#9e0059'
];

const TEXT_NAME = 'typst';

const open = new Map<string, OpenFile>();
let documentId: string | null = null;
let activeId: string | null = null;
let color = userColors[0];

export function setDocument(id: string) {
	documentId = id;
	color = userColors[Math.floor(Math.random() * userColors.length)];
}

/** The status bar and the avatars follow the file that is being edited. */
function publishPresence(entry: OpenFile) {
	if (entry.nodeId !== activeId) return;
	const localId = entry.provider.awareness.clientID;
	const users = new Map<string, AwarenessUser>();
	entry.provider.awareness.getStates().forEach((state, clientId) => {
		if (!state.user) return;
		const isLocal = clientId === localId;
		// One avatar per person, preferring the local session.
		if (isLocal || !users.get(state.user.name)?.isLocal) {
			users.set(state.user.name, { clientId, ...state.user, isLocal });
		}
	});
	connectedUsers.set(Array.from(users.values()));
}

export function openFile(nodeId: string): OpenFile {
	const existing = open.get(nodeId);
	if (existing) return existing;
	if (!documentId) throw new Error('Document not set');

	const doc = new Y.Doc();
	const text = doc.getText(TEXT_NAME);
	const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
	const provider = new WebsocketProvider(
		`${protocol}//${window.location.host}/yjs`,
		`doc:${documentId}:${nodeId}`,
		doc
	);

	provider.awareness.setLocalStateField('user', {
		name: get(userStore)?.username || 'Anonymous',
		color,
		colorLight: color + '33'
	});

	const entry: OpenFile = { nodeId, doc, text, provider };
	provider.on('status', (event: { status: string }) => {
		if (entry.nodeId === activeId) connectionStatus.set(event.status);
	});
	provider.awareness.on('change', () => publishPresence(entry));

	open.set(nodeId, entry);
	return entry;
}

export function getOpenFile(nodeId: string): OpenFile | undefined {
	return open.get(nodeId);
}

export function setActiveFile(nodeId: string | null) {
	activeId = nodeId;
	const entry = nodeId ? open.get(nodeId) : undefined;
	if (entry) {
		connectionStatus.set(entry.provider.wsconnected ? 'connected' : 'connecting');
		publishPresence(entry);
	} else {
		connectedUsers.set([]);
	}
}

export function closeFile(nodeId: string) {
	const entry = open.get(nodeId);
	if (!entry) return;
	entry.provider.disconnect();
	entry.provider.destroy();
	entry.doc.destroy();
	open.delete(nodeId);
	if (activeId === nodeId) setActiveFile(null);
}

/**
 * Text of every open file that has finished syncing, by node id. Sent along
 * with compile requests so the preview never lags behind the editor.
 */
export function openTexts(): Map<string, string> {
	const result = new Map<string, string>();
	for (const entry of open.values()) {
		if (entry.provider.synced) result.set(entry.nodeId, entry.text.toString());
	}
	return result;
}

export function closeAllFiles() {
	for (const nodeId of Array.from(open.keys())) closeFile(nodeId);
}

export function cleanupDocument() {
	closeAllFiles();
	documentId = null;
	activeId = null;
	connectionStatus.set('disconnected');
	connectedUsers.set([]);
}
