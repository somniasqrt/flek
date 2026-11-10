import test from "node:test";
import assert from "node:assert/strict";
import { indicator, dndUntil, visibleStatus, statusExpiration, tomorrowMinutes, type Presence } from "./presence.ts";

const p = (extra: Partial<Presence> = {}): Presence => ({
  user_id: "UFAKE", presence: "active", dnd_enabled: false, dnd_start: 0, dnd_end: 0,
  snooze_enabled: false, snooze_end: 0, manual_away: null, auto_away: null, online: null,
  connection_count: null, presence_fresh: true, dnd_fresh: true, updated_at: 100, ...extra,
});

test("presence mapping: manual away, live DND, active and unknown", () => {
  assert.equal(indicator(undefined,100),"unknown");
  assert.equal(indicator(p(),100),"active");
  assert.equal(indicator(p({presence:"away"}),100),"away");
  for (const extra of [{online:false},{auto_away:true},{connection_count:0}]) assert.equal(indicator(p(extra),100),"away");
  const dnd = p({dnd_enabled:true,dnd_start:50,dnd_end:150});
  assert.equal(indicator(dnd,49),"active"); assert.equal(indicator(dnd,50),"dnd"); assert.equal(indicator(dnd,150),"active");
  assert.equal(indicator({...dnd,manual_away:true},100),"away");
  assert.equal(indicator({...dnd,presence_fresh:false},100),"unknown");
  assert.equal(indicator({...dnd,dnd_fresh:false},100),"active");
  assert.equal(indicator(p({snooze_enabled:true,snooze_end:200}),100),"dnd");
  assert.equal(dndUntil(p({snooze_enabled:false,snooze_end:200}),100),null);
});

test("expired custom statuses disappear, including emoji-only statuses", () => {
  const status = {status_text:"Reading",status_emoji:":books:",status_expiration:200};
  assert.deepEqual(visibleStatus(status,199),status); assert.equal(visibleStatus(status,200),null);
  assert.equal(visibleStatus({...status,status_text:""},201),null);
  assert.ok(visibleStatus({...status,status_expiration:0},300));
  assert.ok(visibleStatus({...status,status_expiration:null},300));
  assert.equal(visibleStatus({...status,status_text:"",status_emoji:""},100),null);
});

test("clear after uses Unix seconds and local end of day/week", () => {
  const now = new Date(2026,10,10,14,23,45); // Tuesday, in the host's local zone.
  const seconds = Math.floor(now.getTime()/1000);
  assert.equal(statusExpiration("never",now),0);
  assert.equal(statusExpiration("30m",now),seconds+1800);
  assert.equal(statusExpiration("1h",now),seconds+3600);
  assert.equal(statusExpiration("4h",now),seconds+14400);
  const today = new Date(statusExpiration("today",now)*1000);
  assert.deepEqual([today.getDate(),today.getHours(),today.getMinutes(),today.getSeconds()],[10,23,59,59]);
  const week = new Date(statusExpiration("week",now)*1000);
  assert.deepEqual([week.getDay(),week.getDate(),week.getHours(),week.getSeconds()],[0,15,23,59]);
  assert.equal(statusExpiration("week",new Date(2026,10,15,12)),new Date(2026,10,15,23,59,59).getTime()/1000);
  const tomorrow = new Date(now.getTime()+tomorrowMinutes(now)*60000);
  assert.equal(tomorrow.getDate(),11); assert.equal(tomorrow.getHours(),9);
  assert.equal(now.getHours(),14); // Caller-owned Date is unchanged.
});
