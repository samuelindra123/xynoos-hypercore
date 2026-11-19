const assert=require('node:assert/strict');
const {openStudio,project,runStudio}=require('./built.cjs');let assertions=0;
function check(value,label){assert.ok(value,label);assertions++}
for(const reduced of [false,true]){
 const dom=openStudio({runScripts:'outside-only'});
 const w=dom.window,d=w.document;w.matchMedia=()=>({matches:reduced,addEventListener:()=>{}});const observers=[];const NativeMutationObserver=w.MutationObserver;w.MutationObserver=class extends NativeMutationObserver{constructor(...args){super(...args);observers.push(this)}};runStudio(w);
 const toggle=d.querySelector('#plate-toggle');toggle.click();check(toggle.getAttribute('aria-pressed')==='true','compact pressed');check(d.querySelector('.architecture-art').classList.contains('is-compact'),'compact diagram');check(d.querySelector('#plate-state').textContent.includes('COMPACT'),'compact caption');toggle.click();check(!d.querySelector('.architecture-art').classList.contains('is-compact'),'expanded diagram restored');check(toggle.textContent.includes('Compact layers'),'button action restored');
 const motion=d.querySelector('.motion-control');if(reduced){check(motion.disabled,'device reduced motion respected');check(d.body.classList.contains('motion-paused'),'decorative motion paused')}else{motion.click();check(d.body.classList.contains('motion-paused'),'manual pause');check(motion.getAttribute('aria-pressed')==='true','pause state accessible');motion.click();check(!d.body.classList.contains('motion-paused'),'manual resume')}
 for(const layer of [2,1,0]){d.querySelector('[data-layer="'+layer+'"]').click();check(d.querySelector('#detail-glyph').classList.contains('mode-'+layer),'layer visual updates');check(d.querySelector('#layer-tag').textContent.includes('0'+(layer+1)),'layer copy updates')}
 check([...d.querySelectorAll('footer a')].some((link)=>link.href===new URL('documentation/',project).href),'documentation route in footer');observers.forEach(observer=>observer.disconnect());w.close();
}
console.log('PASS Studio upgrade: '+assertions+' assertions across normal and reduced-motion modes. No browser rendering.');
