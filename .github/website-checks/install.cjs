const assert=require('node:assert/strict');
const {openStudio,runStudio}=require('./built.cjs');
(async()=>{
 for(const available of [true,false]){
  const dom=openStudio({runScripts:'outside-only'});
  const w=dom.window,d=w.document;w.matchMedia=()=>({matches:true,addEventListener(){}});
  const observers=[];const Observer=w.MutationObserver;w.MutationObserver=class extends Observer{constructor(...args){super(...args);observers.push(this)}};
  let copied=null;Object.defineProperty(w.navigator,'clipboard',{value:available?{writeText:async value=>{copied=value}}:undefined});
  runStudio(w);
  const command=d.querySelector('#install-command').textContent;
  assert.equal(command,'curl -fsSL https://raw.githubusercontent.com/samuelindra123/xynoos-hypercore/main/install.sh | sh && export PATH="$HOME/.local/bin:$PATH"');
  d.querySelector('#copy-install').click();await new Promise(resolve=>setImmediate(resolve));
  if(available){assert.equal(copied,command);assert.match(d.querySelector('#copy-install').textContent,/Copied/);assert.match(d.querySelector('#install-copy-status').textContent,/Copied/)}
  else{assert.match(d.querySelector('#install-copy-status').textContent,/Clipboard unavailable/);assert.equal(w.getSelection().toString(),command)}
  assert.equal(d.querySelector('#install-copy-status').getAttribute('role'),'status');
  observers.forEach(o=>o.disconnect());w.close();
 }
 console.log('PASS official install command, clipboard success and unavailable fallback.');
})().catch(error=>{console.error(error);process.exitCode=1});
