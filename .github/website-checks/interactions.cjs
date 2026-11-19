const assert = require('node:assert/strict');
const { openStudio, runStudio } = require('./built.cjs');

const { window } = openStudio({ runScripts: 'outside-only' });
const { document } = window;
window.matchMedia = () => ({ matches: true });
runStudio(window);

assert.equal(document.documentElement.lang, 'en');
assert.equal(document.querySelectorAll('h1').length, 1);
for (const tag of ['main', 'nav', 'footer']) assert.ok(document.querySelector(tag));
for (const input of document.querySelectorAll('input')) {
  assert.ok(document.querySelector('label[for="' + input.id + '"]'));
}

const menu = document.querySelector('.menu');
menu.click();
assert.equal(menu.getAttribute('aria-expanded'), 'true');
document.querySelector('nav a').click();
assert.equal(menu.getAttribute('aria-expanded'), 'false');
menu.click();
document.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'Escape' }));
assert.equal(menu.getAttribute('aria-expanded'), 'false');
assert.equal(document.activeElement, menu);

for (const index of [1, 2, 0]) {
  const button = document.querySelector('[data-layer="' + index + '"]');
  button.click();
  assert.ok(document.querySelector('#layer-tag').textContent.includes('0' + (index + 1)));
  assert.equal(button.getAttribute('aria-pressed'), 'true');
}
window.close();
console.log('PASS Studio landmarks, navigation, keyboard dismissal and architecture controls.');
