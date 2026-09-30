// End-to-end check of `mtk mcp-proxy` against a real stdio MCP server.
// usage: node scripts/mcp-e2e.js <server cmd> [args...]     (Roblox: cmd.exe /c ...\mcp.bat)
// Calls the same read-only tools directly and through the proxy and compares.
const { spawn } = require('child_process');

function session(cmd, args, env) {
  const child = spawn(cmd, args, { stdio: ['pipe', 'pipe', 'inherit'], env: { ...process.env, ...env } });
  let buf = '';
  const waiting = new Map();
  child.stdout.on('data', (d) => {
    buf += d.toString('utf8');
    let i;
    while ((i = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, i);
      buf = buf.slice(i + 1);
      if (!line.trim()) continue;
      const m = JSON.parse(line);
      if (m.id !== undefined && waiting.has(m.id)) {
        waiting.get(m.id)({ msg: m, raw: line });
        waiting.delete(m.id);
      }
    }
  });
  let n = 0;
  const rpc = (method, params) =>
    new Promise((res, rej) => {
      const id = ++n;
      waiting.set(id, res);
      child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
      setTimeout(() => rej(new Error('timeout ' + method)), 30000);
    });
  const notify = (method) => child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method }) + '\n');
  return { rpc, notify, close: () => child.stdin.end() };
}

async function run(label, cmd, args, env) {
  const s = session(cmd, args, env);
  await s.rpc('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'mtk-e2e', version: '0' } });
  s.notify('notifications/initialized');
  const tools = await s.rpc('tools/list', {});
  const studios = await s.rpc('tools/call', { name: 'list_roblox_studios', arguments: {} });
  const studioText = studios.msg.result.content[0].text;
  const studioId = JSON.parse(studioText).studios[0].id;
  const tree = await s.rpc('tools/call', {
    name: 'search_game_tree',
    arguments: { datamodel_type: 'Edit', studio_id: studioId, path: 'Workspace', max_depth: 3, head_limit: 60 },
  });
  const text = tree.msg.result.content[0].text;
  s.close();
  return { label, toolCount: tools.msg.result.tools.length, studioText, treeText: text, treeBytes: tree.raw.length, isError: !!tree.msg.result.isError };
}

(async () => {
  const [cmd, ...args] = process.argv.slice(2);
  const direct = await run('direct', cmd, args, {});
  const proxied = await run('proxy', 'mtk', ['mcp-proxy', '--name', 'RobloxStudio', '--', cmd, ...args], { MTK_HOME: process.env.TEMP + '\\mtk-e2e-home' });
  console.log('tools/list count  direct:', direct.toolCount, ' proxy:', proxied.toolCount);
  console.log('list_roblox_studios identical (small output untouched):', direct.studioText === proxied.studioText);
  console.log('search_game_tree  direct bytes:', direct.treeBytes, ' proxy bytes:', proxied.treeBytes,
    ' saved:', (100 - (proxied.treeBytes * 100) / direct.treeBytes).toFixed(1) + '%');
  console.log('isError direct/proxy:', direct.isError, proxied.isError);
  console.log('--- proxied tree text (first 700 chars) ---');
  console.log(proxied.treeText.slice(0, 700));
  process.exit(0);
})().catch((e) => { console.error('FAILED', e); process.exit(1); });
