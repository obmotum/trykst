// Feasibility test: drive tinymist-web like the Trykst editor would.
// Results end up in window.__tinymist.
const sample = import.meta.glob('../../tools/screenshots/sample/**/*', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const ROOT = 'file:///project';
const files = new Map<string, string>(Object.entries(sample).map(([path, text]) => [`${ROOT}${path.replace('../../tools/screenshots/sample', '')}`, text]));
// A project package, as the server would deliver it.
files.set('file:///@packages/project/gruss/0.1.0/typst.toml', '[package]\nname = "gruss"\nversion = "0.1.0"\nentrypoint = "lib.typ"\n');
files.set('file:///@packages/project/gruss/0.1.0/lib.typ', '/// Greets the reader.\n#let gruss(name) = [Hallo #name]\n');

const out: Record<string, unknown> = { fsWatch: [] as unknown[], notifications: {} as Record<string, number>, serverRequests: [] as string[] };
const log = (key: string, value: unknown) => ((out as any)[key] = value);
const started = performance.now();
const ms = () => Math.round(performance.now() - started);

const worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
let nextId = 1;
const pending = new Map<number, (result: any) => void>();
const diagnostics = new Map<string, any[]>();
let workerReady: (v: unknown) => void;
const ready = new Promise((r) => (workerReady = r));

const request = (method: string, params: unknown) =>
	new Promise<any>((resolve) => {
		const id = nextId++;
		pending.set(id, resolve);
		worker.postMessage({ id, method, params });
	});
const notify = (method: string, params: unknown) => worker.postMessage({ method, params });
const b64 = (text: string) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));

worker.onmessage = (event) => {
	const m = event.data;
	if (m.method === '$/workerReady') return workerReady(m.params);
	if (m.method && m.id !== undefined) {
		// The server asks us something.
		(out.serverRequests as string[]).push(m.method);
		if (m.method === 'tinymist/fs/watch') {
			(out.fsWatch as unknown[]).push({ at: ms(), inserts: m.params.inserts, removes: m.params.removes });
			worker.postMessage({ id: m.id, result: null });
			// Hand over the files it wants, from what the page already has.
			const inserts = (m.params.inserts as string[]).map((uri) => {
				const text = files.get(uri);
				return { uri, content: text === undefined ? { type: 'err', error: 'not found' } : { type: 'ok', content: b64(text) } };
			});
			// No longer watched is not deleted: the files still exist, so nothing is reported as removed.
			void request('tinymist/fsChange', { inserts, removes: [], isSync: false });
			return;
		}
		if (m.method === 'workspace/configuration') return worker.postMessage({ id: m.id, result: (m.params.items ?? []).map(() => null) });
		return worker.postMessage({ id: m.id, result: null });
	}
	if (m.method) {
		const counts = out.notifications as Record<string, number>;
		counts[m.method] = (counts[m.method] ?? 0) + 1;
		if (m.method === 'textDocument/publishDiagnostics') diagnostics.set(m.params.uri, m.params.diagnostics);
		if (m.method === '$/workerError') log('workerError', m.params);
		if (m.method === '$/resolvePackage') ((out as any).packages ??= []).push(m.params);
		return;
	}
	if (m.id !== undefined) {
		pending.get(m.id)?.(m.error ? { error: m.error } : m.result);
		pending.delete(m.id);
	}
};

const sleep = (t: number) => new Promise((r) => setTimeout(r, t));
const labels = (completion: any) => (Array.isArray(completion) ? completion : completion?.items ?? []).map((i: any) => i.label);

(async () => {
	try {
		log('worker', await ready);
		let t = performance.now();
		const initResult = await request('initialize', {
			processId: null,
			rootUri: ROOT,
			workspaceFolders: [{ uri: ROOT, name: 'project' }],
			capabilities: { textDocument: { completion: { completionItem: { snippetSupport: true } }, hover: { contentFormat: ['markdown'] } } },
			initializationOptions: {}
		});
		notify('initialized', {});
		// Hand over every file of the document right away, so nothing is missing on the first compile.
		await request('tinymist/fsChange', {
			inserts: [...files].map(([uri, text]) => ({ uri, content: { type: 'ok', content: b64(text) } })),
			removes: [],
			isSync: false
		});
		log('initializeMs', Math.round(performance.now() - t));
		log('capabilities', Object.keys(initResult?.capabilities ?? {}).sort());

		const mainUri = `${ROOT}/main.typ`;
		const main = files.get(mainUri)!;
		notify('textDocument/didOpen', { textDocument: { uri: mainUri, languageId: 'typst', version: 1, text: main } });
		t = performance.now();
		for (let i = 0; i < 100 && !diagnostics.has(mainUri); i++) await sleep(100);
		log('firstDiagnosticsMs', Math.round(performance.now() - t));
		log('diagnosticsMain', (diagnostics.get(mainUri) ?? []).map((d) => `${d.range.start.line}:${d.range.start.character} ${d.message}`));

		// Completion of a function name, and of what template.typ exports.
		const line = main.split('\n').length;
		let text = main + '\n#ima';
		notify('textDocument/didChange', { textDocument: { uri: mainUri, version: 2 }, contentChanges: [{ text }] });
		t = performance.now();
		const completion = await request('textDocument/completion', { textDocument: { uri: mainUri }, position: { line, character: 4 } });
		log('completion', { ms: Math.round(performance.now() - t), count: labels(completion).length, sample: labels(completion).slice(0, 8), hasImage: labels(completion).includes('image') });

		// Hover over `paper`, a function defined in another file (template.typ).
		const paperLine = main.split('\n').findIndex((l) => l.includes('paper.with'));
		t = performance.now();
		const hover = await request('textDocument/hover', { textDocument: { uri: mainUri }, position: { line: paperLine, character: main.split('\n')[paperLine].indexOf('paper') + 2 } });
		log('hover', { ms: Math.round(performance.now() - t), text: JSON.stringify(hover?.contents ?? hover).slice(0, 240) });

		// Path completion inside an include.
		text = main + '\n#include "sections/';
		notify('textDocument/didChange', { textDocument: { uri: mainUri, version: 3 }, contentChanges: [{ text }] });
		const paths = await request('textDocument/completion', { textDocument: { uri: mainUri }, position: { line, character: 19 } });
		log('pathCompletion', labels(paths).slice(0, 10));

		// An error, a package from Typst Universe, a font.
		text = main + '\n#import "@project/gruss:0.1.0": gruss\n#gruss("Welt")\n#text(font: "Libertinus Serif")[x]\n#text(font: "Gibtsnicht Sans")[y]\n#unbekannt()\n';
		notify('textDocument/didChange', { textDocument: { uri: mainUri, version: 4 }, contentChanges: [{ text }] });
		diagnostics.delete(mainUri);
		for (let i = 0; i < 100 && !diagnostics.has(mainUri); i++) await sleep(100);
		await sleep(3000);
		// Hover over a function from the project package.
		const lines = text.split('\n');
		const gl = lines.findIndex((l) => l.startsWith('#gruss('));
		const pkgHover = await request('textDocument/hover', { textDocument: { uri: mainUri }, position: { line: gl, character: 3 } });
		log('packageHover', JSON.stringify(pkgHover?.contents ?? pkgHover).slice(0, 200));
		log('diagnosticsAfterChanges', (diagnostics.get(mainUri) ?? []).map((d) => `${d.range.start.line}:${d.range.start.character} ${d.severity} ${d.message}`.slice(0, 160)));
	} catch (e) {
		log('error', String((e as Error)?.stack ?? e));
	}
	log('totalMs', ms());
	(window as any).__tinymist = out;
	document.getElementById('status')!.textContent = 'done';
})();
