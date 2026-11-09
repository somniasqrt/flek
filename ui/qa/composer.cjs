const {webkit}=require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert=require('node:assert/strict');
(async()=>{
 const browser=await webkit.launch({headless:true});
 try {
 const page=await browser.newPage({viewport:{width:1440,height:1000}});
 await page.route('**/*',r=>new URL(r.request().url()).hostname==='127.0.0.1'?r.continue():r.abort());
 await page.goto('http://127.0.0.1:5174/preview.html');
 const editor=page.locator('.composer-editor').first(); await editor.waitFor();
 const mode=process.argv[2]; const before=process.argv[3]==='before';
 const payload=()=>editor.evaluate(async el=>(await import('/src/rich-compose.ts')).editorBlocks(el));
 if(mode==='urls') {
 await editor.fill('See https://example.org/a_(b). and https://example.com/two!');
 const blocks=await payload();
 const links=blocks[0].elements[0].elements.filter(n=>n.type==='link');
 assert.equal(links.length,before?0:2);
 if(!before) {
 assert.deepEqual(links.map(n=>n.url),['https://example.org/a_(b)','https://example.com/two']);
 await page.getByRole('button',{name:'Send',exact:true}).first().click();
 const sent=page.locator('.msg').filter({hasText:'See'}).last();
 await sent.locator('a[href="https://example.org/a_(b)"]').waitFor();
 await editor.evaluate(el=>{el.innerHTML='<pre>https://example.com/pre</pre><code>https://example.com/code</code>';});
 const code=await payload();
 assert.equal(code[0].elements[0].type,'rich_text_preformatted');
 assert.equal(JSON.stringify(code).includes('"type":"link"'),false);
 }
 } else if(mode==='mrkdwn') {
 await editor.fill('*bold* _italic_ ~strike~ `code`');
 const nodes=(await payload())[0].elements[0].elements;
 assert.equal(nodes.some(n=>n.style?.bold),!before);
 if(!before) {
 await page.getByRole('button',{name:'Send',exact:true}).first().click();
 const sent=page.locator('.msg').filter({hasText:'bold italic strike code'}).last();
 await sent.locator('strong').waitFor(); assert.equal(await sent.locator('code').innerText(),'code');
 }
 } else if(mode==='lists') {
 for(const tag of ['ul','ol']) {
 await editor.evaluate((el,tag)=>{el.innerHTML=`<${tag}><li>first</li></${tag}>`; el.focus(); const r=document.createRange();r.selectNodeContents(el.querySelector('li'));r.collapse(false);const s=getSelection();s.removeAllRanges();s.addRange(r);},tag);
 await page.keyboard.press('Shift+Enter');
 assert.equal(await editor.locator('li').count(),before?1:2);
 if(!before) {
 await page.keyboard.type('second'); assert.equal(await editor.locator('li').nth(1).innerText(),'second');
 await page.keyboard.press('Shift+Enter'); await page.keyboard.press('Shift+Enter'); await page.keyboard.type('outside');
 assert.equal(await editor.locator('li').count(),2);
 assert.equal(await editor.locator('li').filter({hasText:'outside'}).count(),0);
 }
 }
 } else if(mode==='mentions') {
 await page.getByRole('button',{name:'Mention someone',exact:true}).first().click();
 await page.getByRole('button',{name:'＠ Olena Koval',exact:true}).click();
 const result=await editor.evaluate(async el=>{const m=await import('/src/rich-compose.ts');const p=await import('/src/people.ts');return m.blocksText(m.editorBlocks(el),p.people());});
 assert.equal(result,before?'@U1':'@Olena Koval');
 if(!before) {
 await page.getByRole('button',{name:'Send',exact:true}).first().click();
 await page.waitForTimeout(900);
 const stored=await page.evaluate(async()=>{const {api}=await import('/src/api.ts');return api.messages('C1',null,100);});
 assert.equal(stored.at(-1).text,'@Olena Koval');
 }
 } else if(mode==='selection') {
 const row=page.locator('[data-ts="1762500110.000000"]'); await row.scrollIntoViewIfNeeded();
 const prevented=await row.evaluate(el=>{
 const text=el.querySelector('.rich p').lastChild; const r=document.createRange();r.selectNodeContents(text);const s=getSelection();s.removeAllRanges();s.addRange(r);
 const event=new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:500,clientY:500});el.dispatchEvent(event);return event.defaultPrevented;
 });
 assert.equal(prevented,before);
 if(!before) {
 assert.equal(await page.getByRole('menu').count(),0);
 const other=page.locator('[data-ts="1762500100.000000"]');
 const unrelated=await other.evaluate(el=>{
 const event=new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:500,clientY:500});el.dispatchEvent(event);return event.defaultPrevented;
 });
 assert.equal(unrelated,true); await page.getByRole('menu').waitFor();
 await page.keyboard.press('Escape');
 const collapsed=await row.evaluate(el=>{getSelection().removeAllRanges();const event=new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:500,clientY:500});el.dispatchEvent(event);return event.defaultPrevented;});
 assert.equal(collapsed,true); await page.getByRole('menu').waitFor();
 }
 }
 await page.screenshot({path:`/tmp/flek-regressions/${mode}-${before?'before':'after'}.png`});
 console.log(`${mode}: ${before?'regression reproduced':'passed'}`);
 }finally{await browser.close()}
})().catch(e=>{console.error(e);process.exitCode=1});
