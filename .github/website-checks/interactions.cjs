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

const menu = document.querySelector('.site-menu-toggle');
const navigation = document.getElementById(menu.getAttribute('popovertarget'));
assert.equal(navigation.getAttribute('popover'), 'auto');
assert.equal(navigation.querySelectorAll('a').length, 10);
assert.equal(navigation.querySelector('[aria-current="page"]').textContent, 'Home');

for (const index of [1, 2, 0]) {
  const button = document.querySelector('[data-layer="' + index + '"]');
  button.click();
  assert.ok(document.querySelector('#layer-tag').textContent.includes('0' + (index + 1)));
  assert.equal(button.getAttribute('aria-pressed'), 'true');
}
window.close();
console.log('PASS Studio landmarks, navigation markup and architecture controls.');
