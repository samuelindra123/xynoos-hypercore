const {JSDOM}=require('jsdom');const fs=require('fs');const assert=require('node:assert/strict');
const base=require('node:path').resolve(__dirname,'..');let checks=0;
function test(value,msg){assert.ok(value,msg);checks++}
for(const c of ['a','b','c']){
 const path=base+'/concept-'+c;
 const dom=new JSDOM(fs.readFileSync(path+'/index.html','utf8'),{runScripts:'outside-only',url:'http://localhost:410'+({a:1,b:2,c:3}[c])+'/'});const w=dom.window,d=w.document;
 w.matchMedia=()=>({matches:true});w.eval(fs.readFileSync(path+'/script.js','utf8'));
 test(d.documentElement.lang==='en',c+' language');test(d.querySelectorAll('h1').length===1,c+' one h1');test(d.querySelector('main')&&d.querySelector('nav')&&d.querySelector('footer'),c+' landmarks');
 for(const a of d.querySelectorAll('a[href^="#"]'))test(!!d.querySelector(a.getAttribute('href')),c+' anchor '+a.getAttribute('href'));
 for(const input of d.querySelectorAll('input'))test(!!d.querySelector('label[for="'+input.id+'"]'),c+' labeled input');
 const menu=d.querySelector('.menu');menu.click();test(menu.getAttribute('aria-expanded')==='true',c+' menu opens');d.querySelector('nav a').click();test(menu.getAttribute('aria-expanded')==='false',c+' menu closes');
 if(c==='a'){for(const type of ['simulate','recovery','observe']){d.querySelector('[data-command="'+type+'"]').click();test(d.querySelector('#command-text').textContent.includes(type),'terminal '+type);test(d.querySelector('#terminal-output').textContent.includes('[demo]'),'demo output');}d.querySelector('#replay').click();test(d.querySelector('#terminal-output').textContent.includes('DISABLED'),'replay');}
 if(c==='b'){for(const i of [1,2,0]){d.querySelector('[data-layer="'+i+'"]').click();test(d.querySelector('#layer-tag').textContent.includes('0'+(i+1)),'layer detail '+i);test(d.querySelector('[data-layer="'+i+'"]').getAttribute('aria-pressed')==='true','pressed layer '+i);}}
 if(c==='c'){
 const title=()=>d.querySelector('#rec-title').textContent;const change=(id,value)=>{const el=d.querySelector('#'+id);el.value=value;el.dispatchEvent(new w.Event('input'))};
 test(title()==='Continue observing.','default recommendation');d.querySelector('[data-preset="stress"]').click();test(title()==='Review a bounded experiment.','memory stress');test(d.querySelector('#memory-value').textContent==='88%','memory readout');d.querySelector('[data-preset="mixed"]').click();test(title()==='Inspect the tradeoff first.','mixed pressure');d.querySelector('#stale').click();test(title()==='Withhold a recommendation.','stale data');d.querySelector('#stale').click();change('memory',10);change('cpu',90);test(title()==='Investigate CPU contention.','CPU pressure');d.querySelector('[data-preset="idle"]').click();test(title()==='Continue observing.','idle');for(const name of ['pressure','safety','aura']){d.querySelector('[data-node="'+name+'"]').click();test(d.querySelector('[data-node="'+name+'"]').getAttribute('aria-pressed')==='true','node '+name);}d.querySelector('#reset').click();test(d.querySelector('#memory-value').textContent==='42%','reset memory');test(d.querySelector('#cpu-value').textContent==='24%','reset cpu');test(!d.querySelector('#stale').checked,'reset freshness');}
 test(fs.readFileSync(path+'/styles.css','utf8').includes('prefers-reduced-motion'),c+' reduced-motion rule');w.close();console.log('PASS concept '+c.toUpperCase());
}
console.log('PASS '+checks+' assertions. DOM interaction checks; no browser rendering performed.');
