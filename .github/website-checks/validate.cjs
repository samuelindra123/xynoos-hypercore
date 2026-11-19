const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const cssTree = require("css-tree");
const { JSDOM } = require("jsdom");
const { HtmlValidate } = require("html-validate");
const { preview, root } = require("./preview.cjs");
const { localFile, project, read } = require("./built.cjs");

function checkScript(source) {
  const result = spawnSync(
    process.execPath,
    ["--input-type=module", "--check"],
    { input: source, encoding: "utf8" },
  );
  assert.equal(result.status, 0, result.stderr);
}

async function main() {
  const sitemap = read("sitemap.xml");
  const routes = [...sitemap.matchAll(/<loc>([^<]+)<\/loc>/g)].map((entry) => new URL(entry[1]));
  const expected = ["", "about", "changelog", "developers", "documentation", "downloads", "hypercore", "research", "roadmap"];
  assert.deepEqual(routes.map((route) => localFile(route.href)).sort(),
    expected.map((slug) => (slug ? slug + "/" : "") + "index.html").sort());
  const validator = new HtmlValidate(require("./htmlvalidate.json"));
  const assets = new Set(["robots.txt", "sitemap.xml", "404.html"]);
  const documents = new Map();
  for (const route of routes) {
    const file = localFile(route.href);
    assets.add(file);
    const html = read(file);
    const report = await validator.validateString(html);
    assert.ok(report.valid, file + " " + JSON.stringify(report.results.flatMap((result) => result.messages)));
    const dom = new JSDOM(html, { url: route.href });
    const doc = dom.window.document;
    documents.set(route.pathname, doc);
    const ids = [...doc.querySelectorAll("[id]")].map((node) => node.id);
    assert.equal(ids.length, new Set(ids).size, file + " unique IDs");
    assert.equal(doc.querySelectorAll("main h1").length, 1, file);
    assert.equal(doc.querySelector("link[rel=canonical]").href, route.href);
    assert.equal(doc.querySelector('meta[property="og:url"]').content, route.href);
    for (const slug of expected) {
      const path = new URL(slug ? slug + "/" : "", project).pathname;
      assert.ok([...doc.querySelectorAll("header a")].some((a) => new URL(a.href).pathname === path), file + " header link to " + path);
    }
  }
  const addAsset = (reference, from) => {
    const file = localFile(reference, from);
    assets.add(file);
    return file;
  };

  for (const [pathname, doc] of documents) {
    const page = new URL(pathname, project);
    const html = read(localFile(page.href));
    let respectsReducedMotion = false;
    for (const element of doc.querySelectorAll(
      '[src], [poster], link[href], meta[property="og:image"], meta[name="twitter:image"]',
    )) {
      const ref =
        element.getAttribute("src") ??
        element.getAttribute("href") ??
        element.getAttribute("poster") ??
        element.content;
      const file = addAsset(ref, page);
      if (element.rel === "stylesheet") {
        const css = read(file);
        respectsReducedMotion ||= css.includes("prefers-reduced-motion");
        const ast = cssTree.parse(css, {
          positions: true,
          onParseError: (error) => {
            throw error;
          },
        });
        cssTree.walk(ast, {
          visit: "Url",
          enter: (node) => {
            if (!node.value.startsWith("data:"))
              addAsset(node.value, new URL(ref, page));
          },
        });
      }
      if (element.tagName === "SCRIPT") checkScript(read(file));
    }
    assert.ok(respectsReducedMotion, pathname + " reduced-motion support");
    const scripts = [];
    for (const script of doc.scripts) {
      assert.equal(script.type, "module");
      if (!script.src) checkScript(script.textContent);
      scripts.push(script.src ? read(localFile(script.src)) : script.textContent);
    }
    assert.equal(scripts.length, pathname === project.pathname ? 1 : 0, "only Studio ships interaction code");
    assert.equal(doc.querySelectorAll("astro-island").length, 0, "static markup");

    for (const link of doc.querySelectorAll("a[href]")) {
      const url = new URL(link.getAttribute("href"), page);
      if (url.origin !== project.origin) {
        assert.equal(url.protocol, "https:", link.href);
        continue;
      }
      assert.ok(fs.existsSync(path.join(root, addAsset(url.href))), "local resource link " + link.href);
      if (url.hash) {
        const target = documents.get(url.pathname);
        assert.ok(target?.getElementById(decodeURIComponent(url.hash.slice(1))), link.href);
      }
    }
    for (const source of [html, ...scripts]) {
      assert.doesNotMatch(
        source,
        /localhost:|app\.github\.dev|\/blob\/7094074\//,
      );
      for (const match of source.matchAll(
        /https:\/\/github\.com\/samuelindra123\/xynoos-hypercore\/blob\/main\/([^"'<>`\s]+)/g,
      )) {
        const target = match[1].split("#")[0];
        assert.ok(
          fs.existsSync(path.resolve(root, "../..", target)),
          "repository evidence link " + target,
        );
      }
    }
  }
  for (const doc of documents.values()) {
    doc.defaultView.close();
  }
  assert.ok(read("robots.txt").includes(new URL("sitemap.xml", project).href));

  const files = fs
    .readdirSync(root, { recursive: true })
    .filter((file) => fs.statSync(path.join(root, file)).isFile());
  for (const file of files) {
    const bytes = fs.readFileSync(path.join(root, file));
    if (file.endsWith(".woff2")) {
      assert.equal(bytes.subarray(0, 4).toString("ascii"), "wOF2");
      assert.equal(bytes.readUInt32BE(8), bytes.length);
    }
    if (file.endsWith(".jpg")) {
      assert.equal(bytes.readUInt16BE(0), 0xffd8);
      assert.equal(bytes.readUInt16BE(bytes.length - 2), 0xffd9);
    }
    assert.doesNotMatch(file, /^(apt|legacy)\/|\.(ttf|png|gif)$/i);
    if (!file.endsWith("-OFL.txt"))
      assert.ok(assets.has(file), "unreferenced output " + file);
  }

  const server = preview();
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const origin = "http://127.0.0.1:" + server.address().port;
    for (const asset of assets) {
      const response = await fetch(
        origin + project.pathname + asset.replace(/(^|\/)index\.html$/, "$1"),
      );
      assert.equal(response.status, 200, asset);
      assert.deepEqual(
        Buffer.from(await response.arrayBuffer()),
        fs.readFileSync(path.join(root, asset)),
        asset,
      );
    }
    assert.equal((await fetch(origin + "/missing-page/")).status, 404);
    console.log(
      "PASS HTML, CSS, JS, SEO, fragments, media signatures and " +
        assets.size +
        " project-path HTTP assets.",
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
