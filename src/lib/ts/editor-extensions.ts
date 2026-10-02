import { RangeSetBuilder, countColumn, type EditorState, type Extension } from '@codemirror/state';
import { Decoration, EditorView, ViewPlugin, keymap, type DecorationSet, type ViewUpdate } from '@codemirror/view';
import { codeFolding, foldGutter, foldKeymap, foldService } from '@codemirror/language';

const HEADING = /^(=+)\s/;

function indentOf(state: EditorState, text: string): number {
	return countColumn(text, state.tabSize, text.search(/\S|$/));
}

/**
 * What a line folds: a heading folds its section (up to the next heading of the
 * same or a higher level), any other line folds the more deeply indented lines
 * below it. Works for Typst and for the other text files alike.
 */
function foldRange(state: EditorState, lineStart: number): { from: number; to: number } | null {
	const doc = state.doc;
	const line = doc.lineAt(lineStart);
	if (!line.text.trim()) return null;

	let end = line.number;
	const heading = HEADING.exec(line.text);
	if (heading) {
		for (let n = line.number + 1; n <= doc.lines; n++) {
			const text = doc.line(n).text;
			const other = HEADING.exec(text);
			if (other && other[1].length <= heading[1].length) break;
			if (text.trim()) end = n;
		}
	} else {
		const indent = indentOf(state, line.text);
		for (let n = line.number + 1; n <= doc.lines; n++) {
			const text = doc.line(n).text;
			if (!text.trim()) continue;
			if (indentOf(state, text) <= indent) break;
			end = n;
		}
	}
	return end > line.number ? { from: line.to, to: doc.line(end).to } : null;
}

/** Fold markers next to the line numbers; Ctrl+Shift+[ and ] fold and unfold. */
export function folding(): Extension {
	return [
		codeFolding({ placeholderText: '⋯' }),
		foldService.of((state, lineStart) => foldRange(state, lineStart)),
		foldGutter({ openText: '⌄', closedText: '›' }),
		keymap.of(foldKeymap)
	];
}

// The text of a list item starts after its marker; continuation lines align with it.
const LIST_MARKER = /^[ \t]*(?:[-+]|\d+\.)[ \t]+/;
const LEADING_SPACE = /^[ \t]*/;

function wrapDecorations(view: EditorView): DecorationSet {
	const builder = new RangeSetBuilder<Decoration>();
	const { doc, tabSize } = view.state;
	for (const { from, to } of view.visibleRanges) {
		for (let pos = from; pos <= to; ) {
			const line = doc.lineAt(pos);
			const prefix = (LIST_MARKER.exec(line.text) ?? LEADING_SPACE.exec(line.text)!)[0];
			const columns = countColumn(prefix, tabSize);
			if (columns > 0) {
				builder.add(
					line.from,
					line.from,
					Decoration.line({
						// The first row is pulled back by what the continuation rows are pushed in.
						attributes: { style: `text-indent: -${columns}ch; padding-left: calc(${columns}ch + 6px);` }
					})
				);
			}
			pos = line.to + 1;
		}
	}
	return builder.finish();
}

/**
 * Wrapped lines keep the indentation of the line they belong to, and the
 * wrapped text of a list item lines up with its first word instead of
 * starting at the left edge.
 */
export function indentedWrapping(): Extension {
	return [
		EditorView.lineWrapping,
		ViewPlugin.fromClass(
			class {
				decorations: DecorationSet;
				constructor(view: EditorView) {
					this.decorations = wrapDecorations(view);
				}
				update(update: ViewUpdate) {
					if (update.docChanged || update.viewportChanged) this.decorations = wrapDecorations(update.view);
				}
			},
			{ decorations: (plugin) => plugin.decorations }
		)
	];
}
