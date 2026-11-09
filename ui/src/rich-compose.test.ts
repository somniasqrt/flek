import assert from "node:assert/strict";
import { test } from "node:test";
import { blocksText, safeLink, linkify, formatRichText } from "./rich-compose.ts";

test("rich text has a searchable plain fallback, including mentions and lists", () => {
  assert.equal(
    blocksText([
      {
        type: "rich_text",
        elements: [
          {
            type: "rich_text_section",
            elements: [
              { type: "text", text: "Hello ", style: { bold: true } },
              { type: "user", user_id: "U1" },
            ],
          },
          {
            type: "rich_text_list",
            style: "ordered",
            indent: 0,
            elements: [
              {
                type: "rich_text_section",
                elements: [{ type: "link", url: "https://example.com", text: "Example" }],
              },
            ],
          },
        ],
      },
    ]),
    "Hello @U1\nExample",
  );
});

test("editor links accept safe protocols only", () => {
  for (const url of ["https://example.com", "http://localhost:1234", "mailto:test@example.com"])
    assert.equal(safeLink(url), true);
  for (const url of [
    "javascript:alert(1)",
    "data:text/html,hello",
    "file:///etc/passwd",
    "not a url",
  ])
    assert.equal(safeLink(url), false);
});

const t = (text: string) => ({ type: "text", text });
const l = (url: string) => ({ type: "link", url });
test("linkify: URL in a sentence and trailing punctuation", () => {
  assert.deepEqual(linkify("See https://example.com/a.,;:!? now"), [t("See "), l("https://example.com/a"), t(".,;:!? now")]);
});
test("linkify: parentheses are retained only when balanced inside the URL", () => {
  assert.deepEqual(linkify("(https://example.com/a)"), [t("("), l("https://example.com/a"), t(")")]);
  assert.deepEqual(linkify("(https://example.com/a_(b))."), [t("("), l("https://example.com/a_(b)"), t(").")]);
});
test("linkify: two URLs preserve styles and surrounding text", () => {
  assert.deepEqual(linkify("https://a.example and http://b.example", { bold: true }), [
    { ...l("https://a.example"), style: { bold: true } },
    { ...t(" and "), style: { bold: true } },
    { ...l("http://b.example"), style: { bold: true } },
  ]);
});
test("linkify: code remains untouched", () => {
  assert.deepEqual(linkify("https://example.com", { code: true }), [{ ...t("https://example.com"), style: { code: true } }]);
});

const formatted = (text: string, style?: import('./rich-compose.ts').TextStyle) => formatRichText([{ type: 'rich_text', elements: [{ type: 'rich_text_section', elements: [{ type: 'text', text, ...(style ? {style} : {}) }] }] }])[0].elements;
test('typed mrkdwn: bold, italic, strike and code use the existing parser', () => {
  assert.deepEqual(formatted('*bold* _italic_ ~strike~ `code`'), [{type:'rich_text_section',elements:[
    {type:'text',text:'bold',style:{bold:true}},t(' '),
    {type:'text',text:'italic',style:{italic:true}},t(' '),
    {type:'text',text:'strike',style:{strike:true}},t(' '),
    {type:'text',text:'code',style:{code:true}},
  ]}]);
});
test('typed mrkdwn: snake_case_names, 2*3*4, URL underscores and emoji survive', () => {
  assert.deepEqual(formatted('snake_case_names 2*3*4 https://example.com/_a_b_ :smile:'), [{type:'rich_text_section',elements:[
    t('snake_case_names 2*3*4 '),l('https://example.com/_a_b_'),t(' '),{type:'emoji',name:'smile'},
  ]}]);
});
test('typed mrkdwn: fenced blocks are preformatted and never linkified', () => {
  assert.deepEqual(formatted('before\n```\nhttps://example.com *literal* & <>\n```\nafter'), [
    {type:'rich_text_section',elements:[t('before\n')]},
    {type:'rich_text_preformatted',elements:[t('https://example.com *literal* & <>')]},
    {type:'rich_text_section',elements:[t('\nafter')]},
  ]);
});
test('typed mrkdwn: literal escapes and unclosed fences stay text', () => {
  assert.deepEqual(formatted('& <@U1> &lt;tag&gt;'),[{type:'rich_text_section',elements:[t('& <@U1> &lt;tag&gt;')]}]);
  assert.deepEqual(formatted('before ``` unfinished'),[{type:'rich_text_section',elements:[t('before ``` unfinished')]}]);
});
test('typed mrkdwn: toolbar style and inline code are not parsed again', () => {
  for(const style of [{bold:true},{code:true}])
    assert.deepEqual(formatted('*literal* https://example.com',style),[{type:'rich_text_section',elements:[{...t('*literal* https://example.com'),style}]}]);
  assert.deepEqual(formatted('`https://example.com`'),[{type:'rich_text_section',elements:[{type:'text',text:'https://example.com',style:{code:true}}]}]);
});
test('typed mrkdwn: fences span DOM paragraphs, split text nodes and preserve rich neighbors', () => {
  const blocks: import('./rich-compose.ts').RichBlock[] = [{type:'rich_text',elements:[
    {type:'rich_text_section',elements:[t('```')]},
    {type:'rich_text_section',elements:[t('https://example.com')]},
    {type:'rich_text_section',elements:[t('```')]},
    {type:'rich_text_section',elements:[t('*bo'),t('ld* '),{type:'user',user_id:'U1'}]},
  ]}];
  const result=formatRichText(blocks);
  assert.equal(result[0].elements[0].type,'rich_text_preformatted');
  assert.equal(result[0].elements[0].elements[0].type,'text');
  assert.equal(JSON.stringify(result).includes('"bold":true'),true);
  assert.equal(JSON.stringify(result).includes('"user_id":"U1"'),true);
});
