const {JSDOM}=require('jsdom');const fs=require('node:fs');const path=require('node:path');const assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../concept-b');let assertions=0;
function check(condition,label){assert.ok(condition,label);assertions++}
for(const reduced of [false,true]){
 const dom=new JSDOM(fs.readFileSync(path.join(root,'index.html'),'utf8'),{runScripts:'outside-only',url:'http://localhost:4102/',pretendToBeVisual:true});
 const w=dom.window,d=w.document;let id=0;const timers=new Map();w.setTimeout=(callback)=>{timers.set(++id,callback);return id};w.clearTimeout=id=>timers.delete(id);w.matchMedia=()=>({matches:reduced,addEventListener:()=>{}});
 w.eval(fs.readFileSync(path.join(root,'script.js'),'utf8'));
 const film=d.querySelector('.system-film'),play=d.querySelector('#film-play');
 const tick=()=>{const entry=timers.entries().next().value;check(!!entry,'scheduled scene');timers.delete(entry[0]);entry[1]()};
 check(film.dataset.shot==='overview','initial overview');check(d.querySelectorAll('[data-film-scene]').length===6,'six scenes');
 const shots=['overview','cpu','ram','gpu','reason','safety'];
 for(let n=0;n<shots.length;n++){
  const button=d.querySelector('[data-film-scene="'+n+'"]');button.click();check(film.dataset.shot===shots[n],'shot '+shots[n]);check(button.getAttribute('aria-pressed')==='true','scene pressed state');check(d.querySelectorAll('[data-film-scene][aria-pressed=true]').length===1,'one active scene');
 }
 check(d.querySelector('#film-story-copy').textContent.includes('disabled'),'safety boundary stated');
 d.querySelector('[data-film-scene="3"]').click();check(d.querySelector('#film-story-copy').textContent.includes('not a claim'),'GPU capability qualification');
 if(reduced){check(play.disabled,'reduced motion disables playback');d.querySelector('#film-restart').click();check(film.dataset.shot==='overview','reduced restart static');check(timers.size===0,'no reduced-motion timers')}else{
  d.querySelector('#film-restart').click();check(film.classList.contains('film-running'),'restart starts film');tick();check(film.dataset.shot==='cpu','automatic CPU scene');play.click();check(!film.classList.contains('film-running'),'pause stops animation');check(timers.size===0,'pause cancels timer');play.click();tick();check(film.dataset.shot==='ram','play continues narrative');
  d.querySelector('[data-film-scene="4"]').click();check(timers.size===0,'manual selection cancels autoplay');play.click();tick();check(film.dataset.shot==='safety','play reaches safety');tick();check(!film.classList.contains('film-running'),'finite film ends');check(play.textContent.includes('Replay'),'end offers replay');play.click();check(film.dataset.shot==='overview','replay returns to overview');
  d.querySelector('.motion-control').click();play.click();check(timers.size===0,'global motion pause prevents playback');
 }
 check(d.querySelector('.film-disclaimer').textContent.includes('never reads or changes'),'no host access disclosed');
 check(d.querySelector('a[href="#system-film"]'),'hero links to motion study');w.close();
}
console.log('PASS system film: '+assertions+' assertions; scenes, playback, finite ending, manual navigation, GPU disclosure, global pause and reduced motion. No browser rendering.');
