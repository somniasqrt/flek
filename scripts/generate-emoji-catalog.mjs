// Run after updating emojibase-data: node scripts/generate-emoji-catalog.mjs
import { readFileSync, writeFileSync } from "node:fs";
const read = name => JSON.parse(readFileSync(new URL(`../node_modules/emojibase-data/en/${name}.json`, import.meta.url), "utf8"));
const names = read("shortcodes/iamcal");
const groups = {0:"people",1:"people",3:"nature",4:"food",5:"travel",6:"activities",7:"objects",8:"symbols",9:"flags"};
const catalog = read("data").filter(e => names[e.hexcode] && groups[e.group]).sort((a,b)=>a.order-b.order).map(e => {
 const aliases = [names[e.hexcode]].flat();
 const tones = [1,2,3,4,5].map(tone => e.skins?.find(s => s.tone === tone)?.emoji);
 return { name: aliases[0], label:e.label, group:groups[e.group], terms:[...aliases,...e.tags ?? []].join(" "), ...(tones.every(Boolean) ? {tones} : {}) };
});
writeFileSync(new URL("../ui/src/emoji-catalog.json", import.meta.url), JSON.stringify(catalog));
console.log(`${catalog.length} emoji written`);
