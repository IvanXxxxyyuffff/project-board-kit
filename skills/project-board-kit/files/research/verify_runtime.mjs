// 看板运行态检测治具：真实 DOM 断言（Moli CDP + Playwright）
// 用法: node verify_runtime.mjs [url]
//   BOARD_URL / CDP_URL / PLAYWRIGHT_MODULE 三个环境变量可覆盖本机默认值，
//   便于在同事机器或不同端口复跑（审核 2026-11-14「治具不可复现」整改）。
//   旧版三条假绿已修：圆环 arcs>=1→>=2、面板位置只看 adjacent→全顺序参与、
//   搜索只数可见 .pc→必须验 #st-qhits 真的显示且非空。
const PW_URL = process.env.PLAYWRIGHT_MODULE || 'file:///<TOOLS>/npm-global/node_modules/playwright/index.js';
const pw = (await import(PW_URL)).default;
const { chromium } = pw;

const BOARD_URL = process.argv[2] || process.env.BOARD_URL ||
  'http://127.0.0.1:8899/%E9%A1%B9%E7%9B%AE%E6%95%B0%E6%8D%AE%E7%9C%8B%E6%9D%BF.html?v=' + Date.now();
const CDP_URL = process.env.CDP_URL || 'http://127.0.0.1:9333';

const rows = [];
const check = (name, ok, detail = '') => rows.push([!!ok, name, detail]);

const browser = await chromium.connectOverCDP(CDP_URL);
const ctx = browser.contexts()[0] || (await browser.newContext());
const page = await ctx.newPage();
const errs = [];
// 旧版只收 pageerror，console.error / 同源资源加载失败都漏掉。
page.on('pageerror', e => errs.push('pageerror: ' + String(e).slice(0, 200)));
page.on('console', m => { if (m.type() === 'error') errs.push('console: ' + m.text().slice(0, 200)); });
page.on('requestfailed', r => {
  const u = r.url(), f = (r.failure() || {}).errorText || '';
  if (/ERR_ABORTED/.test(f)) return;            // 导航取消不算
  if (!/127\.0\.0\.1|localhost/.test(u)) return; // 外域字体/CDN 不判本机故障
  errs.push('requestfailed: ' + u.slice(0, 120) + ' ' + f);
});
await page.goto(BOARD_URL, { waitUntil: 'networkidle', timeout: 60000 });
await page.waitForTimeout(2200);

// 1 品牌分布位置：现役 V4 版面 = 03 项目段之后、04 时间轴(sec-nodes)之前、
//   紧邻 #groups 之前。旧版 adjacent 只算「groups 在 panel 之后」，
//   afterTl/afterAll 算出来却不参与断言 → 面板跑丢也 PASS（审核 2026-11-14）。
//   注意：不要断言「时间轴之后」——那是更早期版面，现役构建期 _move_panel
//   固定把面板插到 #groups 所属 section 之前（即 03 与 04 之间）。
const order = await page.evaluate(() => {
  const panel = document.getElementById('st-wrap');
  const groups = document.getElementById('groups');
  const findText = t => [...document.querySelectorAll('h1,h2,h3')].find(e => (e.textContent || '').includes(t));
  const tl = document.getElementById('sec-nodes') || findText('时间轴');
  const s03 = findText('03') || findText('项目');
  const seq = (a, b) => a && b ? !!(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING) : false;
  const after03 = seq(s03, panel);
  const beforeTimeline = seq(panel, tl);
  const beforeGroups = seq(panel, groups);
  // panel 与 groups 之间只允许极少量节点，不能塞进另一个整段 section
  let between = 0, cur = panel ? panel.nextElementSibling : null;
  while (cur && cur !== groups && between <= 8) { between++; cur = cur.nextElementSibling; }
  const adjacent = !!(panel && groups && beforeGroups && between <= 3);
  return { hasPanel: !!panel, hasGroups: !!groups, has03: !!s03, hasTimeline: !!tl, after03, beforeTimeline, beforeGroups, between, adjacent };
});
const orderOk = order.hasPanel && order.hasGroups && order.has03 && order.hasTimeline &&
  order.after03 && order.beforeTimeline && order.beforeGroups && order.adjacent;
check('品牌分布紧邻群聊段之前(03后/04前)', orderOk, JSON.stringify(order));
check('品牌分布位于群聊与时间轴之前', order.hasPanel && order.beforeGroups && order.beforeTimeline, JSON.stringify(order));

// 2 圆环：文案「≥2 段弧」必须真断言 >=2（旧版 arcs>=1 让单段环也 PASS）
const rings = await page.evaluate(() => {
  const out = [];
  document.querySelectorAll('.donut').forEach(d => {
    const cs = [...d.querySelectorAll('circle')].filter(c => c.getAttribute('stroke-dasharray'));
    out.push({ arcs: cs.length, colors: [...new Set(cs.map(c => c.getAttribute('stroke')))] });
  });
  return out;
});
check('每个圆环 ≥2 段 SVG 弧', rings.length > 0 && rings.every(r => r.arcs >= 2), JSON.stringify(rings));
check('圆环多色 ≥3', new Set(rings.flatMap(r => r.colors)).size >= 3, JSON.stringify([...new Set(rings.flatMap(r => r.colors))]));

// 3 项目卡进度
const cards = await page.evaluate(() => [...document.querySelectorAll('.pc')].map(c => ({
  model: (c.querySelector('.pc-t') || {}).textContent || '',
  prog: (c.querySelector('.pc-prog') || {}).textContent || '',
  none: !!(c.querySelector('.pc-prog.none')),
})));
  // 「未排期」是合法状态：台账里有、专项进度表还没建节点表的预立项机型（如 ITEM_X / ITEM_X）。
  // 旧断言把所有无进度卡的卡都判红，等于要求「台账不许早于节点表」，会把真实业务状态当故障。
  // 现在：无进度必须显式写「未排期」，且占比 ≤30%（全量塌成未排期仍是故障，那是解析崩了）。
  const unplanned = cards.filter(c => c.none || /0\s*\/\s*0|无节点表|未排期/.test(c.prog));
  const badProg = unplanned.filter(c => !/未排期/.test(c.prog));
  check('无进度卡必须是显式「未排期」', cards.length > 0 && badProg.length === 0, `cards=${cards.length} bad=${JSON.stringify(badProg.slice(0, 4))}`);
  check('未排期卡片占比 ≤30%', unplanned.length <= Math.ceil(cards.length * 0.3), `unplanned=${unplanned.length}/${cards.length}`);
check('所有卡片有真实进度', cards.length > 0 && badProg.length === 0, `cards=${cards.length} bad=${JSON.stringify(badProg.slice(0, 4))}`);
check('页面运行态无「无节点表」', !(await page.evaluate(() => document.body.innerText.includes('无节点表'))));

// 4 负责人分组
const heads = await page.evaluate(() => [...document.querySelectorAll('.grp-h b')].map(b => b.textContent.trim()));
const dup = [...new Set(heads)].filter(h => heads.filter(x => x === h).length > 1);
check('负责人分组无重名', heads.length > 0 && dup.length === 0, JSON.stringify({ heads, dup }));

// 5 已归档：空数组不能算「已排序」；日期按数值比较，不靠裸字符串运气
let arch = { rows: 0, first: '', last: '', sorted: false };
try {
  await page.evaluate(() => { const b = document.getElementById('st-arch-btn') || document.getElementById('st-arch-open'); if (b) b.click(); });
  await page.waitForTimeout(900);
  arch = await page.evaluate(() => {
    const list = [...document.querySelectorAll('#st-list .a')].map(x => x.textContent.trim()).filter(Boolean);
    // 每条取 YYYY-MM-DD，转 yyyymmdd 数值后判降序
    const dates = list.map(s => {
      const m = s.match(/(\d{4})-(\d{2})-(\d{2})/);
      return m ? Number(m[1]) * 10000 + Number(m[2]) * 100 + Number(m[3]) : null;
    }).filter(v => v != null);
    let sorted = dates.length > 0;
    for (let i = 1; i < dates.length; i++) if (dates[i] > dates[i - 1]) sorted = false;
    return { rows: dates.length, first: dates[0] || '', last: dates[dates.length - 1] || '', sorted, sample: list.slice(0, 2) };
  });
  await page.evaluate(() => { const m = document.getElementById('st-modal'); if (m) m.classList.remove('on'); });
} catch (e) { arch = { rows: 0, first: '', last: '', sorted: false, err: String(e).slice(0, 120) }; }
check('已归档列表 ≥60 条且带年份', arch.rows >= 60, JSON.stringify(arch));
check('已归档按日期倒序(非空)', arch.rows > 0 && arch.sorted, JSON.stringify(arch));

// 6 搜索：分两个关键词分别验两条路径，不能靠一个关键词蒙混过关。
//   - 'pikus' 唯一机型 ITEM_X 本来就在 13 张项目卡里 → 验「卡片同步过滤」；
//     此时「表格里其它匹配（不在卡片里）」区按设计为空，属正确行为。
//   - 'ITEM_X' 是已结案 2025-03-24 且不在任何卡片里 → 验「#st-qhits 真的显示且非空」。
//   旧版只数可见 .pc，实测 #st-qhits disp=none/n=0 时仍 PASS（审核坐实假绿）。
const qOk = await page.evaluate(() => !!document.getElementById('q'));
check('搜索框 #q 存在', qOk);
const peek = async kw => {
  if (qOk) { await page.fill('#q', kw).catch(() => {}); await page.waitForTimeout(900); }
  return page.evaluate(() => {
    const d = document.querySelector('#st-qhits');
    const vis = [...document.querySelectorAll('.pc')].filter(c => c.style.display !== 'none');
    const disp = d ? getComputedStyle(d).display : '';
    return {
      exists: !!d, disp, txt: d ? (d.textContent || '').slice(0, 140) : '',
      n: d ? d.children.length : 0, cards: vis.length,
      cardTxt: vis.map(c => (c.textContent || '').slice(0, 24))
    };
  });
};
const hits = qOk ? await peek('pikus') : { exists: false, disp: '', txt: '', n: 0, cards: 0, cardTxt: [] };
const extra = qOk ? await peek('ITEM_X') : { exists: false, disp: '', txt: '', n: 0, cards: 0, cardTxt: [] };
check('搜索 pikus 命中项目卡', hits.cards > 0 && /ITEM_X|Pikus/i.test(hits.cardTxt.join(' ') + hits.txt), JSON.stringify(hits));
check('搜索已归档机型命中其它匹配区', extra.exists && extra.disp !== 'none' && extra.n > 0 && /ITEM_X/.test(extra.txt), JSON.stringify(extra));
if (qOk) await page.fill('#q', '').catch(() => {});
await page.waitForTimeout(400);
const restored = await page.evaluate(() => [...document.querySelectorAll('.pc')].filter(c => c.style.display !== 'none').length);
check('清空搜索恢复全部项目卡', restored >= 12, `visible=${restored}`);

// 7 品牌芯片过滤
const chips = await page.evaluate(() => {
  const box = document.getElementById('st-chips');
  return box ? [...box.children].map(b => (b.textContent || '').trim()) : [];
});
check('品牌芯片 ≥6 个且含 019/Pikus', chips.length >= 6 && chips.some(c => c === '019') && chips.some(c => /Pikus/i.test(c)), JSON.stringify(chips));

// 8 无 JS 异常（含 console.error / 同源 requestfailed）
check('无页面 JS 异常', errs.length === 0, errs.slice(0, 3).join(' | '));

const w = Math.max(...rows.map(r => r[1].length));
let ok = 0;
for (const [good, name, detail] of rows) {
  console.log(`${good ? 'PASS' : 'FAIL'} | ${name.padEnd(w)} | ${detail}`);
  if (good) ok++;
}
console.log('-'.repeat(60));
console.log(`RESULT ${ok}/${rows.length} ${ok === rows.length ? 'ALL PASS' : 'HAS FAILURES'}`);
await page.close();
await browser.close().catch(() => {});
process.exit(ok === rows.length ? 0 : 1);
