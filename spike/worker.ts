// Feasibility test: the Typst compiler running in a Web Worker.
import { createTypstCompiler, MemoryAccessModel, FetchPackageRegistry, initOptions } from '@myriaddreamin/typst.ts';
import compilerWasm from '@myriaddreamin/typst-ts-web-compiler/pkg/typst_ts_web_compiler_bg.wasm?url';

const sample = import.meta.glob('../tools/screenshots/sample/**/*', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

/** `@preview` from Typst Universe as usual; `@project` served from memory, as Trykst packages would be. */
class TrykstRegistry extends FetchPackageRegistry {
	constructor(private model: MemoryAccessModel) {
		super(model);
	}
	resolve(spec: any, context: any): string | undefined {
		if (spec.namespace === 'project' && spec.name === 'gruss') {
			const dir = `/@memory/trykst/${spec.namespace}/${spec.name}/${spec.version}`;
			const enc = new TextEncoder();
			this.model.insertFile(`${dir}/typst.toml`, enc.encode(`[package]\nname = "gruss"\nversion = "${spec.version}"\nentrypoint = "lib.typ"\n`), new Date());
			this.model.insertFile(`${dir}/lib.typ`, enc.encode('#let gruss = [Hallo aus dem Projektpaket]\n'), new Date());
			return dir;
		}
		return super.resolve(spec, context);
	}
}

function longDocument(): string {
	let content = '= Richtlinien\n\n';
	for (let s = 1; s <= 40; s++) {
		content += `== Abschnitt ${s}\n`;
		for (let i = 1; i <= 25; i++) {
			content += `    - *Konten: Lokale Kontenverwendung von leeren Kennwörtern auf Konsolenanmeldung beschränken ${s}.${i}* -- Aktiviert (In Windows Server 2025 nicht mehr erforderlich, Microsoft hat diese Legacy Richtlinie entfernt)\n`;
		}
		content += '\n';
	}
	return content;
}

const ms = (from: number) => Math.round((performance.now() - from) * 10) / 10;
const summary = (r: any) => ({ bytes: r.result?.length ?? 0, diagnostics: (r.diagnostics ?? []).map((d: any) => `${d.path}:${d.range} ${d.severity} ${d.message}`).slice(0, 3) });

self.onmessage = async () => {
	const out: Record<string, unknown> = {};
	try {
		let t = performance.now();
		const compiler = createTypstCompiler();
		const model = new MemoryAccessModel();
		await compiler.init({
			getModule: () => compilerWasm,
			beforeBuild: [initOptions.withAccessModel(model), initOptions.withPackageRegistry(new TrykstRegistry(model))]
		});
		out.initMs = ms(t); // includes downloading the compiler and the default fonts

		// 1. The multi-file sample paper (template, sections in folders, bibliography, SVG figure)
		for (const [file, text] of Object.entries(sample)) {
			const path = file.replace('../tools/screenshots/sample', '');
			if (path.endsWith('.svg')) compiler.mapShadow(path, new TextEncoder().encode(text));
			else compiler.addSource(path, text);
		}
		t = performance.now();
		const paper = await compiler.compile({ mainFilePath: '/main.typ', format: 'vector', diagnostics: 'full' } as any);
		out.samplePaper = { ms: ms(t), ...summary(paper) };

		// 2. Packages: one from Typst Universe, one served by us
		compiler.addSource('/pkg.typ', '#import "@preview/example:0.1.0": add\n#import "@project/gruss:0.1.0": gruss\n#gruss #add(1, 2)\n');
		t = performance.now();
		const pkg = await compiler.compile({ mainFilePath: '/pkg.typ', format: 'vector', diagnostics: 'full' } as any);
		out.packages = { ms: ms(t), ...summary(pkg) };

		// 3. An error, to see what the diagnostics look like
		compiler.addSource('/broken.typ', '= Titel\n#include "gibt-es-nicht.typ"\n#unbekannt()\n');
		const broken = await compiler.compile({ mainFilePath: '/broken.typ', format: 'vector', diagnostics: 'full' } as any);
		out.errorDiagnostics = broken.diagnostics;

		// 4. The long document: first compile, then one small edit at a time
		let text = longDocument();
		compiler.addSource('/long.typ', text);
		t = performance.now();
		const long = await compiler.compile({ mainFilePath: '/long.typ', format: 'vector', diagnostics: 'full' } as any);
		out.longFirst = { ms: ms(t), ...summary(long) };

		const edits: number[] = [];
		const at = text.indexOf('1.3*');
		for (let i = 0; i < 10; i++) {
			text = text.slice(0, at) + 'x' + text.slice(at);
			compiler.addSource('/long.typ', text);
			t = performance.now();
			await compiler.compile({ mainFilePath: '/long.typ', format: 'vector', diagnostics: 'none' } as any);
			edits.push(ms(t));
		}
		out.longEditsMs = edits;

		// 5. Incremental output: only what changed is handed to the renderer
		await compiler.withIncrementalServer(async (server) => {
			t = performance.now();
			const first = await compiler.compile({ mainFilePath: '/long.typ', format: 'vector', diagnostics: 'none', incrementalServer: server } as any);
			const firstBytes = first.result?.length ?? 0;
			const firstMs = ms(t);
			const deltas: { ms: number; bytes: number }[] = [];
			for (let i = 0; i < 5; i++) {
				text = text.slice(0, at) + 'y' + text.slice(at);
				compiler.addSource('/long.typ', text);
				t = performance.now();
				const delta = await compiler.compile({ mainFilePath: '/long.typ', format: 'vector', diagnostics: 'none', incrementalServer: server } as any);
				deltas.push({ ms: ms(t), bytes: delta.result?.length ?? 0 });
			}
			out.incremental = { firstMs, firstBytes, deltas };
		});

		(self as any).postMessage({ out, paper: paper.result, long: long.result }, [paper.result!.buffer, long.result!.buffer]);
	} catch (e) {
		out.error = String((e as any)?.stack ?? e);
		(self as any).postMessage({ out });
	}
};
