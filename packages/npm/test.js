// Run: SOMA_BIN=…/soma node packages/npm/test.js <a copy of packages/fixture/app.cell>
'use strict';
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const { load, SomaError, LoadError, Variant } = require('./index.js');

const program = process.argv[2];

async function kindOf(f) {
  try { await f(); } catch (e) { if (e instanceof SomaError) return e.kind; throw e; }
  return null;
}

(async () => {
  const app = await load(program);
  assert.ok(app.version.startsWith('3.'), app.version);
  assert.deepStrictEqual(app.handlers().Ledger.debit, { params: [{ name: 'amount', type: 'Int' }], returns: 'Int' });

  // typed values in and out
  assert.strictEqual(await app.seed(1000), 1000);
  assert.strictEqual(await app.debit(300), 700);
  assert.strictEqual(await app.call('Ledger.debit', { amount: 200 }), 500);
  assert.strictEqual(await app.balance(), 500);
  assert.strictEqual(await app.big(), 2n ** 70n);
  assert.strictEqual(await app.echo(2n ** 70n + 1n), 2n ** 70n + 1n);
  assert.strictEqual(await app.echo(2n ** 60n), 2n ** 60n);          // past 2^53: exact, as a bigint
  assert.strictEqual(await app.echo(7n), 7);
  for (const value of [null, true, 1.5, 'é', [1, [2, null]], { k: { n: 1 } }]) {
    assert.deepStrictEqual(await app.echo(value), value);
  }
  assert.deepStrictEqual(await app.find('r1'), { id: 'r1', tags: ['a', 'b'] });
  assert.strictEqual(await app.shout('hi'), 'HI');

  // the program's own sum types
  const pay = await app.classify(40);
  assert.ok(pay instanceof Variant && pay.type === 'Decision' && pay.variant === 'Pay' && pay.fields.amount === 40);
  assert.strictEqual((await app.classify(0)).variant, 'Skip');
  assert.strictEqual(await app.label(pay), 'pay 40');
  assert.strictEqual(await app.label(new Variant('Decision', 'Hold', { reason: 'manual' })), 'hold: manual');

  // calls made together are answered each with its own result
  const many = await Promise.all([1, 2, 3, 4, 5].map((n) => app.echo(n)));
  assert.deepStrictEqual(many, [1, 2, 3, 4, 5]);

  // errors by kind — and a failed call leaves nothing behind
  assert.strictEqual(await kindOf(() => app.debit(5000)), 'invariant');
  assert.strictEqual(await app.balance(), 500);
  assert.strictEqual(await kindOf(() => app.debit(-1)), 'requires');
  assert.strictEqual(await kindOf(() => app.find('')), 'BadId');
  assert.strictEqual(await kindOf(() => app.find('ghost')), 'not_found');
  assert.strictEqual(await kindOf(() => app.debit('ten')), 'type');
  assert.strictEqual(await kindOf(() => app.call('nope')), 'no_handler');
  assert.strictEqual(await kindOf(() => app.call('debit', { sum: 1 })), 'type');
  assert.strictEqual(app.nope, undefined);
  await app.close();
  assert.strictEqual(await kindOf(() => app.balance()), 'closed');

  // the data is the program's: it is there for the next process
  const again = await load(program);
  assert.strictEqual(await again.balance(), 500);
  await again.close();

  // a program that fails `soma check` does not load
  const broken = path.join(path.dirname(program), 'broken.cell');
  fs.writeFileSync(broken, 'cell B {\n    on f() { return undefined_name }\n}\n');
  await assert.rejects(() => load(broken), (e) => e instanceof LoadError && e.errors.length > 0 && /undefined/.test(e.errors[0].message));

  console.log('node: ok');
})().catch((e) => { console.error(e); process.exit(1); });
