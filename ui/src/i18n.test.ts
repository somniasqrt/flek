import assert from "node:assert/strict";
import { test } from "node:test";
import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import { en } from "./i18n/en.ts";
import { uk } from "./i18n/uk.ts";
import { errorMessage, interpolate, plural, resolveLocale, translate } from "./i18n/index.ts";
import { renderRichMessage } from "./i18n/rich-message.ts";
import { activityDay, listTime, localTime, relativeTime } from "./time.ts";

// TypeScript also enforces exact dictionary keys in uk.ts via `satisfies`.
test("dictionaries preserve keys, interpolation parameters and literal values", () => {
  assert.deepEqual(Object.keys(uk).sort(), Object.keys(en).sort());
  for (const key of Object.keys(en) as (keyof typeof en)[]) {
    assert.equal(key, key.trim(), `key whitespace: ${JSON.stringify(key)}`);
    assert.equal(en[key], en[key].trim(), `English whitespace: ${key}`);
    assert.equal(uk[key], uk[key].trim(), `Ukrainian whitespace: ${key}`);
    const params = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
    assert.deepEqual(params(uk[key]), params(en[key]), key);
  }
  assert.equal(interpolate("Hello {name}, {n}!", { name: "$& {n}", n: 2 }), "Hello $& {n}, 2!");
  assert.equal(interpolate("{missing}"), "{missing}");
  assert.equal(translate("uk", "Search {name}", { name: "Demo" }), "Пошук у Demo");
  assert.equal(resolveLocale("system", "uk-UA"), "uk");
  assert.equal(resolveLocale("system", "fr"), "en");
  assert.equal(resolveLocale("en", "uk"), "en");
  assert.equal(
    errorMessage("Slack API error: not_in_channel"),
    "Something went wrong (not_in_channel)",
  );
  assert.equal(errorMessage("a private path or arbitrary server body"), "Something went wrong");
});

test("Ukrainian plural categories include teen exceptions and compound counts", () => {
  for (const [n, expected] of [
    [1, "1 учасник"],
    [2, "2 учасники"],
    [5, "5 учасників"],
    [11, "11 учасників"],
    [21, "21 учасник"],
    [22, "22 учасники"],
    [25, "25 учасників"],
  ] as const)
    assert.equal(plural("members", n, "uk"), expected);
  assert.equal(plural("members", 1.5, "uk"), "1.5 учасника");
  assert.equal(plural("replies", 2, "uk"), "2 відповіді");
  assert.equal(plural("results", 1, "en"), "1 result");
  assert.equal(plural("results", 5, "en"), "5 results");
  assert.equal(plural("pixels", 1, "uk"), "1 піксель");
  assert.equal(plural("pixels", 2, "uk"), "2 пікселі");
  assert.equal(plural("pixels", 5, "uk"), "5 пікселів");
});

test("relative and calendar times in both languages and both clock formats", () => {
  const now = new Date(2026, 10, 10, 15, 30);
  const recent = new Date(now.getTime() - 10 * 60000);
  assert.equal(listTime(recent, now, "en"), "10 mins");
  assert.equal(listTime(recent, now, "uk"), "10 хв");
  assert.equal(relativeTime(recent, now, "uk"), "10 хвилин тому");
  assert.equal(relativeTime(new Date(now.getTime() - 1000), now, "uk"), "щойно");
  assert.equal(activityDay(new Date(2026, 10, 9), now, "uk"), "Вчора");
  assert.equal(activityDay(new Date(2026, 10, 9), now, "en"), "Yesterday");
  assert.equal(activityDay(new Date(2026, 10, 8), now, "uk"), "неділя");
  const afternoon = new Date(2026, 10, 10, 13, 5);
  assert.equal(relativeTime(afternoon, now, "uk", false), "сьогодні о 13:05");
  assert.match(relativeTime(afternoon, now, "en", true), /01:05 PM/);
  assert.equal(localTime(new Date(Date.UTC(2026, 10, 10, 13, 5)), "UTC", 0, "uk", false), "13:05");
});

function fragmentedSiblings(node: ts.Node): boolean {
  if (!ts.isJsxElement(node) && !ts.isJsxFragment(node)) return false;
  const children = node.children.filter((child) => !ts.isJsxText(child) || !!child.text.trim());
  const isTranslation = (child: ts.JsxChild) => {
    if (!ts.isJsxExpression(child) || !child.expression) return false;
    const translated = (expr: ts.Expression): boolean => {
      if (ts.isCallExpression(expr)) return expr.expression.getText() === "t";
      if (ts.isConditionalExpression(expr)) {
        return translated(expr.whenTrue) || translated(expr.whenFalse);
      }
      if (ts.isParenthesizedExpression(expr)) return translated(expr.expression);
      return false;
    };
    return translated(child.expression);
  };
  const inline = new Set(["b", "strong", "em", "i", "kbd", "code", "span"]);
  let previous = -1;
  for (let i = 0; i < children.length; i++) {
    const child = children[i];
    if (isTranslation(child)) {
      if (previous >= 0) return true;
      previous = i;
    } else if (ts.isJsxElement(child) && !inline.has(child.openingElement.tagName.getText())) {
      previous = -1;
    } else if (ts.isJsxSelfClosingElement(child) && child.tagName.getText() !== "Emoji") {
      previous = -1;
    }
  }
  return false;
}

test("fragment scanner rejects translations glued around variables or emphasis", () => {
  for (const jsx of [
    '<p>{t("Before")}{name}{t("after")}</p>',
    '<p>{t("Before")}<strong>{name}</strong>{t("after")}</p>',
    '<p>{t("Before")}<Emoji name={emoji} />{t("after")}</p>',
    '<p>{t("Before")}{" "}{t("after")}</p>',
  ]) {
    const ast = ts.createSourceFile("fixture.tsx", jsx, 99, true, ts.ScriptKind.TSX);
    const expression = (ast.statements[0] as ts.ExpressionStatement).expression;
    assert.equal(fragmentedSiblings(expression), true, jsx);
  }
  const ast = ts.createSourceFile(
    "fixture.tsx",
    '<div><button>{t("Cancel")}</button><button>{t("Open")}</button></div>',
    99,
    true,
    ts.ScriptKind.TSX,
  );
  assert.equal(fragmentedSiblings((ast.statements[0] as ts.ExpressionStatement).expression), false);
});

test("rich messages allow translated order and nesting while params stay literal text", () => {
  type Element = { tag: string; children: Array<string | Element> };
  const tags = {
    b: (children: Array<string | Element>): Element => ({ tag: "strong", children }),
    kbd: (children: Array<string | Element>): Element => ({ tag: "kbd", children }),
  };
  const name = "<script>alert(1)</script><b>name</b>{other}";
  assert.deepEqual(renderRichMessage("<b>{name}</b>!", { name }, tags), [
    { tag: "strong", children: [name] },
    "!",
  ]);
  assert.deepEqual(renderRichMessage("<b>Press <kbd>{key}</kbd></b>", { key: "Enter" }, tags), [
    { tag: "strong", children: ["Press ", { tag: "kbd", children: ["Enter"] }] },
  ]);
  for (const markup of ["<img src=x onerror=alert(1)>", '<b onclick="x">text</b>', "<b>unclosed"]) {
    assert.deepEqual(renderRichMessage(markup, {}, tags), [markup]);
  }
  assert.deepEqual(renderRichMessage("<script>{name}</script>", { name }, tags), [
    "<script>",
    name,
    "</script>",
  ]);
  const key = "This is the start of your conversation with <b>{name}</b>.";
  assert.deepEqual(renderRichMessage(translate("uk", key), { name: "Olena" }, tags), [
    "Початок розмови: ",
    { tag: "strong", children: ["Olena"] },
    ".",
  ]);
  assert.deepEqual(renderRichMessage("Hello {name}", { name: "$& <kbd>X</kbd>" }, tags), [
    "Hello $& <kbd>X</kbd>",
  ]);
});

test("JSX copy and accessibility attributes cannot bypass translation", () => {
  const root = fileURLToPath(new URL(".", import.meta.url));
  // Technical notation, product name and physical-key names are intentionally literal.
  const allowed = new Set(["Flek", "URL", "PDF", "MB", "R", "T", "L", "Enter", "Aa"]);
  const failures: string[] = [];
  for (const file of readdirSync(root, { recursive: true }) as string[]) {
    if (!file.endsWith(".tsx") || file === "preview.tsx") continue; // synthetic user content
    const source = ts.createSourceFile(
      file,
      readFileSync(root + file, "utf8"),
      99,
      true,
      ts.ScriptKind.TSX,
    );
    const report = (node: ts.Node, value: string) => {
      const text = value.trim();
      if (!/\p{L}/u.test(text) || allowed.has(text)) return;
      const line = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
      failures.push(`${file}:${line}: ${text}`);
    };
    const walk = (node: ts.Node) => {
      if (ts.isJsxText(node)) report(node, node.text);
      if (fragmentedSiblings(node)) {
        const line = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
        failures.push(`${file}:${line}: adjacent translations must be one message`);
      }
      if (
        ts.isJsxAttribute(node) &&
        ["aria-label", "title", "placeholder"].includes(node.name.getText(source))
      ) {
        if (node.initializer && ts.isStringLiteral(node.initializer))
          report(node, node.initializer.text);
        if (
          node.initializer &&
          ts.isJsxExpression(node.initializer) &&
          node.initializer.expression
        ) {
          const expr = node.initializer.expression;
          if (ts.isStringLiteral(expr) || ts.isNoSubstitutionTemplateLiteral(expr))
            report(expr, expr.text);
          if (ts.isTemplateExpression(expr)) report(expr, expr.head.text);
        }
      }
      ts.forEachChild(node, walk);
    };
    walk(source);
  }
  assert.deepEqual(failures, []);
});
