/* Capture the real frontend with a demo IPC bridge; no host clipboard access. */
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const { spawn } = require('node:child_process');

async function main() {
  const root = path.resolve(__dirname, '..');
  const chrome = process.env.CLIPFORGE_CHROME || 'C:/Program Files/Google/Chrome/Application/chrome.exe';
  const profile = await fs.mkdtemp(path.join(os.tmpdir(), 'clipforge-docs-'));
  const browser = spawn(chrome, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check', 'about:blank'], { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] });
  let socket;
  try {
    const endpoint = await new Promise((resolve, reject) => {
      let output = '';
      const timer = setTimeout(() => reject(new Error('Chrome startup timed out')), 20000);
      browser.stderr.on('data', chunk => {
        output += chunk;
        const match = output.match(/DevTools listening on (ws:\/\/[^\s]+)/);
        if (match) { clearTimeout(timer); resolve(match[1]); }
      });
      browser.once('error', reject);
      browser.once('exit', code => { clearTimeout(timer); reject(new Error(`Chrome exited: ${code}`)); });
    });
    socket = new WebSocket(endpoint);
    await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', reject, { once: true }); });
    let id = 0;
    const pending = new Map();
    socket.addEventListener('message', event => {
      const message = JSON.parse(event.data);
      const callback = pending.get(message.id);
      if (callback) { pending.delete(message.id); message.error ? callback.reject(new Error(message.error.message)) : callback.resolve(message.result); }
    });
    const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
      const requestId = ++id;
      pending.set(requestId, { resolve, reject });
      socket.send(JSON.stringify({ id: requestId, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
    const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
    const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
    const call = (method, params) => send(method, params, sessionId);
    await call('Page.enable');
    await call('Runtime.enable');
    await call('Emulation.setDeviceMetricsOverride', { width: 840, height: 650, deviceScaleFactor: 2, mobile: false });
    await call('Page.addScriptToEvaluateOnNewDocument', { source: `
      const demo = {
        hotkeys: { menu: 'Ctrl+Alt+Space', copy: 'Ctrl+Alt+C', paste: 'Ctrl+Alt+V' },
        registers: Array(26).fill(null), registerNames: Array(26).fill(''),
        currentClipboard: 'ssh dev@lab-host\\nConnect to the development VM',
        history: ['ssh dev@lab-host\\nConnect to the development VM', 'cargo test --locked --all-targets', 'Thanks for the update!\\nI will review the changes this afternoon.', 'https://github.com/Pline-0x8/ClipForge', 'git status --short'],
        status: 'Ready', copy: false, visible: true, selection: null
      };
      demo.registers[0] = 'Thanks for the update!\\nI will review the changes this afternoon.';
      demo.registerNames[0] = 'Quick reply';
      demo.registers[10] = 'cargo test --locked --all-targets';
      demo.registerNames[10] = 'Run tests';
      demo.registers[23] = 'ssh dev@lab-host\\nConnect to the development VM';
      demo.registerNames[23] = 'Development VM';
      const listeners = {};
      window.__TAURI__ = {
        core: { invoke: async (name, args = {}) => {
          if (name === 'snapshot') return structuredClone(demo);
          if (name === 'set_clipboard') { demo.currentClipboard = args.text; listeners['clipforge-state']?.({ payload: structuredClone(demo) }); }
          return null;
        } },
        event: { listen: async (name, callback) => { listeners[name] = callback; return () => {}; } }
      };
    ` });
    await call('Page.navigate', { url: pathToFileURL(path.join(root, 'ui/index.html')).href });
    const evaluate = async expression => {
      const result = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
      if (result.exceptionDetails) throw new Error(result.exceptionDetails.text);
      return result.result.value;
    };
    for (let attempt = 0; attempt < 100; attempt++) {
      if (await evaluate(`document.querySelectorAll('#registers .row').length === 26`)) break;
      await new Promise(resolve => setTimeout(resolve, 100));
      if (attempt === 99) throw new Error('Frontend did not initialize');
    }
    await evaluate('document.fonts.ready.then(() => true)');
    // Check the panel actions at the minimum supported window width too.
    for (const width of [650, 840]) {
      await call('Emulation.setDeviceMetricsOverride', { width, height: 650, deviceScaleFactor: 2, mobile: false });
      const fits = await evaluate(`['clear-history', 'clear-registers', 'save-current', 'clear'].every(id => {
        const button = document.getElementById(id);
        const rect = button.getBoundingClientRect();
        const parent = button.closest('.panel') || button.closest('footer');
        const bounds = parent.getBoundingClientRect();
        return rect.width > 0 && rect.left >= bounds.left && rect.right <= bounds.right && rect.bottom <= innerHeight;
      })`);
      if (!fits) throw new Error(`Panel actions overflow at ${width}px`);
    }
    const output = path.join(root, 'docs/images');
    await fs.mkdir(output, { recursive: true });
    const capture = async name => {
      const { data } = await call('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
      await fs.writeFile(path.join(output, name), Buffer.from(data, 'base64'));
      console.log(`Saved docs/images/${name}`);
    };
    await capture('picker.png');
    await evaluate(`document.querySelector('[data-register="23"] .register-edit').click()`);
    for (let attempt = 0; attempt < 100; attempt++) {
      if (await evaluate(`!!document.querySelector('.inline-editor')`)) break;
      await new Promise(resolve => setTimeout(resolve, 50));
      if (attempt === 99) throw new Error('Inline editor did not open');
    }
    await evaluate(`document.querySelector('.inline-text').value = 'ssh dev@lab-host\\ncd ~/projects/clipforge'; document.querySelector('.inline-text').blur(); document.querySelector('.inline-editor').scrollIntoView({block: 'nearest'})`);
    await capture('register-edit.png');
    await send('Browser.close');
  } finally {
    socket?.close();
    if (browser.exitCode === null) browser.kill();
    // Keep the temporary Chrome profile for diagnosis; never touch a user's profile.
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
