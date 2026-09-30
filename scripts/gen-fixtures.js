// Generates SYNTHETIC but realistic tool outputs into tests/fixtures/synthetic.
// (The Roblox fixtures in tests/fixtures are real captures; these are not.)
const fs = require('fs');
const path = require('path');
const out = path.join(__dirname, '..', 'tests', 'fixtures', 'synthetic');
fs.mkdirSync(out, { recursive: true });
const w = (name, text) => fs.writeFileSync(path.join(out, name), text);

// cargo build with a real error buried after 180 crates
let cargo = '';
for (let i = 0; i < 180; i++) cargo += `   Compiling dep${i} v1.${i % 9}.${i % 4}\n`;
cargo += `   Compiling myapp v0.1.0 (C:\\proj\\myapp)
warning: unused variable: \`x\`
 --> src\\main.rs:12:9
  |
12 |     let x = 5;
  |         ^ help: if this is intentional, prefix it with an underscore: \`_x\`

error[E0308]: mismatched types
 --> src\\main.rs:20:18
  |
20 |     let y: u32 = "hello";
  |            ---   ^^^^^^^ expected \`u32\`, found \`&str\`
  |            |
  |            expected due to this

error: could not compile \`myapp\` (bin "myapp") due to 1 previous error; 1 warning emitted
`;
w('Bash__cargo-build-error.txt', cargo);

// npm install noise
let npm = '';
for (let i = 0; i < 40; i++) npm += `npm warn deprecated pkg${i}@1.0.${i}: This package is no longer supported\n`;
for (let i = 0; i < 60; i++) npm += `npm http fetch GET 200 https://registry.npmjs.org/lib${i} ${20 + i}ms (cache miss)\n`;
npm += '\nadded 412 packages, and audited 413 packages in 14s\n\n83 packages are looking for funding\n  run `npm fund` for details\n\n3 vulnerabilities (1 moderate, 2 high)\n';
w('Bash__npm-install.txt', npm);

// pytest: 250 dots, one failure with traceback
let py = '============================= test session starts =============================\ncollected 251 items\n\n';
for (let i = 0; i < 25; i++) py += 'tests/test_mod' + i + '.py ' + '.'.repeat(10) + `  [ ${Math.round((i + 1) * 100 / 26)}%]\n`;
py += `tests/test_api.py .......F.                                             [100%]

================================== FAILURES ===================================
_______________________________ test_create_user ______________________________

    def test_create_user():
        user = create_user("ana")
>       assert user.id == 42
E       assert 7 == 42
E        +  where 7 = User(id=7, name='ana').id

tests/test_api.py:31: AssertionError
=========================== short test summary info ===========================
FAILED tests/test_api.py::test_create_user - assert 7 == 42
======================== 1 failed, 250 passed in 4.21s ========================
`;
w('Bash__pytest-fail.txt', py);

// GitHub-style issue list (typical of MCP servers wrapping REST APIs)
const user = (n) => ({
  login: n, id: 1000 + n.length, node_id: 'MDQ6VXNlcjEyMzQ1', avatar_url: `https://avatars.githubusercontent.com/u/${1000 + n.length}?v=4`,
  gravatar_id: '', url: `https://api.github.com/users/${n}`, html_url: `https://github.com/${n}`,
  followers_url: `https://api.github.com/users/${n}/followers`, following_url: `https://api.github.com/users/${n}/following{/other_user}`,
  repos_url: `https://api.github.com/users/${n}/repos`, type: 'User', site_admin: false,
});
const issues = Array.from({ length: 25 }, (_, i) => ({
  url: `https://api.github.com/repos/acme/widgets/issues/${i + 1}`,
  repository_url: 'https://api.github.com/repos/acme/widgets',
  html_url: `https://github.com/acme/widgets/issues/${i + 1}`,
  id: 2000000 + i, node_id: 'I_kwDOABC' + i, number: i + 1,
  title: `Widget ${i + 1} fails when the frobnicator is enabled`,
  user: user(['ana', 'bob', 'cy'][i % 3]), labels: i % 2 ? [{ id: 9, name: 'bug', color: 'd73a4a', default: true, description: "Something isn't working" }] : [],
  state: 'open', locked: false, assignee: null, assignees: [], milestone: null, comments: i % 5,
  created_at: '2026-09-0' + (1 + i % 9) + 'T10:11:12Z', updated_at: '2026-09-1' + (i % 9) + 'T10:11:12Z', closed_at: null,
  author_association: 'CONTRIBUTOR', active_lock_reason: null, body: i % 4 ? `Steps to reproduce #${i}: enable frobnicator, click widget.` : null,
  reactions: { url: `https://api.github.com/repos/acme/widgets/issues/${i + 1}/reactions`, total_count: 0, '+1': 0, '-1': 0, laugh: 0, hooray: 0, confused: 0, heart: 0, rocket: 0, eyes: 0 },
  timeline_url: `https://api.github.com/repos/acme/widgets/issues/${i + 1}/timeline`, performed_via_github_app: null, state_reason: null,
}));
w('mcp__github__list_issues.txt', JSON.stringify(issues, null, 2));

// a noisy application log with repeats and one crash
let log = '';
for (let i = 0; i < 400; i++) {
  log += `2026-09-29T10:${String(i % 60).padStart(2, '0')}:00Z INFO request id=${1000 + i} path=/api/items latency=${10 + i % 7}ms status=200\n`;
  if (i % 40 === 0) log += '2026-09-29T10:00:00Z WARN cache miss ratio above threshold, consider warming the cache\n';
}
log += '2026-09-29T11:00:00Z ERROR unhandled exception: NullReferenceException at OrderService.Process(Order o) line 88\n';
w('Bash__app-log.txt', log);
console.log('fixtures written to', out);
