const assert = require("node:assert/strict");
const { openStudio } = require("./built.cjs");

const { window } = openStudio();
const { document } = window;
assert.ok(document.querySelector(".system-film"));
assert.ok(document.querySelector(".hardware-svg"));
assert.equal(document.querySelectorAll("[data-film-scene]").length, 6);
assert.ok(document.querySelector("#film-play"));
assert.ok(document.querySelector("#film-restart"));
assert.ok(
  document
    .querySelector(".film-disclaimer")
    .textContent.includes("never reads or changes"),
);
assert.equal(document.querySelector("video"), null);
assert.equal(document.querySelector(".film-transcript"), null);
window.close();
console.log(
  "PASS original interactive SVG motion study and all six scene controls restored.",
);
