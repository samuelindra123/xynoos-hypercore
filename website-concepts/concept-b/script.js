
const menu=document.querySelector('.menu');
menu?.addEventListener('click',()=>{const open=menu.getAttribute('aria-expanded')!=='true';menu.setAttribute('aria-expanded',open);document.querySelector('nav').classList.toggle('open',open)});
document.querySelectorAll('nav a').forEach(a=>a.addEventListener('click',()=>{document.querySelector('nav').classList.remove('open');menu?.setAttribute('aria-expanded','false')}));
if(!matchMedia('(prefers-reduced-motion: reduce)').matches && 'IntersectionObserver' in window){document.documentElement.classList.add('js-motion');const observer=new IntersectionObserver(entries=>entries.forEach(e=>{if(e.isIntersecting){e.target.classList.add('visible');observer.unobserve(e.target)}}),{threshold:.08});document.querySelectorAll('.reveal').forEach(el=>observer.observe(el));}

const layers=[{tag:'LAYER 01 / READ-ONLY',title:'Listen before deciding.',copy:'PressureIQ collects and qualifies Linux resource signals. Host support and permissions affect availability; missing or stale telemetry requires conservative handling.',url:'https://github.com/samuelindra123/xynoos-hypercore/blob/7094074/docs/MVP-ARCHITECTURE.md',label:'Read the architecture ↗'},{tag:'LAYER 02 / POLICY RESEARCH',title:'Make the reasoning inspectable.',copy:'AURA explores transactional policy. Trace-driven simulation makes decisions reproducible for a given input. This visual model is a conceptual overview, not a production control path.',url:'https://github.com/samuelindra123/xynoos-hypercore/blob/7094074/docs/MVP-QUICKSTART.md',label:'Explore simulation workflows ↗'},{tag:'LAYER 03 / PHYSICAL ADMISSION LOCKED',title:'Know when to stop.',copy:'Physical mutation is disabled in the MVP. Safety and recovery foundations exist, but durable physical crash recovery, exclusive ownership and safe cleanup remain validation questions.',url:'https://github.com/samuelindra123/xynoos-hypercore/blob/7094074/docs/MVP-SAFETY-RECOVERY.md',label:'Inspect safety & recovery ↗'}];
document.querySelectorAll('[data-layer]').forEach(button=>button.addEventListener('click',()=>{const i=Number(button.dataset.layer),layer=layers[i];document.querySelectorAll('[data-layer]').forEach(b=>{const active=b===button;b.classList.toggle('selected',active);b.setAttribute('aria-pressed',active)});document.querySelector('#layer-tag').textContent=layer.tag;document.querySelector('#layer-title').textContent=layer.title;document.querySelector('#layer-copy').textContent=layer.copy;const a=document.querySelector('#layer-link');a.href=layer.url;a.textContent=layer.label;document.querySelector('#detail-glyph').className='detail-glyph mode-'+i;}));

document.addEventListener('keydown',event=>{if(event.key==='Escape' && menu?.getAttribute('aria-expanded')==='true'){menu.setAttribute('aria-expanded','false');document.querySelector('nav').classList.remove('open');menu.focus()}});

// Studio night edition: event-driven depth, with no persistent rendering loop.
const motionQuery=matchMedia('(prefers-reduced-motion: reduce)');
const motionButton=document.querySelector('.motion-control');
const plate=document.querySelector('.architecture-art');
let userPaused=false;
function updateMotion(){
  const paused=userPaused||motionQuery.matches;
  document.body.classList.toggle('motion-paused',paused);
  motionButton.setAttribute('aria-pressed',String(paused));
  motionButton.disabled=motionQuery.matches;
  motionButton.setAttribute('aria-label',motionQuery.matches?'Reduced motion follows your device setting':paused?'Enable decorative motion':'Pause decorative motion');
  motionButton.querySelector('span').textContent=paused?'off':'on';
  if(paused){plate.style.removeProperty('--rotate-x');plate.style.removeProperty('--rotate-y')}
}
motionButton.addEventListener('click',()=>{userPaused=!document.body.classList.contains('motion-paused');updateMotion()});
motionQuery.addEventListener?.('change',updateMotion);updateMotion();
const plateToggle=document.querySelector('#plate-toggle');
plateToggle.addEventListener('click',()=>{
  const compact=plate.classList.toggle('is-compact');
  plateToggle.setAttribute('aria-pressed',String(compact));
  plateToggle.innerHTML=(compact?'Expand layers':'Compact layers')+' <span aria-hidden="true">↗</span>';
  document.querySelector('#plate-state').textContent=compact?'COMPACT VIEW / ONE SYSTEM':'EXPLODED VIEW / THREE LAYERS';
});
let depthFrame;
plate.addEventListener('pointermove',event=>{
  if(event.pointerType==='touch'||motionQuery.matches||userPaused)return;
  cancelAnimationFrame(depthFrame);
  depthFrame=requestAnimationFrame(()=>{
    const bounds=plate.getBoundingClientRect();
    const x=(event.clientX-bounds.left)/bounds.width-.5;
    const y=(event.clientY-bounds.top)/bounds.height-.5;
    plate.style.setProperty('--rotate-x',(-y*5).toFixed(2)+'deg');
    plate.style.setProperty('--rotate-y',(x*7).toFixed(2)+'deg');
  });
});
plate.addEventListener('pointerleave',()=>{
  cancelAnimationFrame(depthFrame);plate.style.setProperty('--rotate-x','0deg');plate.style.setProperty('--rotate-y','0deg');
});
// Stagger only neighbouring groups, retaining the original scroll reveals.
document.querySelectorAll('.report-links>a,.materials>div').forEach((el,index)=>{
  el.classList.add('reveal');el.style.setProperty('--reveal-delay',(index%2)*90+'ms');
});
if(!motionQuery.matches && 'IntersectionObserver' in window){
  const detailObserver=new IntersectionObserver(entries=>entries.forEach(entry=>{if(entry.isIntersecting){entry.target.classList.add('visible');detailObserver.unobserve(entry.target)}}),{threshold:.1});
  document.querySelectorAll('.report-links>a,.materials>div').forEach(el=>detailObserver.observe(el));
}
let progressPending=false;
function updateProgress(){
  const range=document.documentElement.scrollHeight-innerHeight;
  const progress=range>0?Math.min(1,Math.max(0,scrollY/range)):0;
  document.querySelector('.reading-progress').style.transform='scaleX('+progress+')';progressPending=false;
}
addEventListener('scroll',()=>{if(!progressPending){progressPending=true;requestAnimationFrame(updateProgress)}},{passive:true});
addEventListener('resize',updateProgress,{passive:true});updateProgress();

// A finite illustrated hardware story. This does not inspect or control hardware.
const systemFilm=document.querySelector('.system-film');
const filmPlay=document.querySelector('#film-play');
const filmScenes=[
 {shot:'overview',label:'SYSTEM / OVERVIEW',short:'SYSTEM',micro:'SYSTEM MAP / CONCEPTUAL',title:'A system of competing demands.',copy:'CPU work, memory pressure and parallel workloads share a machine. This stylized tour introduces the context—not a live hardware monitor.'},
 {shot:'cpu',label:'CPU / COMPUTE CORES',short:'CPU',micro:'CPU CORES / STYLIZED ACTIVITY',title:'Inside the compute layer.',copy:'The cover lifts to reveal a stylized compute die. PressureIQ can qualify available Linux CPU pressure signals; this animation does not read processor activity.'},
 {shot:'ram',label:'RAM / MEMORY BANKS',short:'RAM',micro:'MEMORY BANKS / ILLUSTRATIVE',title:'Follow the memory pressure.',copy:'Memory demand is a signal to investigate. HyperMemory explores compression only for participating application-owned data—with CPU and completion-time tradeoffs.'},
 {shot:'gpu',label:'GPU / CONCEPTUAL CUTAWAY',short:'GPU',micro:'PARALLEL WORK / FUTURE CONCEPT',title:'Explore parallel work.',copy:'A conceptual cutaway reveals parallel compute lanes. GPU context is a future illustration here, not a claim of implemented GPU telemetry, tuning or acceleration.'},
 {shot:'reason',label:'XYNOOS / OBSERVE & REASON',short:'REASON',micro:'POLICY / CONCEPTUAL FLOW',title:'From signals to reasoning.',copy:'Qualify available observations, inspect a policy trace, and consider the tradeoff. This conceptual flow explains the research direction; it does not execute the Rust engine.'},
 {shot:'safety',label:'SAFETY / PHYSICAL ADMISSION LOCKED',short:'SAFETY',micro:'BOUNDARY / NO PHYSICAL WRITES',title:'The boundary stays locked.',copy:'A recommendation is not permission to act. Physical mutation stays disabled in the MVP. Real performance gains and production readiness still require evidence.'}
];
let filmIndex=0,filmTimer=null,filmPlaying=false,filmAutoplayed=false,filmInteracted=false;
function filmMotionAllowed(){return !motionQuery.matches&&!document.body.classList.contains('motion-paused')}
function syncFilmControls(){
 const allowed=filmMotionAllowed();
 filmPlay.disabled=!allowed;
 filmPlay.querySelector('span').textContent=!allowed?'Motion off':filmPlaying?'Pause':filmIndex===filmScenes.length-1?'Replay film':'Play film';
 filmPlay.setAttribute('aria-label',!allowed?'Motion is disabled; select scenes manually':filmPlaying?'Pause illustrative system film':filmIndex===filmScenes.length-1?'Replay illustrative system film':'Play illustrative system film');
 filmPlay.querySelector('path').setAttribute('d',filmPlaying?'M7 5h4v14H7ZM14 5h4v14h-4Z':'m8 5 11 7-11 7Z');
 systemFilm.classList.toggle('film-running',filmPlaying);
 document.querySelector('.film-story-copy').setAttribute('aria-live',filmPlaying?'off':'polite');
}
function showFilmScene(index){
 filmIndex=Math.max(0,Math.min(filmScenes.length-1,index));const scene=filmScenes[filmIndex];
 systemFilm.dataset.shot=scene.shot;
 document.querySelector('#film-scene-count').textContent='SCENE 0'+(filmIndex+1)+' / 06';
 document.querySelector('#film-location').textContent=scene.label;
 document.querySelector('#film-story-count').textContent='0'+(filmIndex+1)+' / '+scene.short;
 document.querySelector('#film-story-title').textContent=scene.title;
 document.querySelector('#film-story-copy').textContent=scene.copy;
 document.querySelector('#micro-detail-label').textContent=scene.micro;
 document.querySelectorAll('[data-film-scene]').forEach(button=>button.setAttribute('aria-pressed',String(Number(button.dataset.filmScene)===filmIndex)));
 syncFilmControls();
}
function pauseFilm(){clearTimeout(filmTimer);filmTimer=null;filmPlaying=false;syncFilmControls()}
function scheduleFilm(){
 clearTimeout(filmTimer);
 filmTimer=setTimeout(()=>{
  if(!filmPlaying||!filmMotionAllowed()){pauseFilm();return}
  if(filmIndex===filmScenes.length-1){pauseFilm();return}
  showFilmScene(filmIndex+1);scheduleFilm();
 },4800);
}
function startFilm(){
 if(!filmMotionAllowed())return;
 if(filmIndex===filmScenes.length-1)showFilmScene(0);
 filmPlaying=true;syncFilmControls();scheduleFilm();
}
filmPlay.addEventListener('click',()=>{filmInteracted=true;filmPlaying?pauseFilm():startFilm()});
document.querySelector('#film-restart').addEventListener('click',()=>{filmInteracted=true;pauseFilm();showFilmScene(0);if(filmMotionAllowed())startFilm()});
document.querySelectorAll('[data-film-scene]').forEach(button=>button.addEventListener('click',()=>{filmInteracted=true;pauseFilm();showFilmScene(Number(button.dataset.filmScene))}));
if('IntersectionObserver' in window){
 const filmObserver=new IntersectionObserver(entries=>entries.forEach(entry=>{
  if(entry.isIntersecting){
   if(!filmAutoplayed&&!filmInteracted&&filmMotionAllowed()&&!document.hidden){filmAutoplayed=true;startFilm()}
  }else if(filmPlaying)pauseFilm();
 }),{threshold:.28});filmObserver.observe(systemFilm);
}
const filmMotionObserver=new MutationObserver(()=>{if(!filmMotionAllowed())pauseFilm();else syncFilmControls()});
filmMotionObserver.observe(document.body,{attributes:true,attributeFilter:['class']});
motionQuery.addEventListener?.('change',()=>{if(!filmMotionAllowed())pauseFilm();else syncFilmControls()});
document.addEventListener('visibilitychange',()=>{if(document.hidden)pauseFilm()});
showFilmScene(0);
