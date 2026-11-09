// Run against the local preview only. PLAYWRIGHT_MODULE points to an installed Playwright.
const { webkit } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
(async () => {
 const browser = await webkit.launch({headless:true});
 try {
 const page = await browser.newPage({viewport:{width:1440,height:1000}});
 const errors=[]; page.on('pageerror', e=>errors.push(e.message));
 await page.route('**/*', route => new URL(route.request().url()).hostname === '127.0.0.1' ? route.continue() : route.abort());
 await page.goto('http://127.0.0.1:5174/preview.html');
 const row = page.locator('[data-ts="1762500100.000000"]');
 await row.waitFor(); await row.scrollIntoViewIfNeeded();
 const comparison = page.locator('[data-ts="1762500110.000000"]');
 const phase=process.argv[2] || 'after';
 fs.mkdirSync('/tmp/flek-regressions',{recursive:true});
 await page.screenshot({path:`/tmp/flek-regressions/received-${phase}.png`});
 const signatures=async r=>r.locator('.rich').evaluate(el=>({links:[...el.querySelectorAll('a')].map(a=>({class:a.className,text:a.textContent,href:a.getAttribute('href')})),mention:el.querySelector('.mention')?.textContent,emoji:el.querySelector('[title*="wave"]')?.textContent}));
 const actual=await signatures(row), expected=await signatures(comparison);
 console.log({actual,expected});
 if(phase==='before') assert.notDeepEqual(actual.links,expected.links);
 else {
 assert.deepEqual(actual,expected);
 assert.equal(await row.locator('u strong').innerText(),'bold underline');
 assert.equal(await page.locator('[data-ts="1762500120.000000"] .rich-blocks').count(),0);
 }
 assert.deepEqual(errors,[]);
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1});
