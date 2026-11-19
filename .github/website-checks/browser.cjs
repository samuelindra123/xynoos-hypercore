const {chromium}=require('playwright'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),{preview,base}=require('./preview.cjs');
(async()=>{
 const artifacts=path.join(__dirname,'artifacts');fs.mkdirSync(artifacts,{recursive:true});
 let server;if(!process.env.STUDIO_URL){server=preview();await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve))}
 const url=process.env.STUDIO_URL||'http://127.0.0.1:'+server.address().port+base;
 const browser=await chromium.launch({headless:true,args:['--no-sandbox']}).catch(error=>{server?.close();throw error});let assertions=0;
 try{
  for(const width of [375,768,1440]){
   const page=await browser.newPage({viewport:{width,height:900}});const errors=[];page.on('pageerror',error=>errors.push(error.message));page.on('response',res=>{if(res.status()>=400)errors.push(res.status()+' '+res.url())});
   await page.goto(url,{waitUntil:'networkidle'});await page.evaluate(()=>document.fonts.ready);await page.waitForTimeout(300);
   assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),'no horizontal overflow '+width);assertions++;
   assert.ok(await page.evaluate(()=>document.fonts.check('500 24px "Space Grotesk"')),'self-hosted typeface');assertions++;
   await page.screenshot({path:path.join(artifacts,'studio-'+width+'.png')});
   await page.context().grantPermissions(['clipboard-read','clipboard-write']);
   await page.locator('#copy-install').click();
   await page.waitForFunction(()=>document.querySelector('#copy-install').textContent.includes('Copied'));
   assert.match(await page.locator('#install-copy-status').textContent(),/Copied/);assertions++;
   await page.locator('.install-section').screenshot({path:path.join(artifacts,'install-'+width+'.png')});
   if(width<=1080){await page.locator('.site-menu-toggle').click();assert.ok(await page.locator('#site-menu').isVisible());await page.keyboard.press('Escape');assert.equal(await page.locator('#site-menu').isVisible(),false);assertions+=2}
   await page.locator('[data-layer="2"]').click();assert.match(await page.locator('#layer-title').textContent(),/Know when/);assertions++;
   await page.locator('#plate-toggle').click();assert.equal(await page.locator('#plate-toggle').getAttribute('aria-pressed'),'true');assertions++;
   for(const [index,shot] of [[1,'cpu'],[2,'ram'],[3,'gpu'],[4,'reason'],[5,'safety']]){await page.locator('[data-film-scene="'+index+'"]').click();assert.equal(await page.locator('.system-film').getAttribute('data-shot'),shot);assertions++}
   await page.locator('[data-film-scene="1"]').click();await page.waitForTimeout(2600);await page.locator('.system-film').screenshot({path:path.join(artifacts,'film-'+width+'.png')});
   await page.locator('#film-play').click();assert.ok(await page.locator('.system-film').evaluate(el=>el.classList.contains('film-running')));await page.locator('#film-play').click();assertions++;
   const nav=page.locator(width<=1080 ? '#site-menu' : 'header .site-nav:not(.site-nav-mobile)');
   const routes=await nav.locator('a').evaluateAll((links,basePath)=>links.filter(link=>link.origin===location.origin&&link.pathname!==basePath).map(link=>link.href),base);
   assert.equal(routes.length,8);assertions++;
   for(const route of routes){
    await page.goto(url);
    if(width<=1080){
     await page.locator('.site-menu-toggle').focus();
     await page.keyboard.press('Enter');
    }
    const routePath=new URL(route).pathname;
    await nav.locator('a[href="'+routePath+'"]').click();
    assert.equal(page.url(),route);
    assert.equal(await page.locator('main h1').count(),1);
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),'no horizontal overflow '+route+' '+width);
    if(width<=1080) await page.locator('.site-menu-toggle').click();
    assert.equal(await nav.locator('[aria-current="page"]').getAttribute('href'),routePath);
    assert.equal(await page.locator('.footer-links [aria-current="page"]').getAttribute('href'),routePath);
    assertions+=5;
   }
   assert.deepEqual(errors,[],'no browser errors or missing assets');assertions++;await page.close();
  }
  const page=await browser.newPage({viewport:{width:375,height:812},reducedMotion:'reduce'});await page.goto(url,{waitUntil:'networkidle'});assert.ok(await page.locator('#film-play').isDisabled());await page.locator('[data-film-scene="3"]').click();assert.equal(await page.locator('.system-film').getAttribute('data-shot'),'gpu');assert.equal(await page.locator('.board-camera').evaluate(el=>getComputedStyle(el).transitionDuration),'0s');assertions+=3;await page.close();
  console.log('PASS browser: '+assertions+' assertions at 375 / 768 / 1440 px and reduced motion; screenshots: '+artifacts);
 }finally{await browser.close();if(server)await new Promise(resolve=>server.close(resolve))}
})().catch(error=>{console.error(error);process.exitCode=1});
