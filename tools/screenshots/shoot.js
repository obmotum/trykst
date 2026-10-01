// Takes the README screenshots (preview/dashboard.png, preview/editor.png) of a
// running Trykst dev instance. See README.md in this folder.
//
//   node shoot.js [output dir]     (default: ../../preview)
//
// Environment:
//   TRYKST_URL   base URL of the instance       (default http://localhost:3000)
//   BROWSER      path to Edge/Chrome/Chromium   (default: auto-detect)
const { spawn } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');
const puppeteer = require('puppeteer-core');

const BASE = process.env.TRYKST_URL || 'http://localhost:3000';
const OUT = path.resolve(process.argv[2] || path.join(__dirname, '..', '..', 'preview'));
const PORT = 9333;
const VIEWPORT = { width: 1900, height: 950, deviceScaleFactor: 1 };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function findBrowser() {
  const candidates = [
    process.env.BROWSER,
    'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
    'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
    'C:/Program Files/Google/Chrome/Application/chrome.exe',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
    '/usr/bin/google-chrome',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
    '/usr/bin/microsoft-edge',
  ];
  const found = candidates.find((p) => p && fs.existsSync(p));
  if (!found) throw new Error('No Chromium-based browser found; set BROWSER');
  return found;
}

// Started by hand with a debugging port: puppeteer's default pipe transport
// does not work with every Edge installation.
async function startBrowser() {
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'trykst-shots-'));
  const child = spawn(findBrowser(), [
    '--headless=new',
    `--remote-debugging-port=${PORT}`,
    `--user-data-dir=${profile}`,
    '--no-first-run',
    '--no-default-browser-check',
    'about:blank',
  ], { stdio: 'ignore' });
  for (let i = 0; i < 50; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${PORT}/json/version`);
      if (res.ok) break;
    } catch {}
    await sleep(200);
  }
  const browser = await puppeteer.connect({ browserURL: `http://127.0.0.1:${PORT}`, defaultViewport: VIEWPORT });
  const exited = new Promise((resolve) => child.once('exit', resolve));
  const stop = async () => {
    try { await browser.close(); } catch {}
    child.kill();
    // The browser keeps files in its profile locked for a moment after exiting.
    await Promise.race([exited, sleep(5000)]);
    try { fs.rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 }); } catch {}
  };
  return { browser, stop };
}

const PAPER = fs.readFileSync(path.join(__dirname, 'paper.typ'), 'utf8');
const NOTES = `#set page(paper: "a5", margin: 1.5cm)
#set text(size: 10pt)
= Weekly sync, 1 October
*Attendees:* Alice, Vanessa, Erik

== Decisions
- Release Trykst 0.1.2 with profile pictures
- Invite the partner team as guests

== Next steps
+ Prepare the proposal for Partner AG
+ Review the thesis outline
`;
const PROPOSAL = `#set page(paper: "a4", margin: 2cm)
#set text(size: 11pt)
#align(center, text(18pt, weight: "bold")[Project Proposal])
#align(center)[Partner AG · Collaborative documentation]
#v(1em)
= Scope
#lorem(60)
= Timeline
#table(columns: 2, [Phase 1], [Kick-off], [Phase 2], [Pilot], [Phase 3], [Roll-out])
`;

async function signIn(page) {
  await page.goto(`${BASE}/dashboard`, { waitUntil: 'networkidle2' });
  // Trykst redirects to the IdP from JavaScript once the page has loaded.
  await page.waitForFunction(
    (base) => document.title.includes('Dashboard') || !location.href.startsWith(base),
    { timeout: 15000 },
    BASE,
  );
  // Keycloak with Organizations asks for the username and the password separately.
  for (let step = 0; step < 3 && !page.url().startsWith(BASE); step++) {
    if (await page.$('#username:not([readonly])')) await page.type('#username', 'alice');
    if (await page.$('#password')) await page.type('#password', 'alice');
    await Promise.all([page.waitForNavigation({ waitUntil: 'networkidle2' }), page.click('#kc-login')]);
  }
  if (!page.url().startsWith(BASE)) throw new Error('Sign-in failed at ' + page.url());
  await page.waitForFunction(() => document.title.includes('Dashboard'), { timeout: 15000 });
}

async function createDemoContent(page) {
  return page.evaluate(async ({ PAPER, NOTES, PROPOSAL }) => {
    const post = (u, b) => fetch(u, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(b) }).then((r) => r.json());
    const existing = await fetch('/api/docs').then((r) => r.json());
    if (existing.length) throw new Error('Use an empty database: the account already has documents');
    await post('/api/folders', { name: 'Projects' });
    await post('/api/folders', { name: 'Templates' });
    const ids = [];
    for (const [title, content] of [['Meeting Notes', NOTES], ['Project Proposal', PROPOSAL], ['Research Paper', PAPER]]) {
      const doc = await post('/api/docs', { title, content });
      await post('/api/compile', { text: content, document_id: doc.id }); // renders the dashboard thumbnail
      ids.push(doc.id);
    }
    await post('/api/spaces', { name: 'Thesis' });
    const people = await fetch('/api/directory/search?q=vanessa').then((r) => r.json());
    if (people[0]?.subject) await post(`/api/docs/${ids[2]}/invite`, { subject: people[0].subject, role: 'editor' });
    return ids;
  }, { PAPER, NOTES, PROPOSAL });
}

(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  const { browser, stop } = await startBrowser();
  try {
    const page = await browser.newPage();
    await page.evaluateOnNewDocument((base) => {
      if (location.origin === base) {
        localStorage.setItem('editor-theme', 'Catppuccin');
        localStorage.setItem('editor-dark-mode', 'true');
      }
    }, new URL(BASE).origin);

    await signIn(page);
    const ids = await createDemoContent(page);

    await page.goto(`${BASE}/dashboard`, { waitUntil: 'networkidle2' });
    await sleep(1500);
    await page.screenshot({ path: path.join(OUT, 'dashboard.png') });

    await page.goto(`${BASE}/doc/${ids[2]}`, { waitUntil: 'networkidle2' });
    await page.waitForSelector('.cm-content');
    await sleep(4000); // compile and render the preview
    await page.mouse.click(5, VIEWPORT.height - 10); // move focus off the editor
    await sleep(500);
    await page.screenshot({ path: path.join(OUT, 'editor.png') });

    console.log(`Saved dashboard.png and editor.png to ${OUT}`);
  } finally {
    await stop();
  }
})().catch((e) => {
  console.error(e.message || e);
  process.exit(1);
});
