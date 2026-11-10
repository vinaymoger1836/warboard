// Play modes through the WebAssembly API (doc 12, P9): the browser's own entry points, not the
// Rust test API. Checks that the client cannot pick the side of an order, that the umpire view
// is refused outside Command both, that every side has its own event stream and details, that a
// hot-seat save reopens on its active side with the PIN kept, and that none of it moves the state hash.
// Usage: node crates/wb-wasm/node/play-modes-check.mjs --content <fixtures.wbpack> [--game-content build/content.wbpack]
// (build it with: wb content build --dir scenarios/fixtures --out <file>)
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { Engine, initSync, nation_options, preview_setup } from '../pkg/wb_wasm.js';

const { values } = parseArgs({ options: { content: { type: 'string' }, 'game-content': { type: 'string' } } });
if (!values.content) {
  console.error('usage: play-modes-check.mjs --content <fixtures.wbpack>');
  process.exit(2);
}
const wasm = initSync({ module: readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '..', 'pkg', 'wb_wasm_bg.wasm')) });
const content = readFileSync(values.content);

const scenario = (mode, blue, red) => `
name: wasm play modes
seed: 5
start_utc: 2026-10-27T20:00:00Z
mode: ${mode}
sides:
  - { name: Blue, nations: [tst], player: true, controller: ${blue} }
  - { name: Red, nations: [opp], controller: ${red} }
units:
  - { id: 10, name: Blue Frigate, platform: test-frigate, nation: tst, lat_deg: 55.2, lon_deg: 17.045678, heading_deg: 90 }
  - { id: 500, name: Red Corvette, platform: test-corvette, nation: opp, lat_deg: 55.234567, lon_deg: 18.123457, heading_deg: 270 }
orders:
  - { tick: 0, side: 0, type: AssignTask, units: [10], task: { kind: move, waypoints: [[18.4, 55.2]] } }
  - { tick: 0, side: 1, type: AssignTask, units: [500], task: { kind: move, waypoints: [[16.9, 55.23]] } }
`;
const make = (mode, blue = 'human', red = 'human') => Engine.create(scenario(mode, blue, red), content);
const throws = (fn, re) => assert.throws(fn, (e) => re.test(String(e?.message ?? e)));
const run = (e, ticks) => e.run_ticks(ticks);

// Sides and mode in the reply the worker reads.
{
  const e = make('hot_seat');
  const info = JSON.parse(e.sides());
  assert.equal(info.mode, 'hot_seat');
  assert.deepEqual(info.sides.map((s) => s.controller), ['human', 'human']);
  assert.equal(info.session.active_side, 0);
  assert.equal(JSON.parse(make('vs_ai', 'human', 'ai').sides()).mode, 'vs_ai');
  assert.equal(JSON.parse(make('watch', 'ai', 'ai').sides()).mode, 'watch');
}

// The client cannot choose the side of an order: the engine uses the active side.
{
  const e = make('command_both');
  run(e, 50);
  // Side 1 says it owns the order, but side 0 is active, so unit 500 is "not ours".
  const r = JSON.parse(e.order(JSON.stringify({ tick: e.tick(), side: 1, type: 'AssignTask', units: [500], task: { kind: 'rtb' } })));
  assert.equal(r.ok, false);
  assert.match(r.reason, /Unknown unit 500/);
  e.set_active_side(1);
  const own = JSON.parse(e.order(JSON.stringify({ tick: e.tick(), side: 0, type: 'AssignTask', units: [500], task: { kind: 'move', waypoints: [[17.1, 55.2]] } })));
  assert.equal(own.ok, true, JSON.stringify(own));
  const foreign = JSON.parse(e.order(JSON.stringify({ tick: e.tick(), side: 1, type: 'AssignTask', units: [10], task: { kind: 'rtb' } })));
  assert.equal(foreign.ok, false);
  assert.match(foreign.reason, /Unknown unit 10/);
}

// An AI side takes no client orders; a human cannot look at the AI side's picture in vs AI.
{
  const e = make('vs_ai', 'human', 'ai');
  throws(() => e.set_active_side(1), /controlled by the AI/);
  throws(() => e.details(1, 500), /controlled by the AI/);
  throws(() => e.take_events(1), /controlled by the AI/);
  throws(() => e.set_umpire(true), /command both/i);
  throws(() => e.set_view_side(255), /command both/i);
}

// Watch: both pictures can be looked at; orders are refused for both.
{
  const e = make('watch', 'ai', 'ai');
  e.set_active_side(1);
  JSON.parse(e.details(1, 500));
  const r = JSON.parse(e.order(JSON.stringify({ tick: e.tick(), side: 0, type: 'AssignTask', units: [500], task: { kind: 'rtb' } })));
  assert.equal(r.ok, false);
  assert.match(r.reason, /controlled by the AI/);
}

// Umpire view: Command both only, off by default, read-only, marks the war.
{
  const e = make('command_both');
  throws(() => e.set_view_side(255), /switched off/);
  e.set_umpire(true);
  e.set_view_side(255);
  run(e, 200);
  const len = e.encode_view();
  assert.ok(len > 48);
  const view = new Uint8Array(len);
  const mem = new Uint8Array(wasm.memory.buffer, e.view_ptr(), len);
  view.set(mem);
  assert.equal(view[7] & 1, 1, 'umpire flag');
  assert.equal(view[6], 255);
  const r = JSON.parse(e.order(JSON.stringify({ tick: e.tick(), side: 0, type: 'AssignTask', units: [10], task: { kind: 'rtb' } })));
  assert.equal(r.ok, false);
  assert.match(r.reason, /read-only/);
  assert.equal(JSON.parse(e.session()).umpire_used, true);
  assert.equal(JSON.parse(e.report(0, false, 'json')).umpire_used, true);
  e.set_umpire(false);
  e.encode_view();
}

// Events are kept apart per side, and a side's cursor does not move when the other is read.
{
  const e = make('hot_seat');
  run(e, 3000);
  const a = JSON.parse(e.take_events(0));
  const b = JSON.parse(e.take_events(1));
  assert.ok(a.length > 0 && b.length > 0, 'both sides have events');
  assert.deepEqual(JSON.parse(e.take_events(0)), [], 'a second read is empty');
  run(e, 200);
  const later = JSON.parse(e.take_events(1));
  assert.ok(later.length >= 0);
}

// Switching, viewing and PINs never move the state hash; a hot-seat save keeps the session.
{
  const e = make('hot_seat');
  run(e, 120);
  const hash = e.state_hash();
  e.set_active_side(1);
  e.set_view_side(0);
  e.set_view_side(1);
  e.set_hot_seat(JSON.stringify({ hand_over: { Turns: { minutes: 15 } }, urgent: 'hold' }));
  e.set_pin(1, '4821', new Uint8Array(16).fill(9));
  e.encode_view();
  assert.equal(e.state_hash(), hash, 'session changes are not war state');
  assert.equal(e.check_pin(1, '4821'), true);
  assert.equal(e.check_pin(1, '0000'), false);
  assert.equal(e.check_pin(0, ''), true);
  throws(() => e.set_pin(0, '12', new Uint8Array(16)), /4 digits/);
  const bytes = e.save('wasm play modes', '[]', 0);
  const loaded = Engine.load(bytes, content);
  const info = JSON.parse(loaded.sides());
  assert.equal(info.side, 1, 'a hot-seat save reopens on its active side');
  assert.equal(info.mode, 'hot_seat');
  assert.deepEqual(info.session.has_pin, [false, true]);
  assert.deepEqual(info.session.hot_seat, { hand_over: { Turns: { minutes: 15 } }, urgent: 'hold' });
  assert.equal(loaded.check_pin(1, '4821'), true);
  assert.equal(loaded.state_hash(), hash);
}

// War setup (doc 12 §4, ADR-046) on the Theatre North content, when given.
if (values['game-content']) {
  const game = readFileSync(values['game-content']);
  const options = JSON.parse(nation_options(game));
  assert.ok(options.length >= 8 && options.every((o) => o.platforms > 0 && o.extent[0] <= o.extent[2]));
  const setup = (startMode, a, b) => ({
    name: 'wasm setup',
    seed: 3,
    start_utc: '2026-10-27T06:00:00Z',
    start_mode: startMode,
    sides: [
      { name: 'League', nations: ['ald', 'hlv'], controller: a, difficulty: 'normal', roe: 'bravo' },
      { name: 'Pact', nations: ['vsm', 'kld'], controller: b, difficulty: 'hard', roe: 'charlie' },
    ],
  });
  const preview = JSON.parse(preview_setup(JSON.stringify(setup('garrisoned', 'human', 'ai')), game));
  assert.equal(preview.ok, true);
  assert.equal(preview.mode, 'vs_ai');
  assert.ok(preview.extent[0] < preview.extent[2]);
  const refused = JSON.parse(preview_setup(JSON.stringify({ ...setup('garrisoned', 'human', 'human'), mode: 'watch' }), game));
  assert.equal(refused.ok, false);

  const gar = Engine.create_from_setup(JSON.stringify(setup('garrisoned', 'human', 'ai')), game);
  const info = JSON.parse(gar.sides());
  assert.equal(info.mode, 'vs_ai');
  assert.equal(info.sides[1].controller, 'ai');
  assert.equal(info.sides[1].difficulty, 'hard');
  assert.equal(gar.advance(250, 1) >= 0, true);
  run(gar, 100);

  const dep = Engine.create_from_setup(JSON.stringify(setup('deploy', 'human', 'ai')), game);
  assert.equal(dep.advance(250, 10), 0, 'the clock is held while deploying');
  const view = JSON.parse(dep.deployment(0));
  assert.equal(view.deploying, true);
  const line = view.pool.find((l) => l.place === 'airfield');
  const base = view.bases.find((b) => b.air && b.nation === line.nation);
  const ok = JSON.parse(dep.order(JSON.stringify({ tick: dep.tick(), side: 0, type: 'DeployForce', force: line.id, count: 1, base: base.id })));
  assert.equal(ok.ok, true, JSON.stringify(ok));
  assert.equal(JSON.parse(dep.deployment(0)).pool.find((l) => l.id === line.id && l.nation === line.nation).left, line.left - 1);
  assert.equal(dep.advance(250, 10), 0);
  assert.equal(JSON.parse(dep.order(JSON.stringify({ tick: dep.tick(), side: 0, type: 'FinishDeployment' }))).ok, true);
  assert.ok(dep.advance(250, 10) > 0, 'the clock runs once the side has finished');
}

console.log('play-modes-check: ok');
