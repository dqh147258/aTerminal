const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const net = require('node:net');
const crypto = require('node:crypto');
const {spawn} = require('node:child_process');

(async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'aiterminal-admin-'));
  fs.chmodSync(directory, 0o700);
  const tokenFile = path.join(directory, 'admin-token');
  fs.writeFileSync(tokenFile, crypto.randomBytes(32).toString('hex'), {mode: 0o600});
  const socket = net.createServer();
  await new Promise((resolve) => socket.listen(0, '127.0.0.1', resolve));
  const port = socket.address().port;
  await new Promise((resolve) => socket.close(resolve));
  const log = fs.openSync(path.join(directory, 'server.log'), 'a', 0o600);
  const root = path.resolve(__dirname, '../../..');
  const child = spawn(path.join(root, 'target/debug/ai-terminal-server'), [], {
    detached: true, stdio: ['ignore', log, log],
    env: {...process.env, AI_TERMINAL_ADMIN_TOKEN_FILE: tokenFile, AI_TERMINAL_DB: path.join(directory, 'preview.sqlite3'), AI_TERMINAL_BIND: `127.0.0.1:${port}`},
  });
  child.unref(); fs.closeSync(log);
  const url = `http://127.0.0.1:${port}`;
  for (let attempt = 0; attempt < 50; attempt++) {
    try { if ((await fetch(`${url}/healthz`)).ok) break; } catch {}
    if (attempt === 49) throw new Error('Preview did not start; inspect the isolated server.log');
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  const metadata = {url, pid: child.pid, directory, tokenFile, database: path.join(directory, 'preview.sqlite3')};
  const metadataFile = path.join(directory, 'preview.json');
  fs.writeFileSync(metadataFile, JSON.stringify(metadata, null, 2) + '\n', {mode: 0o600});
  console.log(JSON.stringify({url: `${url}/admin/`, metadataFile, tokenFile}));
})().catch((error) => { console.error(error.message); process.exitCode = 1; });
