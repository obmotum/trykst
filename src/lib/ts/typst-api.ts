export interface Diagnostic {
	message: string;
	severity: string;
	/** File the range refers to, relative to the document root. */
	path?: string | null;
	from?: number;
	to?: number;
}

/** A page of the live preview; `svg` is left out when the client already has this hash. */
export interface PreviewPage {
	hash: string;
	svg?: string;
}

export interface CompileResponse {
	pages: PreviewPage[] | null;
	errors: Diagnostic[] | null;
	stats?: { pages: number; words: number; characters: number; characters_excluding_spaces: number };
}

/**
 * Compiles a document from its entrypoint. `files` holds unsaved text by path
 * and takes precedence over what the server has stored. `known` lists the
 * hashes of the pages already shown; the server sends only the other pages.
 */
export async function compileDocument(
	document_id: string,
	files: Record<string, string>,
	known: string[] = []
): Promise<CompileResponse> {
	const res = await fetch('/api/compile', {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ document_id, files, known })
	});
	if (!res.ok) throw new Error((await res.text()) || 'Compilation failed');
	return await res.json();
}

export async function renderDocument(
	document_id: string,
	files: Record<string, string>,
	format: 'pdf' | 'png' | 'svg'
): Promise<Blob> {
	const res = await fetch(`/api/export/${format}`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ document_id, files })
	});
	if (!res.ok) throw new Error((await res.text()) || 'Export failed');
	return res.blob();
}

export function downloadBlob(blob: Blob, filename: string) {
	const url = URL.createObjectURL(blob);
	const a = document.createElement('a');
	a.href = url;
	a.download = filename;
	a.click();
	URL.revokeObjectURL(url);
}

export async function exportDocument(
	document_id: string,
	files: Record<string, string>,
	format: 'pdf' | 'png' | 'svg',
	title: string = 'document'
) {
	downloadBlob(await renderDocument(document_id, files, format), `${title}.${format}`);
}
