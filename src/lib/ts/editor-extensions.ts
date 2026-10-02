import { RangeSetBuilder, countColumn, type EditorState, type Extension, type Text } from '@codemirror/state';
import { Decoration, EditorView, ViewPlugin, keymap, type DecorationSet, type ViewUpdate } from '@codemirror/view';
import { codeFolding, foldGutter, foldKeymap, foldService } from '@codemirror/language';

const HEADING = /^(=+)\s/;
const BLANK = -1;

/** Per line (1-based): heading level (0 = none) and indentation (BLANK for empty lines). */
interface Outline {
	heading: Uint8Array;
	indent: Int32Array;
}

// The fold gutter asks about every visible line after every change. One pass
// per document version answers all of them, instead of rescanning the text
// below each line.
const outlines = new WeakMap<Text, Outline>();

function outlineOf(state: EditorState): Outline {
	const doc = state.doc;
	let outline = outlines.get(doc);
	if (!outline) {
		outline = { heading: new Uint8Array(doc.lines + 1), indent: new Int32Array(doc.lines + 1) };
		let n = 1;
		for (const text of doc.iterLines()) {
			const start = text.search(/\S/);
			outline.indent[n] = start < 0 ? BLANK : countColumn(text, state.tabSize, start);
			const heading = start === 0 ? HEADING.exec(text) : null;
			outline.heading[n] = heading ? Math.min(heading[1].length, 255) : 0;
			n++;
		}
		outlines.set(doc, outline);
	}
	return outline;
}

/**
 * What a line folds: a heading folds its section (up to the next heading of the
 * same or a higher level), any other line folds the more deeply indented lines
 * below it. Works for Typst and for the other text files alike.
 */
function foldRange(state: EditorState, lineStart: number): { from: number; to: number } | null {
	const doc = state.doc;
	const { heading, indent } = outlineOf(state);
	const first = doc.lineAt(lineStart).number;
	if (indent[first] === BLANK) return null;

	let end = first;
	if (heading[first]) {
		for (let n = first + 1; n <= doc.lines; n++) {
			if (heading[n] && heading[n] <= heading[first]) break;
			if (indent[n] !== BLANK) end = n;
		}
	} else {
		for (let n = first + 1; n <= doc.lines; n++) {
			if (indent[n] === BLANK) continue;
			if (indent[n] <= indent[first]) break;
			end = n;
		}
	}
	return end > first ? { from: doc.line(first).to, to: doc.line(end).to } : null;
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
