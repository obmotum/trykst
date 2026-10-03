// Feasibility test: tinymist's language server, built for the web, in a Web Worker.
// It speaks LSP with the page through postMessage, as vscode.dev does.
// @ts-ignore: the web build ships its own types next to it
import init, { TinymistLanguageServer } from './vendor/tinymist.js';
import wasmUrl from './vendor/tinymist_bg.wasm?url';

type Message = { id?: number; method?: string; params?: unknown; result?: unknown; error?: unknown };

const post = (message: Message) => (self as unknown as Worker).postMessage({ jsonrpc: '2.0', ...message });

let bridge: any;
const events: unknown[] = [];
/** Server events are processed after each message, like the vscode.dev worker does. */
function flush<T>(value: T): T {
	while (events.length) for (const event of events.splice(0)) bridge.on_event(event);
	return value;
}

const ready = (async () => {
	const started = performance.now();
	await init({ module_or_path: wasmUrl });
	bridge = new TinymistLanguageServer({
		sendEvent: (event: unknown) => void events.push(event),
		// Requests from the server to the client (us): answered by the page.
		sendRequest: ({ id, method, params }: Message) => post({ id: -Number(id), method, params }),
		sendNotification: ({ method, params }: Message) => post({ method, params }),
		// Packages: answer with a directory; its files are then asked for like any other file.
		resolveFn: (spec: { namespace: string; name: string; version: string }) => {
			post({ method: '$/resolvePackage', params: spec });
			return `/@packages/${spec.namespace}/${spec.name}/${spec.version}`;
		}
	});
	post({ method: '$/workerReady', params: { ms: Math.round(performance.now() - started), version: TinymistLanguageServer.version() } });
})();

self.onmessage = async (event: MessageEvent<Message>) => {
	await ready;
	const message = event.data;
	try {
		if (message.method && message.id !== undefined) {
			// A request from the client.
			const result = await flush(bridge.on_request(message.method, message.params ?? null));
			flush(undefined);
			post({ id: message.id, result: result ?? null });
		} else if (message.method) {
			flush(bridge.on_notification(message.method, message.params ?? null));
		} else if (message.id !== undefined) {
			// The client's answer to one of the server's requests (ids were negated on the way out).
			flush(bridge.on_response({ id: -message.id, result: message.result ?? null, error: message.error }));
		}
	} catch (error) {
		if (message.id !== undefined && message.method) post({ id: message.id, error: { code: -32603, message: String(error) } });
		else post({ method: '$/workerError', params: String((error as Error)?.stack ?? error) });
	}
};
