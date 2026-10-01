// Exercise the real link actions and keyboard listener without a Tauri runtime or app build.
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { runInNewContext } from 'node:vm';
import { parseYtLink } from './ytlink.ts';

function source(file: string): string {
	return stripTypeScriptTypes(
		readFileSync(new URL(file, import.meta.url), 'utf8')
			.replace(/^import[\s\S]*?;\n/gm, '')
			.replace(/^export /gm, '')
	);
}

// Real parsing followed by the shared action: songs resolve via radio, pages navigate once.
const played: string[][] = [];
const navigated: string[] = [];
const open = runInNewContext(`${source('./browse.ts')}\nopenLinkTarget`, {
	api: { LOCAL_ARTIST_PREFIX: 'local-artist:' },
	startRadio: (...args: string[]) => played.push(args),
	goto: (href: string) => navigated.push(href)
}) as (target: NonNullable<ReturnType<typeof parseYtLink>>) => void;

for (const url of [
	' https://youtu.be/track123?si=shared ',
	'https://music.youtube.com/watch?v=track456&list=PLexample'
]) {
	const target = parseYtLink(url);
	assert.ok(target);
	open(target);
}
assert.deepEqual(played, [['song', 'track123'], ['song', 'track456']]);
assert.deepEqual(navigated, []);

for (const [url, href] of [
	['music.youtube.com/playlist?list=PLexample', '/playlist/VLPLexample'],
	['https://music.youtube.com/browse/MPREalbum', '/album/MPREalbum'],
	['https://music.youtube.com/channel/UCartist', '/artist/UCartist'],
	['https://music.youtube.com/playlist?list=PL%2Fexample', '/playlist/VLPL%2Fexample']
]) {
	const target = parseYtLink(url);
	assert.ok(target);
	open(target);
	assert.equal(navigated.at(-1), href);
}
assert.equal(played.length, 2);
assert.equal(navigated.length, 4);

// Capture the actual app listener, with just its browser and player dependencies replaced.
for (const platform of ['Linux', 'MacIntel']) {
	let listener: ((event: KeyboardEvent) => void) | undefined;
	let focused = 0;
	let selected = 0;
	const input = { focus: () => focused++, select: () => selected++ };
	let pageInput: typeof input | null = input;
	let dialog: { querySelector: () => typeof input | null } | null = null;
	const ui = { linkOpen: false, paletteOpen: false };
	const { initShortcuts, combo } = runInNewContext(
		`${source('./shortcuts.ts')}\n({ initShortcuts, combo: OPEN_LINK_COMBO })`,
		{
			browser: true,
			navigator: { platform },
			ui,
			document: {
				querySelector: (selector: string) => selector === '[data-link-input]' ? pageInput : dialog
			},
			window: {
				addEventListener: (_name: string, fn: typeof listener) => (listener = fn),
				removeEventListener: (_name: string, fn: typeof listener) => {
					assert.equal(fn, listener);
					listener = undefined;
				}
			}
		}
	) as { initShortcuts: (mini?: boolean) => () => void; combo: string };
	assert.equal(combo, platform === 'MacIntel' ? '⌘L' : 'Ctrl+L');
	let stop = initShortcuts();
	function press(overrides: Partial<KeyboardEvent> = {}) {
		let prevented = false;
		assert.ok(listener);
		listener({
			key: 'l', ctrlKey: platform !== 'MacIntel', metaKey: platform === 'MacIntel',
			altKey: false, shiftKey: false, defaultPrevented: false,
			preventDefault: () => (prevented = true), ...overrides
		} as KeyboardEvent);
		return prevented;
	}
	assert.equal(press(), true);
	assert.equal(focused, 1);
	assert.equal(selected, 1);
	assert.equal(ui.linkOpen, false);
	assert.equal(press({ key: 'L' }), true);
	assert.equal(selected, 2);
	assert.equal(press({ altKey: true }), false); // leaves global hotkeys / AltGr alone
	assert.equal(press({ defaultPrevented: true }), false);
	assert.equal(press({ key: 'x', ctrlKey: false, metaKey: false }), false);
	assert.equal(selected, 2);
	pageInput = null;
	assert.equal(press(), true);
	assert.equal(ui.linkOpen, true); // a route without the inline field
	ui.linkOpen = false;
	pageInput = input;
	dialog = { querySelector: () => null };
	assert.equal(press(), false); // cannot steal focus from another dialog
	assert.equal(selected, 2);
	assert.equal(ui.linkOpen, false);
	dialog = { querySelector: () => input };
	assert.equal(press(), true); // select the URL in the already open link dialog
	assert.equal(selected, 3);
	stop();
	assert.equal(listener, undefined);
	stop = initShortcuts(true);
	assert.equal(press(), false); // no URL UI in the mini player
	assert.equal(selected, 3);
	stop();
}

console.log('ok');
