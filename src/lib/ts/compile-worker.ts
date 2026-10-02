/// <reference lib="webworker" />
// The Typst compiler for the live preview, running in a Web Worker so that
// compiling never blocks typing. The page sends the files of the document and
// asks for compilations; the worker answers with the changes to the rendered
// document (a full artifact first, small deltas afterwards) and diagnostics.
import { createTypstCompiler, FetchPackageRegistry, MemoryAccessModel, initOptions } from '@myriaddreamin/typst.ts';
import type { TypstCompiler } from '@myriaddreamin/typst.ts';
import type { IncrementalServer } from '@myriaddreamin/typst.ts/compiler';
import type { PackageResolveContext, PackageSpec } from '@myriaddreamin/typst.ts/internal.types';
import compilerWasm from '@myriaddreamin/typst-ts-web-compiler/pkg/typst_ts_web_compiler_bg.wasm?url';

export interface WorkerFile {
	/** Absolute path inside the document, e.g. `/chapters/intro.typ`. */
	path: string;
	text?: string;
	bytes?: Uint8Array;
}

export interface WorkerDiagnostic {
	path: string;
	/** `line:column-line:column`, zero-based. */
	range: string;
	severity: string;
	message: string;
	package: string;
}

export type WorkerRequest =
	| { type: 'init'; documentId: string; fonts: Uint8Array[] }
	| { type: 'files'; set: WorkerFile[]; remove: string[] }
	| { type: 'compile'; id: number; main: string; full: boolean };

export type WorkerResponse =
	| { type: 'ready' }
	| { type: 'failed'; message: string }
	| {
			type: 'compiled';
			id: number;
			ms: number;
			/** `reset` replaces the rendered document, `merge` updates it; absent when compiling failed. */
			action?: 'reset' | 'merge';
			data?: Uint8Array;
			diagnostics: WorkerDiagnostic[];
	  };

let compiler: TypstCompiler;
let incremental: IncrementalServer;
/** False until the page has received a full artifact that deltas can build on. */
let hasBase = false;

const post = (message: WorkerResponse, transfer: Transferable[] = []) =>
	(self as DedicatedWorkerGlobalScope).postMessage(message, transfer);

/**
 * Packages: `@preview/...` from Typst Universe, and Trykst's own `@project/...`
 * and `@trykst/...` from the server, which checks access through the document.
 */
class PackageRegistry extends FetchPackageRegistry {
	private own = new Map<string, string>();

	constructor(
		private model: MemoryAccessModel,
		private documentId: string
	) {
		super(model);
	}

	resolve(spec: PackageSpec, context: PackageResolveContext): string | undefined {
		if (!['project', 'trykst', 'typstdrive'].includes(spec.namespace)) return super.resolve(spec, context);

		const key = `${spec.namespace}/${spec.name}/${spec.version}`;
		const known = this.own.get(key);
		if (known) return known; // versions are immutable

		// The compiler asks synchronously; a worker may block on a request.
		const request = new XMLHttpRequest();
		const parts = [spec.namespace, spec.name, spec.version].map(encodeURIComponent).join('/');
		request.open('GET', `/api/documents/${this.documentId}/packages/${parts}`, false);
		request.send(null);
		if (request.status !== 200) return undefined; // not cached: it may be published later

		const directory = `/@memory/trykst/${key}`;
		for (const file of JSON.parse(request.responseText) as { path: string; data: string }[]) {
			const bytes = Uint8Array.from(atob(file.data), (c) => c.charCodeAt(0));
			this.model.insertFile(`${directory}/${file.path}`, bytes, new Date());
		}
		this.own.set(key, directory);
		return directory;
	}
}

async function init(documentId: string, fonts: Uint8Array[]) {
	compiler = createTypstCompiler();
	const model = new MemoryAccessModel();
	// The same fonts the server exports with, from the server itself.
	const builtIn: string[] = await (await fetch('/api/fonts/default')).json();
	await compiler.init({
		getModule: () => compilerWasm,
		beforeBuild: [
			initOptions.loadFonts([...builtIn, ...fonts], { assets: false }),
			initOptions.withAccessModel(model),
			initOptions.withPackageRegistry(new PackageRegistry(model, documentId))
		]
	});
	// The incremental server lives as long as the worker: the callback never returns.
	await new Promise<void>((ready) => {
		void compiler.withIncrementalServer((server) => {
			incremental = server;
			ready();
			return new Promise<void>(() => {});
		});
	});
}

async function compile(id: number, main: string, full: boolean) {
	const started = performance.now();
	if (full || !hasBase) {
		incremental.reset();
		hasBase = false;
	}
	const output = (await compiler.compile({
		mainFilePath: main,
		format: 'vector',
		diagnostics: 'full',
		incrementalServer: incremental
	} as never)) as { result?: Uint8Array; diagnostics?: WorkerDiagnostic[] };

	const response: WorkerResponse = {
		type: 'compiled',
		id,
		ms: performance.now() - started,
		diagnostics: output.diagnostics ?? []
	};
	if (output.result) {
		response.action = hasBase ? 'merge' : 'reset';
		response.data = output.result;
		hasBase = true;
	}
	post(response, response.data ? [response.data.buffer] : []);
}

// Requests are handled strictly one after the other.
let queue: Promise<unknown> = Promise.resolve();

self.onmessage = (event: MessageEvent<WorkerRequest>) => {
	const request = event.data;
	queue = queue
		.then(async () => {
			switch (request.type) {
				case 'init':
					await init(request.documentId, request.fonts);
					post({ type: 'ready' });
					break;
				case 'files':
					for (const path of request.remove) compiler.unmapShadow(path);
					for (const file of request.set) {
						if (file.text !== undefined) compiler.addSource(file.path, file.text);
						else if (file.bytes) compiler.mapShadow(file.path, file.bytes);
					}
					break;
				case 'compile':
					await compile(request.id, request.main, request.full);
					break;
			}
		})
		.catch((error) => {
			const message = String((error as Error)?.message ?? error);
			if (request.type === 'compile') {
				post({ type: 'compiled', id: request.id, ms: 0, diagnostics: [{ path: '', range: '', severity: 'error', message, package: '' }] });
			} else {
				post({ type: 'failed', message });
			}
		});
};
