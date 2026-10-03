import type { Diagnostic } from './typst-api';
import type { DocumentInsights, WorkerDiagnostic, WorkerFile, WorkerRequest, WorkerResponse } from './compile-worker';

export type { DocumentInsights, WorkerFile };

export interface CompileOutput {
	ms: number;
	/** What to do with `data` in the preview; absent when compiling failed. */
	action?: 'reset' | 'merge';
	data?: Uint8Array;
	diagnostics: WorkerDiagnostic[];
}

/**
 * The Typst compiler in a Web Worker. One instance per open document; the
 * fonts of the document are fixed when it is created.
 */
export class ClientCompiler {
	private worker: Worker;
	private nextId = 1;
	private pending = new Map<number, (output: CompileOutput) => void>();
	private pendingInsights = new Map<number, (insights: DocumentInsights | undefined) => void>();
	/** Resolves when the compiler has loaded; rejects when it cannot start. */
	readonly ready: Promise<void>;

	constructor(documentId: string, fonts: Uint8Array[]) {
		this.worker = new Worker(new URL('./compile-worker.ts', import.meta.url), { type: 'module' });
		this.ready = new Promise((resolve, reject) => {
			this.worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
				const message = event.data;
				if (message.type === 'ready') resolve();
				else if (message.type === 'failed') reject(new Error(message.message));
				else if (message.type === 'compiled') {
					this.pending.get(message.id)?.(message);
					this.pending.delete(message.id);
				} else if (message.type === 'insights') {
					this.pendingInsights.get(message.id)?.(message.insights);
					this.pendingInsights.delete(message.id);
				}
			};
			this.worker.onerror = (event) => reject(new Error(event.message || 'The compiler could not be started'));
		});
		this.send({ type: 'init', documentId, fonts });
	}

	private send(request: WorkerRequest) {
		this.worker.postMessage(request);
	}

	/** Adds or replaces files, and removes the given paths. */
	update(set: WorkerFile[], remove: string[] = []) {
		if (set.length || remove.length) this.send({ type: 'files', set, remove });
	}

	/** Compiles from `main`. With `full`, the answer is the whole document instead of a delta. */
	compile(main: string, full = false): Promise<CompileOutput> {
		const id = this.nextId++;
		return new Promise((resolve) => {
			this.pending.set(id, resolve);
			this.send({ type: 'compile', id, main, full });
		});
	}

	/** Page and word count of the last compiled document, and optionally its first page as SVG. */
	insights(thumbnail: boolean): Promise<DocumentInsights | undefined> {
		const id = this.nextId++;
		return new Promise((resolve) => {
			this.pendingInsights.set(id, resolve);
			this.send({ type: 'insights', id, thumbnail });
		});
	}

	dispose() {
		this.worker.terminate();
		for (const resolve of this.pending.values()) resolve({ ms: 0, diagnostics: [] });
		for (const resolve of this.pendingInsights.values()) resolve(undefined);
		this.pending.clear();
		this.pendingInsights.clear();
	}
}

/** Offset of a zero-based `line:column` position in `text`. */
function offsetOf(text: string, line: number, column: number): number {
	let offset = 0;
	for (let i = 0; i < line; i++) {
		const next = text.indexOf('\n', offset);
		if (next < 0) return text.length;
		offset = next + 1;
	}
	return Math.min(offset + column, text.length);
}

/**
 * Converts the compiler's diagnostics to the editor's: paths relative to the
 * document root and character offsets instead of `line:column` ranges.
 * `textOf` returns the current text of a file by its absolute path.
 */
export function toEditorDiagnostics(diagnostics: WorkerDiagnostic[], textOf: (path: string) => string | undefined): Diagnostic[] {
	return diagnostics.map((d) => {
		const result: Diagnostic = {
			message: d.package ? `${d.message} (in ${d.package})` : d.message,
			severity: d.severity === 'warning' ? 'Warning' : 'Error',
			// Errors inside packages have no place in the document's files.
			path: d.path && !d.package ? d.path.replace(/^\//, '') : null
		};
		const match = /^(\d+):(\d+)-(\d+):(\d+)$/.exec(d.range);
		const text = d.package ? undefined : textOf(d.path);
		if (match && text !== undefined) {
			result.from = offsetOf(text, Number(match[1]), Number(match[2]));
			result.to = offsetOf(text, Number(match[3]), Number(match[4]));
		}
		return result;
	});
}
