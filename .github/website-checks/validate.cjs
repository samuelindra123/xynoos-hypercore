const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{execFileSync}=require('node:child_process');
const {JSDOM}=require('jsdom'),cssTree=require('css-tree'),{HtmlValidate}=require('html-validate'),{preview,root,base}=require('./preview.cjs');
(async()=>{
 const html=fs.readFileSync(path.join(root,'index.html'),'utf8'),css=fs.readFileSync(path.join(root,'styles.css'),'utf8'),dom=new JSDOM(html),d=dom.window.document;
 const report=await new HtmlValidate(require('./htmlvalidate.json')).validateString(html);assert.ok(report.valid,JSON.stringify(report.results));
 cssTree.parse(css,{positions:true,onParseError:error=>{throw error}});execFileSync(process.execPath,['--check',path.join(root,'script.js')]);
 const ids=[...d.querySelectorAll('[id]')].map(x=>x.id);assert.equal(ids.length,new Set(ids).size);
 const assets=new Set(['index.html','robots.txt','sitemap.xml','assets/studio-social.png']);
 for(const el of d.querySelectorAll('[src],link[href]')){const ref=el.getAttribute('src')||el.getAttribute('href');if(!/^(https?:|#|data:)/.test(ref)){assert.ok(!ref.startsWith('/'),'relative asset path');assets.add(ref)}}
 for(const match of css.matchAll(/url\((assets\/[^)]+)\)/g))assets.add(match[1]);
 for(const a of d.querySelectorAll('a[href]')){const href=a.getAttribute('href');if(href.startsWith('#'))assert.ok(ids.includes(href.slice(1)),href);else if(!href.startsWith('https://'))throw Error('Unexpected link '+href)}
 for(const file of ['index.html','styles.css','script.js'])assert.ok(!/localhost:|app\.github\.dev|\/blob\/7094074\//.test(fs.readFileSync(path.join(root,file),'utf8')),'no experimental URLs');
 const project='https://samuelindra123.github.io/xynoos-hypercore/';assert.equal(d.querySelector('link[rel=canonical]').href,project);assert.ok(fs.readFileSync(path.join(root,'sitemap.xml'),'utf8').includes(project));
 for(const file of ['index.html','script.js'])for(const match of fs.readFileSync(path.join(root,file),'utf8').matchAll(/https:\/\/github.com\/samuelindra123\/xynoos-hypercore\/blob\/main\/([^"'<>\s]+)/g)){const target=match[1].split('#')[0];assert.ok(fs.existsSync(path.resolve(root,'..',target)),'repository evidence link '+target)}
 const server=preview();await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 try{const origin='http://127.0.0.1:'+server.address().port;for(const asset of assets){const response=await fetch(origin+base+asset);assert.equal(response.status,200,asset);assert.deepEqual(Buffer.from(await response.arrayBuffer()),fs.readFileSync(path.join(root,asset)),asset)}assert.equal((await fetch(origin+'/styles.css')).status,404);console.log('PASS HTML, CSS parsing, JS syntax, SEO, repository evidence, fragments and '+assets.size+' subpath HTTP assets.')}finally{await new Promise(resolve=>server.close(resolve));dom.window.close()}
})().catch(error=>{console.error(error);process.exitCode=1});
