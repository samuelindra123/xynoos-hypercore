const {JSDOM}=require('jsdom');const fs=require('node:fs');const path=require('node:path');const assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../../website');let assertions=0;
function check(value,label){assert.ok(value,label);assertions++}
for(const reduced of [false,true]){
 const dom=new JSDOM(fs.readFileSync(path.join(root,'index.html'),'utf8'),{runScripts:'outside-only',url:'http://localhost/xynoos-hypercore/'});
 const w=dom.window,d=w.document;w.matchMedia=()=>({matches:reduced,addEventListener:()=>{}});const observers=[];const NativeMutationObserver=w.MutationObserver;w.MutationObserver=class extends NativeMutationObserver{constructor(...args){super(...args);observers.push(this)}};w.eval(fs.readFileSync(path.join(root,'script.js'),'utf8'));
 const toggle=d.querySelector('#plate-toggle');toggle.click();check(toggle.getAttribute('aria-pressed')==='true','compact pressed');check(d.querySelector('.architecture-art').classList.contains('is-compact'),'compact diagram');check(d.querySelector('#plate-state').textContent.includes('COMPACT'),'compact caption');toggle.click();check(!d.querySelector('.architecture-art').classList.contains('is-compact'),'expanded diagram restored');check(toggle.textContent.includes('Compact layers'),'button action restored');
 const motion=d.querySelector('.motion-control');if(reduced){check(motion.disabled,'device reduced motion respected');check(d.body.classList.contains('motion-paused'),'decorative motion paused')}else{motion.click();check(d.body.classList.contains('motion-paused'),'manual pause');check(motion.getAttribute('aria-pressed')==='true','pause state accessible');motion.click();check(!d.body.classList.contains('motion-paused'),'manual resume')}
 for(const layer of [2,1,0]){d.querySelector('[data-layer="'+layer+'"]').click();check(d.querySelector('#detail-glyph').classList.contains('mode-'+layer),'layer visual updates');check(d.querySelector('#layer-tag').textContent.includes('0'+(layer+1)),'layer copy updates')}
 check(d.querySelector('a[href*="docs/README.md"]'),'documentation link');observers.forEach(observer=>observer.disconnect());w.close();
}
console.log('PASS Studio upgrade: '+assertions+' assertions across normal and reduced-motion modes. No browser rendering.');
