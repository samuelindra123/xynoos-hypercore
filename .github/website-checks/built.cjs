const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { JSDOM } = require('jsdom');
const { base, root } = require('./preview.cjs');

const project = new URL(base, 'https://website-xybc.vercel.app');
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8');

function localFile(reference, from = project) {
  const url = new URL(reference, from);
  assert.equal(url.origin, project.origin, reference);
  assert.ok(url.pathname.startsWith(base), 'Project prefix required for ' + reference);
  const relative = decodeURIComponent(url.pathname.slice(base.length));
  return !relative || relative.endsWith('/') ? relative + 'index.html' : relative;
}

function openStudio(options = {}) {
  return new JSDOM(read('index.html'), { url: project.href, ...options });
}

function runStudio(window) {
  const scripts = window.document.querySelectorAll('script[type="module"]');
  assert.equal(scripts.length, 1);
  const script = scripts[0];
  const source = script.hasAttribute('src') ? read(localFile(script.src)) : script.textContent;
  window.eval(source);
}

module.exports = { localFile, openStudio, project, read, runStudio };
