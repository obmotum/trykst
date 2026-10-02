// Takes the README screenshots (preview/dashboard.png, project.png, editor.png)
// of a running Trykst dev instance. See README.md in this folder.
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

// The sample paper: a document with folders, as { path, content } entries.
function readSample(dir, prefix = '') {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const rel = prefix + entry.name;
    const full = path.join(dir, entry.name);
    return entry.isDirectory() ? readSample(full, rel + '/') : [{ path: rel, content: fs.readFileSync(full, 'utf8') }];
  });
}
const SAMPLE = readSample(path.join(__dirname, 'sample'));
const NOTES = `#set page(paper: "a5", margin: 1.5cm)
#set text(size: 10pt)
= Weekly sync, 1 October
*Attendees:* Alice, Vanessa, Erik

== Decisions
- Release Trykst 0.2.0 with projects
- Add the partner team to the project as guests

== Next steps
+ Prepare the proposal for Partner AG
+ Review the paper outline
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
    (base) => document.title.includes('Projects') || !location.href.startsWith(base),
    { timeout: 15000 },
    BASE,
  );
  // Keycloak with Organizations asks for the username and the password separately.
  for (let step = 0; step < 3 && !page.url().startsWith(BASE); step++) {
    await page.waitForSelector('#kc-login', { timeout: 15000 }); // the IdP page may still be loading
    if (await page.$('#username:not([readonly])')) await page.type('#username', 'alice');
    if (await page.$('#password')) await page.type('#password', 'alice');
    await Promise.all([page.waitForNavigation({ waitUntil: 'networkidle2' }), page.click('#kc-login')]);
  }
  if (!page.url().startsWith(BASE)) throw new Error('Sign-in failed at ' + page.url());
  await page.waitForFunction(() => document.title.includes('Projects'), { timeout: 15000 });
}

// Creates three projects; returns the ids of the main project and its paper.
async function createDemoContent(page) {
  return page.evaluate(async ({ SAMPLE, NOTES, PROPOSAL }) => {
    const send = async (method, url, body) => {
      const res = await fetch(url, { method, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
      if (!res.ok) throw new Error(`${method} ${url}: ${res.status} ${await res.text()}`);
      return res.json();
    };
    const post = (url, body) => send('POST', url, body);
    if ((await fetch('/api/projects').then((r) => r.json())).length) {
      throw new Error('Use an empty database: the account already has projects');
    }

    const project = await post('/api/projects', {
      name: 'Research Group',
      description: 'Papers, proposals and meeting notes of the SSO research group',
    });
    const addDocument = async (projectId, title, content) => {
      const doc = await post(`/api/projects/${projectId}/documents`, { title, content });
      await post('/api/compile', { document_id: doc.id }); // renders the thumbnail
      return doc;
    };
    await addDocument(project.id, 'Meeting Notes', NOTES);
    await addDocument(project.id, 'Project Proposal', PROPOSAL);

    // The paper: main.typ exists already, everything else is created with its folders.
    const paper = await post(`/api/projects/${project.id}/documents`, { title: 'Research Paper' });
    const folders = new Map();
    const folderId = async (dir) => {
      if (!dir) return null;
      if (!folders.has(dir)) {
        const cut = dir.lastIndexOf('/');
        const parent_id = await folderId(cut < 0 ? '' : dir.slice(0, cut));
        const node = await post(`/api/documents/${paper.id}/nodes`, { parent_id, name: dir.slice(cut + 1), kind: 'folder' });
        folders.set(dir, node.id);
      }
      return folders.get(dir);
    };
    for (const file of SAMPLE) {
      if (file.path === 'main.typ') {
        await send('PATCH', `/api/documents/${paper.id}/nodes/${paper.entrypoint_id}`, { content: file.content });
        continue;
      }
      const cut = file.path.lastIndexOf('/');
      const parent_id = await folderId(cut < 0 ? '' : file.path.slice(0, cut));
      await post(`/api/documents/${paper.id}/nodes`, { parent_id, name: file.path.slice(cut + 1), kind: 'text', content: file.content });
    }
    const compiled = await post('/api/compile', { document_id: paper.id });
    if (compiled.errors) throw new Error('The sample paper does not compile: ' + JSON.stringify(compiled.errors));

    for (const [query, role] of [['vanessa', 'editor'], ['erik', 'viewer']]) {
      const people = await fetch(`/api/directory/search?q=${query}`).then((r) => r.json());
      if (people[0]?.subject) await post(`/api/projects/${project.id}/members`, { subject: people[0].subject, role });
    }

    const partner = await post('/api/projects', { name: 'Partner AG Onboarding', description: 'Shared with the partner team' });
    await addDocument(partner.id, 'Kick-off Agenda', NOTES);
    await post('/api/projects', { name: 'Templates', description: 'Letterheads and report templates' });

    return { projectId: project.id, paperId: paper.id };
  }, { SAMPLE, NOTES, PROPOSAL });
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
    const { projectId, paperId } = await createDemoContent(page);

    await page.goto(`${BASE}/dashboard`, { waitUntil: 'networkidle2' });
    await sleep(1500);
    await page.screenshot({ path: path.join(OUT, 'dashboard.png') });

    await page.goto(`${BASE}/project/${projectId}`, { waitUntil: 'networkidle2' });
    await sleep(1500);
    await page.screenshot({ path: path.join(OUT, 'project.png') });

    await page.goto(`${BASE}/doc/${paperId}`, { waitUntil: 'networkidle2' });
    await page.waitForSelector('.cm-content');
    await page.waitForSelector('.preview-container svg');
    await sleep(3000); // let the preview settle
    await page.mouse.click(5, VIEWPORT.height - 10); // move focus off the editor
    await sleep(500);
    await page.screenshot({ path: path.join(OUT, 'editor.png') });

    console.log(`Saved dashboard.png, project.png and editor.png to ${OUT}`);
  } finally {
    await stop();
  }
})().catch((e) => {
  console.error(e.message || e);
  process.exit(1);
});
