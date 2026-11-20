// 每日项目简报生成器 —— 综合微信表格（实时/全页签）+ E-Mobile 群聊，产出单文件 HTML
// 用法: node build_daily_brief.mjs
// 数据源缺失时自动降级：任意一步抓不到，简报仍出，并在页面顶部标注。
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// ROOT 自动探测：本文件位于 <ROOT>/research/ 下，不再写死绝对路径
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const WX = path.join(ROOT, 'research', 'wxdata');
const EM = path.join(ROOT, 'research', 'raw', 'emobile');
const RS = path.join(ROOT, 'research');
const MIL_LIB = fs.readFileSync(path.join(RS, 'lib', 'milestone_view.js'), 'utf8');

// 页面样式与运行时脚本放在 research/page/ 下：模板字符串里的 JS 在 node 期零校验，
// 拆成独立文件后可以 node --check、可以格式化，改样式不用在 2000 行里找。
const PAGE_DIR = path.join(RS, 'page');
const pageCss = () => fs.readFileSync(path.join(PAGE_DIR, 'board.css'), 'utf8');
const MIL_MARK = '/*__MIL_LIB__*/';
const pageJs = () => {
  const src = fs.readFileSync(path.join(PAGE_DIR, 'board.js'), 'utf8');
  if (src.indexOf(MIL_MARK) < 0) throw new Error('research/page/board.js 缺 ' + MIL_MARK + ' 占位符：里程碑库注入会静默失效');
  // 函数式替换：库体里出现 $& / $1 之类不会被当替换模式吃掉
  const out = src.replace(MIL_MARK, () => MIL_LIB);
  if (out.length <= src.length) throw new Error('里程碑库注入后长度没有变化，替换未生效');
  return out;
};   // 里程碑归一/判状态的唯一实现，生成时内联
const OUT_HTML = path.join(ROOT, '最新项目简报.html');
const OUT_MD = path.join(ROOT, '最新项目简报.md');

const read = (p, d = '') => { try { return fs.readFileSync(p, 'utf8'); } catch (e) { return d; } };
const readJson = (p, d = null) => { try { return JSON.parse(fs.readFileSync(p, 'utf8')); } catch (e) { return d; } };

// ---------- 业务规则外置：research/brief_rules.json（缺失时用同值默认） ----------
const DEFAULT_RULES = { columns: { model: 2, brand: 3, kind: 4, status: 6, tier: 7, pm: 9 }, noGroupOwner: 'OWNER_A', ownerAliasStrip: '[A-Za-z.\\s]+$' };
const RULES = (() => {
  const j = readJson(path.join(RS, 'brief_rules.json'), null) || {};
  return { ...DEFAULT_RULES, ...j, columns: { ...DEFAULT_RULES.columns, ...(j.columns || {}) } };
})();
// 尾部 ASCII 别名（如「夏宇凡 OWNER_A」的「 OWNER_A」）剥离正则，来自外置规则
const OWNER_ALIAS_RE = (() => { try { return new RegExp(RULES.ownerAliasStrip); } catch (e) { return /[A-Za-z.\s]+$/; } })();
// 负责人归一 key：去掉 ASCII 字母/点/空白，保留 CJK 与数字，转小写；
// 结果为空（纯英文名）则回退原串去空白小写。用于把「夏宇凡 OWNER_A」与「夏宇凡」并成一组。
const ownerCanon = s => {
  const raw = String(s || '').trim();
  const c = raw.replace(OWNER_ALIAS_RE, '').replace(/[A-Za-z.\s]+/g, '').toLowerCase();
  return c || raw.replace(/\s+/g, '').toLowerCase();
};
const esc = s => String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const clip = (s, n) => { const t = String(s ?? '').replace(/\s+/g, ' ').trim(); return t.length > n ? t.slice(0, n) + '…' : t; };
const mtime = p => { try { const s = fs.statSync(p); return s.mtime.toISOString().slice(0, 16).replace('T', ' '); } catch (e) { return '缺失'; } };

// ---------- 时间（Asia/Shanghai） ----------
const NOW = new Date();
const SH = new Date(NOW.getTime() + 8 * 3600 * 1000);
const TODAY = SH.toISOString().slice(0, 10);
const STAMP = SH.toISOString().slice(0, 16).replace('T', ' ');
// E-Mobile 最后同步时间与群总数（用于「沉默群」口径与数据行标注）
const EM_STAT = (() => { try { return JSON.parse(fs.readFileSync(path.join(EM, 'em_full_pull_stats.json'), 'utf8')); } catch (e) { return {}; } })();
const localHM = v => {
  const s = String(v || '');
  const d = new Date(s);
  if (isNaN(d.getTime())) return s.replace('T', ' ').slice(11, 16);
  const p = n => String(n).padStart(2, '0');
  return p(d.getHours()) + ':' + p(d.getMinutes());
};
const EM_AT = localHM(EM_STAT.at);
const EM_GROUPS = Number(EM_STAT.groups || 0);

// 数据实际抓取时间（与“生成时间”区分，避免重建页面让人误以为刚抓过）
const DATA_AT = (() => {
  for (const f of ['数据完整性.json', 'raw/emobile/em_full_pull_stats.json']) {
    try {
      const j = JSON.parse(fs.readFileSync(path.join(RS, f), 'utf8'));
      const v = j.at || '';
      if (v) { const d = new Date(v); if (!isNaN(d.getTime())) { const p = n => String(n).padStart(2, '0'); return d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate()) + ' ' + p(d.getHours()) + ':' + p(d.getMinutes()); } return v.replace('T', ' ').slice(0, 16); }
    } catch (e) {}
  }
  return '';
})();
const addDays = (iso, n) => { const d = new Date(iso + 'T00:00:00Z'); d.setUTCDate(d.getUTCDate() + n); return d.toISOString().slice(0, 10); };
const WIN_START = TODAY;
const WIN_END = addDays(TODAY, 13);
const CHAT_SINCE = addDays(TODAY, -7);
// Excel serial -> 日期
const ymd = n => { if (n === null || n === undefined || n === '') return ''; if (typeof n === 'string') { const m = n.match(/^(\d{4})-(\d{2})-(\d{2})/); if (m) return m[1] + '-' + m[2] + '-' + m[3]; } const v = Number(n); if (!isFinite(v) || v < 20000) return ''; const d = new Date(Date.UTC(1899, 11, 30) + v * 86400000); return isNaN(d.getTime()) ? '' : d.toISOString().slice(0, 10); };
const md = n => { if (typeof n === 'string') { const g = n.match(/^(\d{4})-(\d{1,2})-(\d{2})/); if (g) return String(Number(g[2])) + '/' + String(Number(g[3])); } const v = Number(n); if (!isFinite(v)) return ''; const d = new Date(Date.UTC(1899, 11, 30) + v * 86400000); if (isNaN(d.getTime())) return ''; const mm = String(d.getUTCMonth() + 1); const dd = String(d.getUTCDate()); return mm + '/' + dd; };
const num = s => { const m = String(s ?? '').match(/^(\d{4,5})/); return m ? Number(m[1]) : null; };

const warnings = [];

// ---------- 1. 微信表格：结构化汇总（带行列坐标） ----------
function parseSummary(file, tag) {
  const txt = read(path.join(WX, file));
  if (!txt) { warnings.push(`${tag} 汇总表缺失（${file}）`); return []; }
  const out = []; let cur = null; let HDR = null;
  for (const line of txt.split(/\r?\n/)) {
    const m = line.match(/^R(\d+)\s*\|\s?([\s\S]*)$/);
    if (!m) continue;
    const cols = m[2].split(/\s*\|\s*/);
    // 表头自适应：不同页签列顺序不同（有的「里程碑」在前，有的多一列「节点用时」）
    if (cols.some(c => /关键节点|里程碑/.test(c)) && cols.some(c => /现状计划|计划完成|初始计划/.test(c))) {
      const ix = (re) => cols.findIndex(c => re.test(c));
      HDR = { node: ix(/关键节点|里程碑/), plan: ix(/现状计划|计划完成|计划/), actual: ix(/实际完成|实际/), start: ix(/开始时间|开始/) };
    }
    const c0 = (cols[0] || '').trim(), c1 = (cols[1] || '').trim();
    // 注意：机型可能以 4 位数字开头（ITEM_X / ITEM_X），只能排除「46346(11/20)」这类 Excel serial
    const isStart = /^\d{1,2}$/.test(c0) && c1 && /^[A-Za-z0-9]/.test(c1) && !/^\d{4,5}\(/.test(c1);
    if (isStart) {
      cur = {
        tag, model: c1,
        brand: (cols[3] || '').trim(),
        tier: (cols[4] || '').trim(),
        status: (cols[5] || '').trim(),
        current: (cols[11] || '').trim(),
        history: (cols[12] || '').trim(),
        brandExtra: [], milestones: [], raw: []
      };
      out.push(cur);
    }
    if (!cur) continue;
    cur.raw.push(line);
    const iN = (HDR && HDR.node >= 0) ? HDR.node : 6;
    const iP = (HDR && HDR.plan >= 0) ? HDR.plan : 9;
    const iA = (HDR && HDR.actual >= 0) ? HDR.actual : 10;
    const iS = (HDR && HDR.start >= 0) ? HDR.start : 7;
    const node = (cols[iN] || '').trim();
    if (node && !/^(序|机型|关键节点|里程碑|产品)/.test(node) && !/^\d{1,2}$/.test(node)) {
      cur.milestones.push({ name: node, start: num(cols[iS]), plan: num(cols[iP]), actual: num(cols[iA]) });
    }
    const b = (cols[3] || '').trim();
    if (b && !/^\d/.test(b) && b !== cur.brand && !cur.brandExtra.includes(b)
      && !/^(深圳市|2026年|\+--)/.test(b) && b.length < 24) cur.brandExtra.push(b);
  }
  if (!out.length) warnings.push(`${tag} 汇总表解析为空（${file}）`);
  return out;
}

// ---------- 2. 全 19 页签最新更新摘录（扁平 harvest，补结构化之外的页签） ----------
function httpUpdates() {
  const txt = read(path.join(WX, 'ALL_HTTP_TEXT.txt'));
  if (!txt) { warnings.push('ALL_HTTP_TEXT.txt 缺失（全页签更新摘录不可用）'); return []; }
  const lines = txt.split(/\r?\n/);
  const segs = []; let cur = null;
  for (const l of lines) {
    const h = l.match(/^=====\s*(.+?)\s*=====$/);
    if (h) { cur = { name: h[1], items: [] }; segs.push(cur); continue; }
    if (!cur) continue;
    const m = l.match(/^\d{4}\s{2}(.*)$/);
    if (!m) continue;
    const t = m[1].trim();
    // 更新可能出现在行首（结构化导出），也可能夹在扁平 harvest 的长行中间（cookie 直连），两种都接
    const up = t.match(/(\d{1,2})\/(\d{1,2})\s*更新[:：]?\s*([\s\S]*)$/);
    if (up) cur.items.push({ date: up[1] + '/' + up[2], head: (up[1] + '/' + up[2] + '更新：' + up[3]).trim(), body: '' });
    else if (cur.items.length && cur.items[cur.items.length - 1].body.length < 220) {
      const last = cur.items[cur.items.length - 1];
      if (/^[\d１-９、.\-]|^[A-Za-z\u4e00-\u9fa5]/.test(t) && t.length > 3 && !/^[A-Za-z0-9_]{8,}$/.test(t))
        last.body += (last.body ? ' ' : '') + t;
    }
  }
  const flat = [];
  for (const s of segs) for (const it of s.items) {
    const [mm, dd] = it.date.split('/').map(Number);
    // 只保留 11 月及以后的近期更新（跨年 M/D 简化处理）
    if (mm >= 10) flat.push({ seg: s.name, date: it.date, ord: mm * 100 + dd, text: clip(it.head + ' ' + it.body, 200) });
  }
  flat.sort((a, b) => b.ord - a.ord);
  return flat.slice(0, 14);
}

// ---------- 3. E-Mobile 群聊 ----------
function chatRecent() {
  const d = readJson(path.join(EM, 'project_chatlog.json'), null);
  if (!d) { warnings.push('project_chatlog.json 缺失（群聊动态不可用）'); return { groups: [], total: 0, range: '' }; }
  let total = 0; let oldest = '9999', newest = '0000';
  const groups = [];
  for (const g of d) {
    const msgs = (g.messages || []).filter(m =>
      m.time && m.time.slice(0, 10) >= CHAT_SINCE && m.text && !/^\[图片|^\[语音|^\[文件/.test(m.text)
    ).map(m => ({ time: m.time, sender: m.sender, text: m.text }));
    total += (g.messages || []).length;
    if (g.oldest && g.oldest < oldest) oldest = g.oldest;
    if (g.newest && g.newest > newest) newest = g.newest;
    if (msgs.length) groups.push({ group: g.group, count: g.count, msgs: msgs.sort((a, b) => a.time.localeCompare(b.time)) });
  }
  groups.sort((a, b) => (b.msgs[b.msgs.length - 1].time || '').localeCompare(a.msgs[a.msgs.length - 1].time || ''));
  return { groups, total, range: `${oldest} ~ ${newest}` };
}

// ---------- 3b. E-Mobile 个人会话（项目相关） ----------
function privRecent() {
  const d = readJson(path.join(EM, 'personal_chatlog.json'), null);
  if (!d || !d.length) return { convs: [], total: 0 };
  const convs = d.map(g => {
    const msgs = (g.messages || []).filter(m => m.text && !/^\[图片|^\[语音|^\[文件/.test(m.text))
      .slice(-4).map(m => ({ time: m.time, sender: String(m.sender || '').replace(/\(.*?\)/, ''), text: m.text, hit: m.hit || [] }));
    return { who: g.group, count: g.count, total: g.total || g.count, newest: g.newest || '', score: g.score || 0, msgs };
  }).filter(c => c.msgs.length);
  convs.sort((a, b) => (b.newest || '').localeCompare(a.newest || '') || b.score - a.score);
  return { convs, total: convs.reduce((a, c) => a + c.count, 0) };
}

// 项目成员（产品经理）—— 负责人归属的权威来源之一
const MEMBER = {};
for (const f of ['T1_members_LIVE.txt', 'T2_members_000002_LIVE.txt', 'T2_members_stop_LIVE.txt', 'T2_members_LIVE.txt']) {
  const raw = read(path.join(WX, f), '');
  for (const line of raw.split('\n')) {
    if (!/^R\d+ \|/.test(line)) continue;
    const c = line.split('|').map(x => x.trim());
    if (c.length < 11) continue;
    // 整行含「已归档 / 已结案 / 取消 / 停止」的一律不算在跑项目（成员表有 状态=进行中 但另有列写已归档 的脏行）
    if (/已归档|已结案|取消|停止/.test(line)) continue;
    const COL = RULES.columns;
    const model = c[COL.model], pm = c[COL.pm];
    if (!model || !pm || pm === '—' || pm === '/') continue;
    const k = String(model).toUpperCase().replace(/\s/g, '');
    if (!MEMBER[k]) MEMBER[k] = pm;
    const first = String(model).split(/[\/／]/)[0].trim().toUpperCase().replace(/\s/g, '');
    if (first && !MEMBER[first]) MEMBER[first] = pm;
  }
}
const cleanOwner = s => { const v = String(s || '').trim(); return (v === '' || v === '—' || v === '-' || v === '/' || v === '无') ? '' : v; };
const memberOwner = m => MEMBER[String(m || '').toUpperCase().replace(/\s/g, '')]
  || MEMBER[String(m || '').split(/[\/／]/)[0].trim().toUpperCase().replace(/\s/g, '')] || '';

// ---------- 4. 策展台账（可编辑，稳定口径；模型名模糊匹配） ----------
const FOCUS = readJson(path.join(RS, 'brief_focus.json'), { focus: [] }) || { focus: [] };
function curation(p) {
  const key = p.model.toUpperCase().replace(/\s/g, '');
  const list = FOCUS.focus || [];
  // 先精确匹配（避免 ITEM_X 误命中 ITEM_X），再按「表格机型包含台账机型」匹配
  for (const f of list) if (String(f.model).toUpperCase().replace(/\s/g, '') === key) return f;
  for (const f of list) {
    const fk = String(f.model).toUpperCase().replace(/\s/g, '');
    if (fk.length >= 5 && key.includes(fk)) return f;
  }
  return {};
}

// ---------- 5. 派生：最新更新 / 卡点 / 优先级 / 本周节点 ----------
const BLOCK_RE = /(等客户|待客户|待确认|待反馈|待回|暂停|问题|质量异常|异常|返修|重新|延期|未开始|没有居中|贵|降价|核价|决策|不敢做大货|无法|未锁|没回|催)/;
function latestText(p) {
  const c = p.current || '';
  const m = c.match(/\d{1,2}\/\d{1,2}更新/);
  return clip(m ? c.slice(c.indexOf(m[0])) : c, 230) || '—';
}
function blockerText(p) {
  const src = (p.current || '') + ' ' + (p.history || '');
  const parts = src.split(/(?:1、|2、|3、|4、|5、|；|;|\s{2,})/).map(s => s.trim()).filter(Boolean);
  const hits = parts.filter(s => BLOCK_RE.test(s) && s.length > 6).slice(0, 2);
  return hits.length ? clip(hits.join(' / '), 150) : '无显性卡点';
}
function autoPriority(p) {
  const s = `${p.status} ${p.tier} ${p.current}`;
  if (/延期|特急/.test(s)) return 'P0';
  const inWin = n => n && ymd(n) >= WIN_START && ymd(n) <= WIN_END;
  if (p.milestones.some(m => inWin(m.plan) || inWin(m.actual))) return 'P1';
  if (/1[01]\/(8|9|10)更新/.test(p.current)) return 'P1';
  return 'P2';
}
function nextNode(p) {
  const inWin = n => n && ymd(n) >= TODAY && ymd(n) <= WIN_END;
  const cands = p.milestones.filter(m => inWin(m.plan) || inWin(m.actual)).slice(0, 3);
  if (!cands.length) return '—';
  return cands.map(m => {
    const n = m.plan || m.actual;
    return `${md(n)} ${m.name}${m.actual ? '(已排实际)' : ''}`;
  }).join(' → ');
}
function brandText(p) {
  const extra = p.brandExtra.filter(x => !/^(一级|特急|延期|进行中)/.test(x)).slice(0, 3);
  return clip([p.brand, ...extra].filter(Boolean).join(' · '), 46);
}

const T1 = parseSummary('T1_summary_LIVE.txt', '已立项');
const T2 = parseSummary('T2_summary_LIVE.txt', '预立项');
const ALL = [...T1, ...T2].map(p => {
  const c = curation(p);
  return {
    ...p,
    priority: c.priority || autoPriority(p),
    blocker: c.blocker || blockerText(p),
    next: c.next || nextNode(p),
    action: c.action || '—',
    note: c.note || '',
    owner: cleanOwner(c.owner) || memberOwner(p.model) || cleanOwner(p.owner),
    latest: latestText(p),
    brandText: String(brandText(p) || '').replace(/[·\s]+$/, '')
  };
});

// 台账保底：brief_focus.json 里每一个项目都必须出现在看板上，无论汇总页签当轮解析到没有。
// 根因：汇总页签只解析「新项目开模-进度表」(000004) 与「预立项 进度汇总表」(000003)，
// 且页签内容会波动（ITEM_X 曾在某次刷新后整行消失）；ITEM_X 之类项目本来就在
// 「其它类型项目--进度表」(yhqwsm) /「项目成员」(000003)，列结构不同且历史批量读取有串页签残留。
// 故：不做对这些页签的通用解析，改以策展台账为「项目全集」的保底 ——
//   ① 表格解析到了 → 用台账口径覆盖 priority/blocker/next/action/note/owner（稳定口径）
//   ② 表格没解析到 → 按台账补一条（含手工 milestones），绝不从看板消失
const nk = s => String(s || '').toUpperCase().replace(/\s/g, '');
for (const f of (FOCUS.focus || [])) {
  const fk = nk(f.model);
  const hit = ALL.find(p => nk(p.model) === fk)
    || ALL.find(p => fk.length >= 5 && nk(p.model).includes(fk))
    || ALL.find(p => nk(p.model).length >= 5 && fk.includes(nk(p.model)));
  if (hit) {
    hit.priority = f.priority || hit.priority;
    hit.owner = cleanOwner(f.owner) || memberOwner(hit.model) || hit.owner;
    hit.blocker = f.blocker || hit.blocker;
    hit.next = f.next || hit.next;
    hit.action = f.action || hit.action;
    hit.note = f.note || hit.note;
    if (f.status) hit.status = f.status;
    if (f.tier) hit.tier = f.tier;
    if (f.brand && (!hit.brandText || hit.brandText === '—')) hit.brandText = f.brand;
  } else {
    ALL.push({
      tag: f.tag || '台账保底', model: f.model,
      brand: f.brand || '—', brandText: f.brand || '—',
      tier: f.tier || '', status: f.status || '—',
      current: f.latest || '', history: '', brandExtra: [],
      milestones: f.milestones || [], raw: [],
      priority: f.priority || 'P2',
      blocker: f.blocker || '无显性卡点',
      next: f.next || '—', action: f.action || '—',
      note: (f.note ? f.note + ' ' : '') + '［台账保底：本轮汇总页签未解析到该机型，按策展台账展示］',
      owner: cleanOwner(f.owner) || memberOwner(f.model),
      latest: f.latest || '—'
    });
  }
}

// 自动发现：项目成员表里所有「进行中」机型，只要没进台账也没解析到汇总页签，一律补进看板，
// 宁可多列也不漏项；标「自动发现」提醒人工补策展口径。
const DISCOVERED = [];
for (const f of ['T1_members_LIVE.txt', 'T2_members_000002_LIVE.txt', 'T2_members_stop_LIVE.txt', 'T2_members_LIVE.txt']) {
  const raw = read(path.join(WX, f), '');
  for (const line of raw.split('\n')) {
    if (!/^R\d+ \|/.test(line)) continue;
    const c = line.split('|').map(x => x.trim());
    if (c.length < 11) continue;
    const COL = RULES.columns;
    const model = c[COL.model], brand = c[COL.brand], kind = c[COL.kind], status = c[COL.status], tier = c[COL.tier], pm = c[COL.pm];
    if (!model || /^M/i.test(model)) continue;
    // 成员表里有「状态=进行中」但另一列写「已归档」的脏行，整行含归档/结案的一律排除
    if (/已归档|已结案|取消|停止/.test(line)) continue;
    if (!/进行中|延期|暂停/.test(status || '')) continue;
    if (/停止|已结案|归档|取消/.test(status || '')) continue;
    const mk = nk(model);
    if (!mk) continue;
    // 机型 token 去重：FB/ITEM_X/ITEM_X（双色工装）这类变体行，只要 token 与已有项目相交就算同一项目
    const toks = s => (String(s || '').toUpperCase().match(/[A-Z]{2}\d{4,6}[A-Z]?/g) || []);
    const dt = new Set(toks(model));
    if (!dt.size) continue;
    if (ALL.some(p => { const pt = toks(p.model); return pt.some(x => dt.has(x)) || nk(p.model) === mk; })) continue;
    const item = {
      tag: '自动发现', model, brand: brand || '—', brandText: [brand, kind].filter(Boolean).join(' · ') || '—',
      tier: tier || '', status: status || '—', current: '', history: '', brandExtra: [],
      milestones: [], raw: [], priority: 'P2',
      blocker: '',
      next: '—', action: '确认是否需纳入台账', note: '［自动发现：来自项目成员表，尚未纳入 brief_focus 台账］',
      owner: cleanOwner(pm) || '', latest: '—', auto: true
    };
    ALL.push(item);
    DISCOVERED.push(item);
  }
}

// ---- 节点表补全：汇总页签只列关键节点（11 条），专项进度表才有全部节点（30+ 条）----
// sheet_canon.json 由 research/sheet_parse.py + sheet_canon.py 从专项页签解析，作为权威节点源。
try {
  const CANON = JSON.parse(fs.readFileSync(path.join(RS, 'sheet_canon.json'), 'utf8'));
  const famOf2 = (m) => (String(m || '').toUpperCase().match(/\d{4,5}/) || [''])[0];
  const cnum = (v) => { const mm = /^(\d{4,6})\(/.exec(String(v || '')); return mm ? Number(mm[1]) : null; };
  const CANON_BY_FAM = {};
  for (const k of Object.keys(CANON)) {
    const v = CANON[k] || {};
    if (!(v.milestones || []).length) continue;
    const f = famOf2(v.model || k);
    const bag = CANON_BY_FAM[f] = CANON_BY_FAM[f] || { model: v.model || k, milestones: [], _idx: new Map() };
    for (const m of v.milestones) {
      const nm = String(m.n || '').trim();
      if (!nm) continue;
      const cur = bag._idx.get(nm);
      if (!cur) {
        const item = { n: nm, s: m.s || '', p: m.p || '', a: m.a || '', st: m.st || '', d: m.d || '', _src: [k] };
        bag._idx.set(nm, item);
        bag.milestones.push(item);
      } else {
        if (!cur.p && m.p) cur.p = m.p;
        if (!cur.a && m.a) cur.a = m.a;
        if (!cur.st && m.st) cur.st = m.st;
        if (!cur.s && m.s) cur.s = m.s;
        cur._src.push(k);
      }
    }
  }
  let filled = 0;
  for (const p of ALL) {
    const c = CANON_BY_FAM[famOf2(p.model)];
    if (!c) continue;
    const cms = (c.milestones || []).map(m => ({ name: m.n, start: cnum(m.st), plan: cnum(m.p), actual: cnum(m.a), st: m.s || '' }))
      .filter(m => m.name);
    if (cms.length > (p.milestones || []).length) { p.milestones = cms; filled++; }
  }
  console.log('CANON_FILL ' + filled + ' 台机型节点表已用专项页签补全');
} catch (e) { warnings.push('sheet_canon 不可用：' + e.message); }

// 本周节点日历
const CAL = [];
for (const p of ALL) for (const m of p.milestones) {
  for (const [kind, n] of [['现状计划', m.plan], ['实际完成', m.actual]]) {
    if (!n) continue;
    const d = ymd(n);
    if (d >= WIN_START && d <= WIN_END) CAL.push({ date: d, model: p.model, node: m.name, kind, past: d < TODAY });
  }
}
CAL.sort((a, b) => a.date.localeCompare(b.date) || a.model.localeCompare(b.model));
const CAL_SEEN = new Set();
// 节点去重：同一机型数字族（ITEM_X 与 ITEM_X 同族）+ 同一节点名 + 同一天，只算一次。
// 族键取机型里的 4-5 位数字段，规避 FD/FP/FB 前缀与变体后缀造成的“同一支工装算两遍”。
const nodeFam = (m) => (String(m || '').toUpperCase().match(/\d{4,5}/) || [''])[0];
const CAL_U = CAL.filter(x => { const k = x.date + '|' + nodeFam(x.model) + '|' + x.node; if (CAL_SEEN.has(k)) return false; CAL_SEEN.add(k); return true; });
// 二次合并：同族同日的多个节点并成一张卡（ITEM_X 的「整体修模」「硅胶开大模」是同一支
// 工装的两件事，卡片正文完全相同，分开列会被看成"重复节点"）。nodes 保留下所有节点名。
const CAL_G = [];
{
  const idx = new Map();
  for (const x of CAL_U) {
    const k = x.date + '|' + nodeFam(x.model);
    if (idx.has(k)) {
      const g = CAL_G[idx.get(k)];
      if (g.nodes.indexOf(x.node) < 0) g.nodes.push(x.node);
      if (g.mods.indexOf(x.model) < 0) g.mods.push(x.model);
      g.node = g.nodes.join(' / ');
      continue;
    }
    idx.set(k, CAL_G.length);
    CAL_G.push({ date: x.date, model: x.model, node: x.node, nodes: [x.node], mods: [x.model], kind: x.kind, past: x.past });
  }
}
// 同族机型清单：从去重前的 CAL 取（CAL_U 已把同族同节点合并，取不到 65B 这类变体）
// 逐日节点（唯一口径）：CAL_G 按「同族同日同节点」合并后展开成明细行结构，
// 柱顶数字 = 明细里每行节点数之和，二者不可能再对不上。
const byDay = {}; for (const x of CAL_G) (byDay[x.date] = byDay[x.date] || []).push(x);
const DAYLIST0 = Array.from({ length: 14 }, (_, i) => addDays(TODAY, i));
const dayRowsPre = {};
for (const d0 of DAYLIST0) {
  const arr = [];
  for (const x of (byDay[d0] || [])) {
    const mods = (x.mods && x.mods.length ? x.mods : [x.model]);
    for (const mod of mods) arr.push({ model: mod, n: x.node });
  }
  dayRowsPre[d0] = arr;
}
const dayNodeCount = (d0) => (dayRowsPre[d0] || []).reduce((k, r) => k + String(r.n || '').split(' / ').filter(Boolean).length, 0);
const DAYN_TOTAL = DAYLIST0.reduce((n, d0) => n + dayNodeCount(d0), 0);
// 统一口径：窗口内「明细行数」（= 柱顶数字之和 = 密度副标 条数 = KPI = 标题），
// 同一天同族两台机型各算一条；去重节点数另有 CAL_U，不再对外展示，避免两个数字打架。
const DAYN_FUTURE = DAYLIST0.filter(d0 => d0 >= TODAY).reduce((n, d0) => n + dayNodeCount(d0), 0);

for (const g of CAL_G) {
  const fam = nodeFam(g.model);
  const ms = [];
  for (const x of CAL) {
    if (x.date !== g.date) continue;
    if (nodeFam(x.model) !== fam) continue;
    if (g.nodes.indexOf(x.node) < 0) continue;
    if (ms.indexOf(x.model) < 0) ms.push(x.model);
  }
  if (ms.length) g.mods = ms;
}

const FOCUS_LIST = ALL.filter(p => p.priority === 'P0' || p.priority === 'P1')
  .sort((a, b) => ({ P0: 0, P1: 1, P2: 2 }[a.priority] - { P0: 0, P1: 1, P2: 2 }[b.priority]));
const OTHER_LIST = ALL.filter(p => !FOCUS_LIST.includes(p));

const HTTP = httpUpdates();
const CHAT = chatRecent();
const PRIV = privRecent();

// ---- 企业微信群聊 / 私聊（只读本地库导出，见 wxwork_chat_export.py） ----
const WXCHAT = (() => {
  const d = readJson(path.join(RS, 'wxwork_chat.json'), null);
  if (!d || !Array.isArray(d.conversations)) {
    warnings.push('wxwork_chat.json 缺失（企微群聊/私聊不可用）');
    return { groups: [], direct: [], msgs: 0 };
  }
  const groups = []; const direct = [];
  for (const c of d.conversations) {
    // 文本用 text；文件/图片/视频用 label（如「[文件] xxx.pptx (137 MB)」），两类都进群聊流
    const msgs = (c.msgs || [])
      .filter(m => m && m.t && m.t.slice(0, 10) >= CHAT_SINCE &&
        (m.type === 2 ? !!m.text : !!(m.label || m.text)))
      .map(m => ({
        time: m.t, sender: m.from || '', src: '企微',
        text: m.type === 2 ? m.text : (m.label || m.text),
        kind: m.type === 2 ? '' : (m.kind || '')
      }));
    if (!msgs.length) continue;
    const item = {
      group: (c.name && c.name.trim()) || (c.kind === 'group' ? '未命名群' : '私聊'),
      msgs: msgs.sort((a, b) => a.time.localeCompare(b.time)),
      src: '企微',
    };
    (c.kind === 'direct' ? direct : groups).push(item);
  }
  return { groups, direct, msgs: groups.concat(direct).reduce((a, g) => a + g.msgs.length, 0) };
})();
CHAT.groups = CHAT.groups.concat(WXCHAT.groups, WXCHAT.direct);
CHAT.groups.sort((a, b) => {
  const ta = a.msgs.length ? a.msgs[a.msgs.length - 1].time : '';
  const tb = b.msgs.length ? b.msgs[b.msgs.length - 1].time : '';
  return tb.localeCompare(ta);
});

const PULL = readJson(path.join(EM, 'em_full_pull_stats.json'), null);
const RUN = readJson(path.join(RS, 'brief_run_status.json'), null);


// 全部里程碑都已过期时，给 next 正文加「已逾期 N 天 ·」（chip 已标红，正文此前没标）
const ymdOf = (v) => {
  const t = String(v || '').trim();
  let m = /^(\d{4})-(\d{1,2})-(\d{1,2})$/.exec(t);
  if (m) return [Number(m[1]), Number(m[2]), Number(m[3])];
  m = /^(\d{1,2})[\/\-](\d{1,2})$/.exec(t);
  if (m) return [Number(TODAY.slice(0, 4)), Number(m[1]), Number(m[2])];
  m = /^\d{5}$/.exec(t);
  if (m) { const d = new Date(Date.UTC(1899, 11, 30) + Number(t) * 86400000); return [d.getUTCFullYear(), d.getUTCMonth() + 1, d.getUTCDate()]; }
  return null;
};
const toDays = (a) => Math.floor(Date.UTC(a[0], a[1] - 1, a[2]) / 86400000);
const overdueDays = (p) => {
  const list = (p.milestones || []).map(m => ymdOf(m.plan) || ymdOf(m.actual)).filter(Boolean).map(toDays);
  if (!list.length) return 0;
  const now = toDays(ymdOf(TODAY));
  if (list.some(d => d >= now)) return 0;
  return now - Math.max(...list);
};
for (const p of ALL) {
  const od = overdueDays(p);
  if (od > 0 && p.next && !p.next.startsWith('已逾期')) p.nextShow = '已逾期 ' + od + ' 天 · ' + p.next;
}

// ---------- 6. Dashboard v2（图形优先 · 可交互 · 单文件） ----------
const q = s => esc(s);
const IC = {
  grid: 'M3 3h7v7H3zM14 3h7v7h-7zM3 14h7v7H3zM14 14h7v7h-7z',
  alert: 'M12 9v4M12 17h.01M10.3 3.9 2.6 17a2 2 0 0 0 1.7 3h15.4a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z',
  cal: 'M3 5h18v16H3zM3 9h18M8 3v4M16 3v4',
  shield: 'M12 3l8 4v5c0 5-3.5 8-8 9-4.5-1-8-4-8-9V7z',
  chat: 'M4 5h16v11H9l-5 4z',
  clock: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3 2',
  search: 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4',
  x: 'M6 6l12 12M18 6L6 18',
  moon: 'M20 14A8 8 0 0 1 10 4a8 8 0 1 0 10 10z',
  refresh: 'M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6',
  check: 'M4 12l5 5L20 6',
  dot: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z',
  arrow: 'M5 12h14M13 6l6 6-6 6'
};
const icon = (d, sz) => '<svg viewBox="0 0 24 24" width="' + (sz || 14) + '" height="' + (sz || 14) + '" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="' + d + '"/></svg>';

const shortModel = m => {
  const parts = String(m || '').split(/[\/／]/).map(x => x.trim()).filter(Boolean);
  const withNum = parts.find(p => /\d{4,}/.test(p));
  return (withNum || parts[0] || '').trim();
};
const isDel = p => /延期|暂停/.test(p.status || '');
const stKey = p => isDel(p) ? 'delay' : /进行中/.test(p.status || '') ? 'run' : 'other';
const ST_LABEL = { run: '进行中', delay: '延期/暂停', other: '其它' };

// 里程碑进度：实际完成 / 有排期
// 实际完成只认「真的像日期」的值：表格里用 / 、未预立项、空串表示未开始，
// 以前一律当 truthy 计入，会出现「文档登记 33 / 实际写回 30」自相矛盾。
const _hasDate = v => {
  const t = String(v == null ? '' : v).trim();
  if (!t || t === '/' || t === '-' || t === '—' || t === '未预立项') return false;
  for (let i = 0; i < t.length; i++) { const c = t.charCodeAt(i); if (c >= 48 && c <= 57) return true; }
  return false;
};
function prog(p) {
  const ms = p.milestones || [];
  const total = ms.length;
  const done = ms.filter(m => _hasDate(m.actual)).length;
  return { total, done, pct: total ? Math.round(done / total * 100) : 0 };
}

const K = {
  total: ALL.length,
  p0: ALL.filter(p => p.priority === 'P0').length,
  p1: ALL.filter(p => p.priority === 'P1').length,
  p2: ALL.filter(p => p.priority === 'P2').length,
  running: ALL.filter(p => stKey(p) === 'run').length,
  delayed: ALL.filter(p => stKey(p) === 'delay').length,
  blockers: ALL.filter(p => p.blocker).length,
  nodesSoon: DAYN_FUTURE,
  nodesAll: DAYN_TOTAL,
  chatMsgs: CHAT.groups.reduce((a, g) => a + g.msgs.length, 0),
  chatGroups: CHAT.groups.length,
  runSec: (RUN && RUN.totalSec != null) ? RUN.totalSec : null
};
const T = K.total || 1;

// ---- 头部 ----
const topB = FOCUS_LIST.find(p => p.priority === 'P0') || FOCUS_LIST[0];
const runSteps = (RUN && RUN.steps) ? RUN.steps : [];
const stepsHtml = runSteps.length
  ? runSteps.map(s => '<span class="st' + (s.ok ? '' : ' bad') + '" title="' + q(s.detail || '') + '">' + (s.ok ? icon(IC.check, 10) : icon(IC.x, 10)) + q(s.name) + '<b>' + s.sec + 's</b></span>').join('')
  : '<span class="st">无本轮记录</span>';
// 数据完整性条：各源条数 + 新鲜度 + 告警
const INTEG = readJson(path.join(RS, '数据完整性.json'), null);
// ---------- 今日必看（09:10 汇报只看这一块）----------
const MUST = (() => {
  const shortOwn = s => String(s || '').replace(/\s*OWNER_A\s*$/i, '').trim();
  const late = [];
  for (const p of ALL) {
    for (const m of (p.milestones || [])) {
      const d = ymd(m.plan) || '';
      if (!d || d >= TODAY || (m.actual && ymd(m.actual))) { continue; }
      late.push({ model: p.model, m: shortModel(p.model), n: m.name, d: d.slice(5), days: Math.round((Date.parse(TODAY) - Date.parse(d)) / 86400000), pr: p.priority, ow: shortOwn(p.owner) });
    }
  }
  late.sort((a, b) => b.days - a.days);
  const todayN = (CAL_U || []).filter(x => x.date === TODAY).map(x => ({ model: x.model, m: shortModel(x.model), n: x.node, k: x.kind }));
  const tom = addDays(TODAY, 1);
  const tomN = (CAL_U || []).filter(x => x.date === tom).map(x => ({ model: x.model, m: shortModel(x.model), n: x.node }));
  const p0 = ALL.filter(p => p.priority === 'P0' && p.blocker && !/^无显性卡点/.test(p.blocker)).map(p => ({ model: p.model, m: shortModel(p.model), bl: clip(p.blocker, 60), ow: shortOwn(p.owner) }));
  return { late, todayN, tomN, p0, lateTotal: late.length, todayTotal: todayN.length, tomTotal: tomN.length };
})();
function mustRow(kind, model, m, txt, extra) {
  return '<button class="mrow" data-model="' + q(model) + '"><i class="mk ' + kind + '"></i><b>' + q(m) + '</b><span>' + q(txt) + '</span>' + (extra ? '<em>' + extra + '</em>' : '') + '</button>';
}
const mustHtml = (() => {
  const sec = [];
  if (MUST.late.length) {
    // 首屏只放最严重的 8 条：54 条平铺会把 P0 卡点顶到第二屏之外。
    const top = MUST.late.slice(0, 8);
    const rest = MUST.late.slice(8);
    const byProj = {};
    rest.forEach(x => {
      const k = x.model;
      if (!byProj[k]) byProj[k] = { model: k, m: x.m, n: 0, worst: 0 };
      byProj[k].n += 1;
      if (x.days > byProj[k].worst) byProj[k].worst = x.days;
    });
    const agg = Object.keys(byProj).map(k => byProj[k]).sort((a, b) => b.n - a.n);
    const fold = rest.length
      ? '<details class="mfold"><summary>其余 ' + rest.length + ' 条（涉及 ' + agg.length + ' 个项目）· 按项目汇总</summary>'
        + agg.map(a => mustRow('late', a.model, a.m, a.n + ' 条逾期，最久 ' + a.worst + ' 天', '')).join('')
        + '</details>'
      : '';
    sec.push('<div class="msec"><div class="mh"><i class="mk late"></i>逾期未完成 <b>' + MUST.lateTotal + '</b></div>'
      + top.map(x => mustRow('late', x.model, x.m, x.n + ' · 计划 ' + x.d, '逾期 ' + x.days + ' 天')).join('') + fold + '</div>');
  }
  if (MUST.todayN.length) {
    sec.push('<div class="msec"><div class="mh"><i class="mk today"></i>今天到期 <b>' + MUST.todayTotal + '</b></div>'
      + MUST.todayN.map(x => mustRow('today', x.model, x.m, (x.k ? x.k + ' · ' : '') + x.n, '')).join('') + '</div>');
  }
  if (MUST.tomN.length) {
    sec.push('<div class="msec"><div class="mh"><i class="mk tom"></i>明天 <b>' + MUST.tomTotal + '</b></div>'
      + MUST.tomN.map(x => mustRow('tom', x.model, x.m, x.n, '')).join('') + '</div>');
  }
  if (MUST.p0.length) {
    sec.push('<div class="msec"><div class="mh"><i class="mk p0"></i>P0 卡点 <b>' + MUST.p0.length + '</b></div>'
      + MUST.p0.map(x => mustRow('p0', x.model, x.m, x.bl, q(x.ow))).join('') + '</div>');
  }
  if (!sec.length) { sec.push('<div class="mok">今天没有逾期 / 今日节点 / P0 卡点</div>'); }
  return '<div class="panel must">' + sec.join('') + '</div>';
})();
// ---------- 待确认机型：全页签扫出来的、但没进台账且近期仍有动静的机型 ----------
// 原则：宁可多列也不漏；标「待确认」灰卡片，不算 P0/P1，不写回文档。
const famOf = s => (String(s || '').toUpperCase().match(/\d{4,5}/) || [''])[0];
// P0-3：按「完整机型串」判重，不能用数字族（fam），否则 ITEM_X 被台账 ITEM_X 遮蔽、
// ITEM_X 被 ITEM_X 遮蔽、ITEM_X/ITEM_X 互相遮蔽。fam 仅保留给近 30 天群聊检索用。
const boardModels = new Set(ALL.map(p => String(p.model || '').toUpperCase()));
const PENDING = (() => {
  const src = path.join(RS, 'wxdata', 'ALL_HTTP_TEXT.txt');
  if (!fs.existsSync(src)) return [];
  const tabLines = fs.readFileSync(src, 'utf8').split(/\r?\n/);
  const MRE = /(?<![A-Za-z0-9\-_])((?:FB|FD|FP|FZ)\d{4,5}[A-Z]?|ITEM_X[A-Z]?)(?![A-Za-z0-9\-_])/g;
  let tab = '';
  const seen = new Map(); // 完整 model -> {model, fam, tabs:Set, active:bool}
  for (const l of tabLines) {
    const h = l.match(/^===== (.+?) \| (.+?) =====$/);
    if (h) { tab = h[2].trim(); continue; }
    if (/停止|模版|模板|报表注释/.test(tab)) continue;   // 明确停用/模板/注释，不算在跑
    const activeTab = /进度表|进度汇总表|衍生品/.test(tab);
    for (const mm of l.matchAll(MRE)) {
      const model = mm[1].toUpperCase();
      const fam = famOf(model);
      if (!fam || boardModels.has(model)) continue;
      let e = seen.get(model);
      if (!e) { e = { model, fam, tabs: new Set(), activeTab: false }; seen.set(model, e); }
      e.tabs.add(tab);
      if (activeTab) e.activeTab = true;
    }
  }
  // 只保留「在活动进度表里」或「近 30 天群里有消息」的，历史名单一律不进
  const cut = addDays(TODAY, -30);
  const fc = readJson(path.join(EM, 'project_chatlog.json'), []) || [];
  const out = [];
  for (const [model, e] of seen) {
    const fam = e.fam; // fam 仅用于近 30 天群聊检索
    let chat = 0, newest = '';
    for (const g of fc) {
      for (const m of (g.messages || [])) {
        if (String(m.time || '') < cut) continue;
        const up = (String(g.group || '') + ' ' + String(m.text || '')).toUpperCase();
        if (up.includes(fam)) { chat++; if (m.time > newest) newest = m.time; }
      }
    }
    if (!e.activeTab && chat === 0) continue;
    out.push({ model: e.model, fam, tabs: [...e.tabs].slice(0, 3), chat, newest: newest ? newest.slice(5, 10) : '—' });
  }
  return out.sort((a, b) => (b.chat - a.chat) || a.model.localeCompare(b.model)).slice(0, 12);
})();
const pendingHtml = PENDING.length ? ('<div class="panel pend"><div class="pt"><b>待确认机型</b><span>' + PENDING.length + ' 个 · 有动静但不在台账</span></div>'
  + PENDING.map(x => '<div class="prow"><b>' + q(x.model) + '</b><span>' + q(x.tabs.join(' / ')) + '</span>'
    + (x.chat ? '<em>群 ' + x.chat + ' 条 · ' + q(x.newest) + '</em>' : '<em>仅表格出现</em>') + '</div>').join('')
  + '</div>') : '';
try { fs.writeFileSync(path.join(RS, 'pending_models.json'), JSON.stringify({ at: STAMP, rows: PENDING }, null, 2)); } catch (e) { }
// ---------- 文档节点 × 群聊口径 交叉校验 ----------
// 本轮现算：直接用 ALL（当轮解析的文档节点）+ 当轮群聊，
// 不再读上一轮 项目数据看板.html（原管道 4c 在渲染前跑，会滞后一轮）。
const XCHK_PATH = path.join(RS, 'progress_conflicts.json');
const XCHK = (() => {
  const fc = readJson(path.join(EM, 'project_chatlog.json'), []) || [];
  const msgs = [];
  for (const g of fc) for (const m of (g.messages || [])) msgs.push({
    t: String(m.time || ''), s: String(m.sender || ''), x: String(m.text || ''), g: String(g.group || '')
  });
  const NOW = new Date(), DAY = 86400000;
  const ageDays = t => { const d = new Date(String(t).replace(' ', 'T')); return isNaN(d) ? 999 : (NOW - d) / DAY; };
  const planDate = p => {
    const s = String(p || '').trim(); if (!s) return null;
    if (/^\d{4}-\d{1,2}-\d{1,2}$/.test(s)) return new Date(s + 'T00:00:00');
    if (/^\d{1,2}\/\d{1,2}$/.test(s)) {
      const [m, d] = s.split('/').map(Number);
      let dt = new Date(NOW.getFullYear(), m - 1, d);
      if (dt.getTime() < NOW.getTime() - 180 * DAY) dt = new Date(NOW.getFullYear() + 1, m - 1, d);
      return dt;
    }
    return null;
  };
  const DONE = /(已完成|已送样|送样了|已试制|试制完成|批量生产了|已批量生产|已开模|开模完成|已交付|出货了|已关闭|已结案|归档)/;
  const LATE = /(延期|推迟|改期|来不及|卡在|延误|拖了|滞后|滑期|往后)/;
  const BLOCK = /(待|等|催|未回|没人回|未确认|缺|异常|功能异常|结构异常|质量异常|暂停)/;
  const MS_KW = /(试制|送样|批量生产|开模|环测|归档|立项|样件|DVT|T0|T1|工装|试模|夹具)/i;
  // 本机 E-MOBILE 是否有该机型的会话/群：没有群的机型不是本项目在跟的东西（多为别的客户的单被顺带提到），不进诊断
  const convNames = (() => {
    const cj = readJson(path.join(EM, 'conversations_latest.json'), null);
    const arr = (cj && (cj.conversations || cj.list || cj.data)) || [];
    const names = [...new Set(arr.flatMap(c => [c.groupName, c.targetname, c.content]).filter(Boolean).map(s => String(s).toUpperCase()))];
    return names.length ? names : null;
  })();
  const hasOwnGroup = toks => !convNames || toks.some(t => convNames.some(n => n.includes(t)));
  const rows = [];
  const skipped = [];
  for (const p of ALL) {
    const up = String(p.model).toUpperCase();
    const toks = [...new Set([
      ...(up.match(/(?:FB|FD|FP|FZ)\d{4,5}[A-Z]?/g) || []),
      ...(up.match(/(?<!\d)\d{4,5}(?!\d)/g) || []),
      ...(up.match(/ITEM_X[A-Z]?/g) || []), up
    ])];
    const hit = msgs.filter(m => ageDays(m.t) <= 21 && toks.some(t => m.x.toUpperCase().includes(t) || m.g.toUpperCase().includes(t)));
    if (!hasOwnGroup(toks)) { skipped.push({ model: p.model, reason: '本机 E-MOBILE 无该机型群' }); continue; }
    const doneM = hit.filter(m => DONE.test(m.x) && MS_KW.test(m.x));
    const lateM = hit.filter(m => LATE.test(m.x));
    const blockM = hit.filter(m => BLOCK.test(m.x));
    const ms = (p.milestones || []).map(m => ({ n: m.name, p: m.plan ? md(m.plan) : '', a: m.actual ? md(m.actual) : '' }));
    const filledRatio = ms.length ? ms.filter(m => String(m.a || '').trim()).length / ms.length : 0;
    const overdue = ms.filter(m => { const d = planDate(m.p); return d && d < NOW && !String(m.a || '').trim(); });
    const conf = [];
    if (doneM.length >= 1 && ms.length && filledRatio < 0.25) conf.push(`群聊出现 ${doneM.length} 条完成/试制口径，但节点表实际完成仅 ${Math.round(filledRatio * 100)}%`);
    if (lateM.length >= 1 && overdue.length === 0) conf.push(`群聊出现 ${lateM.length} 条延期/推迟口径，节点计划日期未见调整`);
    if (overdue.length >= 1 && hit.length === 0) conf.push(`已逾期节点 ${overdue.length} 个，近 21 天群里无任何进展消息`);
    if (blockM.length >= 2 && !String(p.blocker || '').trim()) conf.push('群里连续催办/报异常，台账卡点为空');
    if (!conf.length) continue;
    const ev = [...doneM, ...lateM, ...blockM].sort((a, b) => (a.t < b.t ? 1 : -1)).slice(0, 3)
      .map(m => `${String(m.t).slice(5, 16)} ${m.s.replace(/\(.*\)$/, '')}：${m.x.replace(/\s+/g, ' ').slice(0, 60)}`);
    rows.push({
      model: p.model, priority: p.priority || '', owner: p.owner || '', status: p.status || '',
      nodeFilled: `${ms.filter(m => String(m.a || '').trim()).length}/${ms.length}`,
      overdue: overdue.map(m => `${m.n}(${m.p})`).slice(0, 4), conflicts: conf,
      chatMsgs21d: hit.length, evidence: ev
    });
  }
  rows.sort((a, b) => {
    const w = s => (s === 'P0' ? 0 : s === 'P1' ? 1 : 2);
    return w(a.priority) - w(b.priority) || a.model.localeCompare(b.model);
  });
  const out = { at: new Date().toISOString(), projects: ALL.length, scanned: ALL.length - skipped.length, skipped, rows };
  try { fs.writeFileSync(XCHK_PATH, JSON.stringify(out, null, 1), 'utf8'); } catch (e) { }
  return out;
})();
const XROWS = XCHK.rows;
const xchkHtml = (() => {
  if (!XCHK) return '';
  if (!XROWS.length) return '<div class="panel xchk"><div class="pt"><b>文档 × 群聊 一致性</b><span>无冲突</span></div>'
    + '<div class="xstat"><b>0</b><span>处口径冲突</span><em>近 21 天</em></div>'
    + '<div class="xok">企微文档节点与近 21 天群聊口径一致</div></div>';
  const items = XROWS.map(r => {
    const ev = (r.evidence || []).slice(0, 2).map(e => '<li>' + q(String(e).slice(0, 90)) + '</li>').join('');
    const od = (r.overdue && r.overdue.length) ? '<span class="xod">逾期 ' + q(r.overdue.join('、')) + '</span>' : '';
    return '<div class="xrow"><div class="xhd"><b>' + q(shortModel(r.model)) + '</b><i class="pri p' + q((r.priority || 'P2').slice(1)) + '">' + q(r.priority || '') + '</i>' +
      '<span class="xown">' + q((r.owner || '').replace(/\s*OWNER_A\s*$/,'')) + '</span>' +
      '<span class="xfill">节点完成 ' + q(r.nodeFilled || '') + ' · 群聊 ' + (r.chatMsgs21d || 0) + ' 条</span>' + od + '</div>' +
      (r.conflicts || []).map(c => '<div class="xc">! ' + q(c) + '</div>').join('') +
      (ev ? '<ul class="xev">' + ev + '</ul>' : '') + '</div>';
  }).join('');
  return '<div class="panel xchk"><div class="pt"><b>文档 × 群聊 一致性</b><span>' + XROWS.length + ' 项需核对</span></div>'
    + '<div class="xstat"><b>' + XROWS.length + '</b><span>处口径冲突</span><em>群聊 21 天窗口</em></div>' + items + '</div>';
})();
const integHtml = INTEG ? (() => {
  const al = Array.isArray(INTEG.alerts) ? INTEG.alerts : [];
  const dot = (ok) => '<i class="idot' + (ok ? '' : ' bad') + '"></i>';
  const parts = [
    dot((INTEG.sheets || 0) >= 19) + '页签 ' + INTEG.sheets + ' · 文本 ' + (INTEG.httpLines || 0).toLocaleString() + ' 行',
    dot((INTEG.groups || 0) >= 100) + '群 ' + INTEG.groups + ' · 消息 ' + (INTEG.messages || 0).toLocaleString() + ' 条',
    dot((INTEG.ageWxH || 0) < 48) + '表格 ' + (INTEG.ageWxH || 0) + 'h 前',
    dot((INTEG.ageEmH || 0) < 48) + '聊天 ' + (INTEG.ageEmH || 0) + 'h 前',
    (INTEG.fresh ? '本轮全量重建' : '本轮增量')
  ];
  return '<div class="integ' + (al.length ? ' bad' : '') + '">' + parts.join('<span class="iseP">·</span>')
    + (al.length ? '<span class="ial">' + q(al.join('；')) + '</span>' : '') + '</div>';
})() : '';

const warnHtml = warnings.length ? '<div class="warn">' + icon(IC.alert, 14) + '数据降级：' + warnings.map(q).join('；') + '</div>' : '';

// ---- KPI（数字 + 单条比例条，不做迷你图） ----
const kpi = (ic, label, val, unit, pct, tone, sub, kind) => '<button class="kpi' + (tone ? ' ' + tone : '') + '" data-kpi="' + (kind || 'all') + '" data-scroll="#sec-' + (tone === 'danger' ? 'focus' : tone === 'warn' ? 'nodes' : 'all') + '">'
  + '<span class="kpi-t">' + icon(IC[ic], 13) + label + '</span>'
  + '<span class="kpi-v" data-n="' + val + '">' + val + '<em>' + unit + '</em></span>'
  + '<span class="kpi-b"><i style="width:' + Math.max(2, Math.min(100, pct)) + '%"></i></span>'
  + '<span class="kpi-s">' + sub + '</span></button>';


// ---- 优先级环（CSS conic-gradient，避免 SVG 属性变量兼容问题） ----
const segs = [];
let acc = 0;
for (const [v, c] of [[K.p0, 'var(--p0)'], [K.p1, 'var(--p1)'], [K.p2, 'var(--p2)']]) {
  if (!v) continue;
  const a = acc / T * 360, b = (acc + v) / T * 360;
  segs.push(c + ' ' + a.toFixed(2) + 'deg ' + b.toFixed(2) + 'deg');
  acc += v;
}
const grad = segs.length ? 'conic-gradient(from 0deg,' + segs.join(',') + ')' : 'var(--track)';
const ring = '<div class="donut" style="background:' + grad + '" role="img" aria-label="优先级分布 P0 ' + K.p0 + ' P1 ' + K.p1 + ' P2 ' + K.p2 + '">'
  + '<span class="veil"></span>'
  + '<span class="donut-in"><b>' + K.total + '</b><em>项目</em></span></div>';

// ---- 状态条 ----
const statusBar = [['run', K.running, 'var(--run)'], ['delay', K.delayed, 'var(--bad)'], ['other', K.total - K.running - K.delayed, 'var(--muted-2)']]
  .map(([k, v, c]) => v ? '<i style="width:' + (v / T * 100).toFixed(2) + '%;background:' + c + '" title="' + ST_LABEL[k] + ' ' + v + '"></i>' : '').join('');
const statusLegend = [['run', K.running], ['delay', K.delayed], ['other', K.total - K.running - K.delayed]]
  .map(([k, v]) => '<span class="lg"><i class="sw ' + k + '"></i>' + ST_LABEL[k] + '<b>' + v + '</b></span>').join('');

// ---- 14 天热力 ----
const DAYS = []; for (let i = 0; i < 14; i++) DAYS.push(addDays(WIN_START, i));
const kpis = [
  kpi('grid', '在管项目', K.total, '个', 100, '', 'P0 ' + K.p0 + ' · P1 ' + K.p1 + ' · P2 ' + K.p2, 'all'),
  kpi('alert', 'P0 必盯', K.p0, '个', K.p0 / T * 100, 'danger', '需本周决策', 'p0'),
  // 静态产物必须自洽：KPI 与标题/热力副标同源（明细行），不能只靠运行时 JS 兜底
  kpi('cal', '未来 14 天', '__NODES__', '个', 100, '', '窗口共 __NODES__', 'nodes'),
  kpi('shield', '待解卡点', K.blockers, '项', K.blockers / T * 100, 'warn', '其中 P0 ' + K.p0 + ' 个', 'blocks'),
  kpi('chat', '群聊近7天', K.chatMsgs, '条', Math.min(100, K.chatGroups / 20 * 100), '', (EM_GROUPS > K.chatGroups ? K.chatGroups + ' 活跃 · ' + (EM_GROUPS - K.chatGroups) + ' 沉默' : K.chatGroups + ' 个群'), 'chat')
].join('');




// ---- 按天节点明细（点热力柱后展开，含卡点内容） ----
const shortOwn = s => String(s || '').replace(/\s*OWNER_A\s*$/i, '').trim();
const DAYD = {};
for (const d of DAYS) {
  DAYD[d] = (byDay[d] || []).flatMap(x => {
    const mods = (x.mods && x.mods.length ? x.mods : [x.model]);
    return mods.map((mod, mi) => {
      const p = ALL.find(y => y.model === mod) || ALL.find(y => y.model === x.model) || {};
      return {
        model: mod, m: shortModel(mod), n: x.node, ms: mods, dup: mi > 0,
        nn: (x.nodes || [x.node]).length, k: x.kind, pr: p.priority || '',
        ow: shortOwn(p.owner), bl: p.blocker || '', nx: p.next || ''
      };
    });
  });
}

// 与热力柱同源的合计（供副标使用，避免两条统计路径）
const __cntDay = d => (DAYD[d] || []).length;
const DAY_TOTAL_NODES = DAYS.reduce((n, d) => n + __cntDay(d), 0);   // 明细行数：柱子与副标都用这个（_page_guard 口径）
const KPIS = kpis.replace(/__NODES__/g, String(DAY_TOTAL_NODES));
const DAY_FUTURE_NODES = DAYS.filter(d => d >= TODAY).reduce((n, d) => n + __cntDay(d), 0);

// ---- 热力柱（数据源 = DAYD，与明细严格同源）----
const byDayN = {};
for (const d of DAYS) byDayN[d] = { length: (DAYD[d] || []).length };
const maxDay = Math.max(1, ...DAYS.map(d => (byDayN[d] || []).length));
const heat = DAYS.map(d => {
  const n = (byDayN[d] || []).length;  const h = n ? Math.round(n / maxDay * 100) : 0;
  const isT = d === TODAY, past = d < TODAY;
  const __rowsOf = d => (DAYD[d] || []).length;
    const tip = d.slice(5) + ' · ' + __rowsOf(d) + ' 条 · ' + n + ' 个节点 · ' + (new Set((DAYD[d] || []).map(x => x.model)).size) + ' 台机型' + (n ? '：' + [...new Set((DAYD[d] || []).map(x => x.model))].join('、') : '');
  const lv = n === 0 ? 'lv0' : (n === 1 ? 'lv1' : (n === 2 ? 'lv2' : 'lv3'));
  const isEdge = d === DAYS[0] || d === DAYS[13];
  const lab = d.slice(8);
  return '<button class="hcol ' + lv + (isT ? ' now' : '') + (past ? ' past' : '') + (n ? '' : ' zero') + '" data-day="' + d + '" title="' + q(tip) + '" aria-label="' + q(tip) + '">'
    + '<span class="hbar" style="height:' + h + '%"><b>' + (n || '') + '</b></span>'
    + '<span class="hlab">' + lab + '</span></button>';
}).join('');

const CAND_KW_PRE = /(试制|送样|批量生产|开模|环测|归档|立项|样件|DVT|T0|T1|工装|试模|夹具|打样|出货|转量|评审|验证)/i;
function candTokensPre(model) {
  const s = String(model || '').toUpperCase();
  const out = new Set();
  (s.match(/(?:FB|FD|FP|FZ)\d{4,5}[A-Z]?/g) || []).forEach(t => out.add(t));
  (s.match(/\d{4,5}[A-Z]?/g) || []).forEach(t => out.add(t));
  if (/ITEM_X/.test(s)) { out.add('ITEM_X'); }
  return [...out];
}// ---- 项目 ↔ 群映射 / 负责人归属 / 私聊印证 ----
const FULLCHAT = readJson(path.join(EM, 'project_chatlog.json'), []) || [];
const PRIVRAW = readJson(path.join(EM, 'personal_chatlog.json'), []) || [];
const NOGROUP_OWNER = RULES.noGroupOwner;
const CORROB = [];
for (const p of ALL) {
  const toks = candTokensPre(p.model);
  const hitGroups = [];
  if (toks.length) {
    for (const g of FULLCHAT) {
      const gname = String(g.group || '').toUpperCase();
      let byName = toks.some(t => gname.includes(t));
      let byMsg = 0;
      if (!byName && Array.isArray(g.messages)) {
        for (const m of g.messages) {
          const up = String(m.text || '').toUpperCase();
          if (toks.some(t => up.includes(t))) { byMsg++; if (byMsg >= 2) { break; } }
        }
      }
      if (byName || byMsg >= 2) { hitGroups.push({ group: g.group, msgs: (g.messages || []).length, byName: byName }); }
    }
  }
  p.groups = hitGroups;
  // 归属：没有对应项目群 → 按组织约定归OWNER_A（文档台账里的 owner 仅作参考）
  if (!hitGroups.length) {
    p.ownerInfer = true;
    p.ownerDoc = p.owner || '';
    p.owner = NOGROUP_OWNER;
  }
  // 群成员（近 21 天发言者）
  const members = new Map();
  for (const g of FULLCHAT) {
    if (!hitGroups.some(h => h.group === g.group)) { continue; }
    for (const m of (g.messages || [])) {
      const who = String(m.sender || '').replace(/\(\d+\)/, '').trim();
      if (who && who !== 'admin') { members.set(who, (members.get(who) || 0) + 1); }
    }
  }
  p.members = [...members.entries()].sort((a, b) => b[1] - a[1]).map(x => x[0]).slice(0, 12);
  // 私聊印证：同这些人、近 45 天、提到机型或里程碑词
  const priv = [];
  if (p.members.length && Array.isArray(PRIVRAW)) {
    const since = addDays(TODAY, -45);
    for (const conv of PRIVRAW) {
      const who = String(conv.group || '').trim();
      if (!p.members.includes(who)) { continue; }
      for (const m of (conv.messages || [])) {
        const txt = String(m.text || '');
        if (!txt || String(m.time || '') < since) { continue; }
        const up = txt.toUpperCase();
        if (!toks.some(t => up.includes(t)) && !CAND_KW_PRE.test(txt)) { continue; }
        priv.push({ who: who, t: m.time, x: clip(txt.replace(/\s+/g, ' '), 60) });
      }
    }
  }
  p.privMs = priv.sort((a, b) => (a.t < b.t ? 1 : -1)).slice(0, 3);
  if (hitGroups.length || p.privMs.length || p.ownerInfer) {
    CORROB.push({ model: p.model, prio: p.priority, owner: p.owner, ownerInfer: !!p.ownerInfer, groups: hitGroups.map(h => h.group).slice(0, 4), groupMsgs: hitGroups.reduce((a, h) => a + h.msgs, 0), members: p.members.slice(0, 6), priv: p.privMs });
  }
}
try { fs.writeFileSync(path.join(RS, 'project_corroborate.json'), JSON.stringify({ at: STAMP, rows: CORROB }, null, 2)); } catch (e) { }
// ---- 群聊口径候选节点（只展示，绝不覆盖文档里程碑） ----
const CAND_KW = /(试制|送样|批量生产|开模|环测|归档|立项|样件|DVT|T0|T1|工装|试模|夹具|打样|出货|转量|评审|验证)/i;
function candTokens(model) {
  const s = String(model || '').toUpperCase();
  const out = new Set();
  (s.match(/(?:FB|FD|FP|FZ)\d{4,5}[A-Z]?/g) || []).forEach(t => out.add(t));
  (s.match(/\d{4,5}[A-Z]?/g) || []).forEach(t => out.add(t));
  if (/ITEM_X/.test(s)) out.add('ITEM_X');
  return [...out];
}
const CHAT_CAND = [];
for (const p of ALL) {
  const toks = candTokens(p.model);
  if (!toks.length) { p.chatMs = []; continue; }
  const found = [];
  for (const g of CHAT.groups) {
    const gname = String(g.group || '').toUpperCase();
    for (const msg of g.msgs) {
      const txt = String(msg.text || '');
      if (!CAND_KW.test(txt)) { continue; }
      const up = txt.toUpperCase();
      if (!toks.some(t => up.includes(t) || gname.includes(t))) { continue; }
      for (const d of [...txt.matchAll(/(\d{1,2})\s*[\/月]\s*(\d{1,2})/g)].slice(0, 4)) {
        const mm = +d[1], dd = +d[2];
        if (mm < 1 || mm > 12 || dd < 1 || dd > 31) { continue; }
        // 只认「节点日期」：日期前后 12 字内必须出现试制/送样/批量生产等节点词。
        // 「剩1/4油」「1/2料」「4/5成品」这类分数量词不是日期。
        const near = txt.slice(Math.max(0, d.index - 12), d.index + d[0].length + 12);
        if (!CAND_KW.test(near)) { continue; }
        if (/[剩占到占]$/.test(txt.slice(Math.max(0, d.index - 1), d.index))) { continue; }
        if (/^[油量比成左右份个套]/.test(txt.slice(d.index + d[0].length))) { continue; }
        // 解析成完整日期：若比今天早 180 天以上，视为下一年的约定
        let yy = +TODAY.slice(0, 4);
        let iso = yy + '-' + String(mm).padStart(2, '0') + '-' + String(dd).padStart(2, '0');
        if (Date.parse(iso) < Date.parse(TODAY) - 180 * 86400000) { yy += 1; iso = yy + '-' + String(mm).padStart(2, '0') + '-' + String(dd).padStart(2, '0'); }
        // 只要“未来 90 天内”的约定；过去日期属于回溯，不进候选
        if (Date.parse(iso) < Date.parse(TODAY) || Date.parse(iso) > Date.parse(addDays(TODAY, 90))) { continue; }
        found.push({ iso: iso, d: iso.slice(5).replace('-', '/'), n: clip(txt.replace(/\s+/g, ' '), 46), who: String(msg.sender || '').replace(/\(\d+\)/, ''), t: msg.time });
      }
    }
  }
  const seen = new Set();
  p.chatMs = found.filter(x => { const k = x.d + x.n; if (seen.has(k)) { return false; } seen.add(k); return true; })
    .sort((a, b) => (a.iso < b.iso ? -1 : 1)).slice(0, 3);
  if (p.chatMs.length) { CHAT_CAND.push({ model: p.model, prio: p.priority, items: p.chatMs }); }
}
try { fs.writeFileSync(path.join(RS, 'chat_candidates.json'), JSON.stringify({ at: STAMP, rows: CHAT_CAND }, null, 2)); } catch (e) { }
// ---- 项目卡（图形编码，点击开抽屉） ----
const card = p => {
  const pr = prog(p);
  const blk = p.blocker && !/^无显性卡点/.test(p.blocker);
  return '<button class="pc" data-model="' + q(p.model) + '" data-prio="' + p.priority + '" data-st="' + stKey(p) + '"'
    + ' data-owner="' + ownerKey(p) + '" data-search="' + q((p.model + ' ' + p.brandText + ' ' + p.status + ' ' + p.blocker + ' ' + p.next + ' ' + (p.owner || '')).toLowerCase()) + '">'
    + '<span class="pc-top"><i class="pr ' + p.priority.toLowerCase() + '">' + p.priority + '</i><b title="' + q(p.model) + '">' + q(shortModel(p.model)) + '</b><i class="dot ' + stKey(p) + '" title="' + (ST_LABEL[stKey(p)]) + '"></i></span>'
    + '<span class="pc-meta">' + q(clip(p.brandText, 26)) + (p.auto ? ' <em class="auto-t">自动发现</em>' : '') + (p.chatMs && p.chatMs.length ? ' <em class="chat-t" title="群聊口径 ' + p.chatMs.length + ' 条（未写回文档）">群聊 ' + p.chatMs.length + '</em>' : '') + '</span>'
    + (pr.total ? '<span class="pc-prog" title="里程碑完成 ' + pr.done + '/' + pr.total + '"><i style="width:' + pr.pct + '%"></i></span>' : '<span class="pc-prog none" title="尚未排期">未排期</span>')
    + '<span class="pc-nums"><em>' + (pr.total ? pr.done + '/' + pr.total : '无排期') + '</em>' + (blk ? '<i class="blk" title="' + q(clip(p.blocker, 120)) + '">' + icon(IC.alert, 11) + '</i>' : '') + '<i class="nx' + (/^逾期/.test(nodeChip(p)) ? ' late' : '') + '">' + q(clip(nodeChip(p), 12)) + '</i></span>'
    + '</button>';
};
function nodeChip(p) {
  // 只显示“今天及以后”的最近节点；全部过期就显示逾期天数，避免把已过去的日期当未来
  const list = (p.milestones || []).map(m => ({ m, d: ymd(m.plan) || ymd(m.actual) })).filter(x => x.d);
  const fut = list.filter(x => x.d >= TODAY).sort((a, b) => (a.d < b.d ? -1 : 1));
  if (fut.length) return (fut[0].d || '').slice(5).replace('-', '/');
  const past = list.filter(x => x.d < TODAY).sort((a, b) => (a.d < b.d ? 1 : -1));
  if (past.length) {
    const dd = Math.round((Date.parse(TODAY) - Date.parse(past[0].d)) / 86400000);
    return '逾期' + dd + '天';
  }
  return '—';
}
// 负责人从台账现算：不再写死任何人名（换公司/换团队自动适配）
// m11139：按 ownerCanon 归一分组，避免「夏宇凡 OWNER_A」与「夏宇凡」生成两个筛选按钮。
// Map key 用 canon；label 取同组最短（优先纯 CJK）；ownerKey 按 canon 比较，不用正则 test。

let OWNERS_CANON = new Map();
const OWNERS = (() => {
  const seen = new Map(); // canon -> { k, canon, label }
  const cjk = s => /^[\u4e00-\u9fa5]+$/.test(s);
  // 同组显示名：优先纯 CJK，其次更短，保证筛选项是最干净的名字
  const better = (a, b) => {
    if (cjk(a) !== cjk(b)) return cjk(a) ? a : b;
    return a.length <= b.length ? a : b;
  };
  for (const p of ALL) {
    const o = String(p.owner || '').trim();
    if (!o || o === '—' || o === '-' || o === '/') continue;
    const canon = ownerCanon(o);
    if (!canon) continue;
    const hit = seen.get(canon);
    if (hit) { hit.label = better(hit.label, o); continue; }
    seen.set(canon, { k: 'o' + seen.size, canon, label: o });
  }
  // 同显示名再合并一次：ownerCanon 会把 "夏宇凡 OWNER_A" 与 "夏宇凡" 分成两个 canon，
  // 但界面上是同一个名字，合并成一个按钮，避免筛选里出现两个一样的项。
  const list = [...seen.values()];
  const byLabel = new Map();
  for (const e of list) {
    const hit = byLabel.get(e.label);
    if (hit) {
      hit.alias.push(e.canon);
      e.dropped = true;
    } else {
      e.alias = [];
      byLabel.set(e.label, e);
    }
  }
  const merged = list.filter(e => !e.dropped);
  merged.forEach((e, i) => { e.k = 'o' + i; });
  const canonMap = new Map();
  for (const e of merged) {
    canonMap.set(e.canon, e.k);
    for (const c of e.alias) canonMap.set(c, e.k);
  }
  OWNERS_CANON = canonMap;
  return merged;
})();
const ownerKey = p => {
  const canon = ownerCanon(p.owner || '');
  return (canon && OWNERS_CANON.get(canon)) || 'other';
};
const ownerLabel = k => (OWNERS.find(o => o.k === k) || { label: '其它' }).label;
const byRank = (a, b) => ({ P0: 0, P1: 1, P2: 2 }[a.priority] - { P0: 0, P1: 1, P2: 2 }[b.priority]) || a.model.localeCompare(b.model);
const cardsHtml = [...OWNERS.map(o => o.k), 'other'].map(k => {
  const list = ALL.filter(p => ownerKey(p) === k).sort(byRank);
  if (!list.length) return '';
  const p0 = list.filter(p => p.priority === 'P0').length;
  return '<div class="grp" data-owner="' + k + '"><div class="grp-h"><b>' + ownerLabel(k) + '</b><em>' + list.length + ' 个</em>'
    + (p0 ? '<i class="grp-p0">P0 ' + p0 + '</i>' : '')
    + '</div><div class="pcs">' + list.map(card).join('') + '</div></div>';
}).join('');

// ---- 甘特：项目 × 14 天 ----
// 每个机型一行（ITEM_X 与 ITEM_X 都保留 —— 用户 2026-11-16 追问"65B 呢"）。
// 同族同节点的重复点降级为空心的"同族"点，既不丢项目也不看成重复。
const __famClaim = new Map();
const ganttRows = ALL.filter(p => {
  const has = (p.milestones || []).some(m => { const d = m.plan ? ymd(m.plan) : (m.actual ? ymd(m.actual) : ''); return d >= WIN_START && d <= WIN_END; });
  return has;
})
  .map(p => {
    const marks = [];
    (p.milestones || []).forEach(m => {
      for (const [kind, n] of [['plan', m.plan], ['act', m.actual]]) {
        if (!n) continue;
        const d = ymd(n);
        if (d >= WIN_START && d <= WIN_END) {
          const idx = DAYS.indexOf(d);
          if (idx >= 0) {
            const fk = d + '|' + nodeFam(p.model) + '|' + m.name;
            const owner = __famClaim.get(fk);
            if (!owner) __famClaim.set(fk, p.model);
            marks.push({ idx, kind: kind === 'act' ? (m.actual && m.plan && m.actual > m.plan ? 'late' : 'done') : 'plan', name: m.name, d, dup: (owner && owner !== p.model) ? owner : '' });
          }
        }
      }
    });
    if (!marks.length) return '';
    // 同一天同族只画一个点：24 号 ITEM_X 的「塑胶修模/硅胶开大模」是同一支工装的两件事，
    // 画两个点会被当成重复（用户 2026-11-16 反馈）。一个点 hover 列出全部节点名。
    const byIdx = new Map();
    for (const x of marks) {
      const a = byIdx.get(x.idx) || { idx: x.idx, kind: x.kind, d: x.d, names: [], dup: x.dup || '', live: !x.dup };
      if (a.names.indexOf(x.name) < 0) a.names.push(x.name);
      if (x.kind !== 'plan') a.kind = x.kind;
      if (!x.dup) { a.live = true; a.dup = ''; }
      byIdx.set(x.idx, a);
    }
    const cells = DAYS.map((d, i) => {
      const ms = marks.filter(x => x.idx === i);
      const one = byIdx.get(i);
      if (!one) return '<span class="gcell"></span>';
      const nm = one.names.join(' / ');
      const cls = 'gdot ' + one.kind + (one.live ? '' : ' dup');
      const tip = one.live ? (one.d.slice(5) + ' ' + nm + ' · 点开详情')
                           : (one.d.slice(5) + ' ' + nm + ' · 与 ' + one.dup + ' 同族同节点');
      return '<span class="gcell">' + '<button type="button" class="' + cls + '" data-model="' + q(p.model) + '" data-day="' + one.d + '" aria-label="' + q(one.d + ' ' + nm) + '" title="' + q(tip) + '"></button>' + '</span>';
    }).join('');
    // 活动窗口条：从首个节点到末个节点，逾期项目用红色
    const idxs = marks.map(x => x.idx);
    const f = Math.min(...idxs), t2 = Math.max(...idxs);
    const lateRow = marks.some(x => x.kind === 'late') || /^逾期/.test(nodeChip(p));
    const track = '<span class="gtrack' + (lateRow ? ' late' : '') + '" style="left:' + (f / 14 * 100).toFixed(2) + '%;width:' + (((t2 - f + 1) / 14 * 100).toFixed(2)) + '%"><i></i></span>';
    return '<div class="grow" data-model="' + q(p.model) + '"><button type="button" class="gname" title="' + q(p.model) + ' · 点开详情">' + q(shortModel(p.model)) + '</button><span class="gcells">' + track + cells + '</span></div>';
  }).join('') || '<div class="empty-v">窗口内无排期</div>';
const ganttHead = DAYS.map((d, i) => '<button type="button" class="gcell ghd' + (d === TODAY ? ' now' : '') + '" data-i="' + i + '" data-day="' + d + '" title="' + q(d + ' · 点选只看这天') + '">' + d.slice(8) + '</button>').join('');

// ---- 群聊流：E-Mobile 与企微分开成列，各自取最近 20 条（混排会被活跃来源整段刷掉） ----
const FEED_EACH = 20;
const feed = [];
for (const g of CHAT.groups) for (const m of g.msgs) feed.push({ t: m.time, s: m.sender, x: m.text, g: g.group, src: m.src || '' });
feed.sort((a, b) => b.t.localeCompare(a.t));
const emFeed = feed.filter(m => m.src !== '企微');
const wxFeed = feed.filter(m => m.src === '企微');
const feedRow = (m, tag, cls) => '<li' + (m.t.slice(0, 10) === TODAY ? ' class="now"' : '') + '>'
  + '<time>' + q(m.t.slice(5, 10)) + '</time>'
  + '<i class="ptag ' + cls + '">' + tag + '</i>'
  + '<b>' + q(clip(m.s.replace(/\(.*?\)/, ''), 10)) + '</b>'
  + '<span title="' + q(m.g) + '">' + q(clip(m.x, 96)) + '</span></li>';
const emTop = emFeed.slice(0, FEED_EACH), wxTop = wxFeed.slice(0, FEED_EACH);
const emHtml = emTop.map(m => feedRow(m, 'EM', 'em')).join('');
const wxHtml = wxTop.map(m => feedRow(m, '企微', 'wx')).join('');
const WX_GROUPS = CHAT.groups.filter(g => g.src === '企微' && g.kind !== 'direct').length;
const WX_DIRECT = CHAT.groups.filter(g => g.src === '企微' && g.kind === 'direct').length;

// ---- 相关私聊 ----
const privHtml = (() => {
  if (!PRIV.convs.length) return '<div class="panel priv"><div class="pt"><b>相关私聊</b><span>本周 0 条命中</span></div><div class="pempty">近 7 天一对一会话未出现台账机型或节点关键词</div></div>';
  const items = PRIV.convs.slice(0, 12).map(c => {
    const ms = c.msgs.map(m => '<li><time>' + q(m.time.slice(5, 16)) + '</time><b>' + q(m.sender) + '</b><span>' + q(clip(m.text, 120)) + '</span></li>').join('');
    const tags = [...new Set(c.msgs.flatMap(m => m.hit || []))].slice(0, 4)
      .map(t => '<i class="ptag">' + q(t) + '</i>').join('');
    return '<div class="pconv"><div class="pch"><b>' + q(c.who) + '</b><em>' + c.count + ' / ' + c.total + ' 条</em>' + tags + '<span class="pcn">' + q(c.newest.slice(0, 10)) + '</span></div><ul>' + ms + '</ul></div>';
  }).join('');
  return '<div class="panel priv"><div class="pt"><b>相关私聊</b><span>' + PRIV.convs.length + ' 人 / ' + PRIV.total + ' 条命中</span></div>' + items + '</div>';
})();

// ---- 抽屉数据 ----
const DRAW = {};
for (const p of ALL) {
  const pr = prog(p);
  DRAW[p.model] = {
    model: p.model, prio: p.priority, status: p.status, stKey: stKey(p),
    brand: p.brandText, owner: p.owner || '', tag: p.tag,
    blocker: p.blocker, next: p.nextShow || p.next, action: p.action, note: p.note, latest: p.latest,
    prog: pr, chatMs: p.chatMs || [], priv: p.privMs || [], groups: (p.groups || []).map(g => g.group).slice(0, 4), members: p.members || [], ownerInfer: !!p.ownerInfer,
    // 里程碑按日期升序排（原先按页签/合并顺序，看起来是乱的）
    // 状态：done=已写回实际完成 / late=实际晚于计划 / today=今天到期未完成 / over=计划已过未完成 / ''=未到期
    ms: (p.milestones || []).map(m => {
      // ymd(null) 会算成 1899-12-30，必须先挡空值，否则「没填实际完成」会被当成已完成
      const isoOf = v => (v === null || v === undefined || v === '') ? '' : ymd(v);
      const dPlan = isoOf(m.plan), dAct = isoOf(m.actual), d = dAct || dPlan || '';
      const late = !!(m.actual && m.plan && m.actual > m.plan);
      const state = dAct ? (late ? 'late' : 'done') : (dPlan && dPlan < TODAY ? 'over' : (dPlan === TODAY ? 'today' : ''));
      return { n: m.name, s: m.st || '', p: m.plan ? md(m.plan) : '', a: m.actual ? md(m.actual) : '', d, late, state };
    }).sort((x, y) => (x.d || '9999-99-99').localeCompare(y.d || '9999-99-99'))
  };
}

const CSS = `${pageCss()}`;

const JS = `${pageJs()}`.replace('__DRAW__', JSON.stringify(DRAW).replace(/</g, '\\u003c')).replace('__TODAYSTR__', TODAY).replace('__DAYD_DATA__', JSON.stringify(DAYD).replace(/</g, '\\u003c'));

// ---------- v3：图形优先视图块（不改变任何数据口径，仅重排渲染） ----------
const GSAP_SRC = (() => { try { return fs.readFileSync(path.join(RS, 'vendor', 'gsap.min.js'), 'utf8'); } catch (e) { return ''; } })();
const WD = '周' + '日一二三四五六'[new Date(TODAY + 'T00:00:00Z').getUTCDay()];
const GP = (() => {
  let d = 0, t = 0;
  for (const p of ALL) { const r = prog(p); d += r.done; t += r.total; }
  return { done: d, total: t, pct: t ? Math.round(d / t * 100) : 0 };
})();
const HP = Math.round(K.running / T * 100);
const healthMeter = (() => {
  const on = Math.round(HP / 100 * 12);
  let s = '<span class="meter" title="进行中 ' + K.running + ' · 延期 ' + K.delayed + ' · 其它 ' + (K.total - K.running - K.delayed) + '">';
  for (let i = 1; i <= 12; i++) s += '<i class="' + (i <= on ? (K.delayed && i === on ? 'bad' : 'on') : '') + '"></i>';
  return s + '</span>';
})();
const railCells = n => { const k = Math.min(12, Math.max(0, Number(n) || 0)); let s = ''; for (let i = 0; i < 12; i++) s += '<i' + (i < k ? ' class="on"' : '') + '></i>'; return s; };
const ugTile = o => '<button class="ug ' + o.tone + '"' + (o.kpi ? ' data-kpi="' + o.kpi + '"' : '') + ' data-scroll="' + (o.scroll || '#sec-diag') + '" title="' + q(o.tip || o.label) + '">'
  + '<span class="lb">' + o.label + '</span>'
  + '<span class="side">' + (o.side || '') + '</span>'
  + '<span class="n" data-n="' + o.n + '">' + o.n + (o.unit ? '<span class="u">' + o.unit + '</span>' : '') + '</span>'
  + '<span class="rail">' + railCells(o.n) + '</span></button>';
const DIAGN = XROWS.length + PENDING.length;
const ugTiles = [
  ugTile({ tone: 'bad', label: '逾期未完成', n: MUST.lateTotal, unit: '项', side: MUST.late.length ? ('最久 ' + MUST.late[0].days + ' 天') : '无', tip: '计划日期已过且未标完成', scroll: '#sec-must' }),
  ugTile({ tone: 'warn', label: '今天到期', n: MUST.todayTotal, unit: '项', side: '明天 ' + MUST.tomTotal, tip: '今天到期的计划节点', scroll: '#sec-must' }),
  ugTile({ tone: 'bad', label: 'P0 卡点', n: MUST.p0.length, unit: '项', kpi: 'p0', side: 'P0 共 ' + K.p0, tip: 'P0 项目未解卡点' }),
  ugTile({ tone: 'blue', label: '口径冲突', n: XROWS.length, unit: '项', side: '文档 × 群聊', tip: '文档节点表与群聊说法不一致；另有 ' + PENDING.length + ' 个待确认机型见诊断区' })
].join('');
const ring2 = '<div class="donut" role="img" aria-label="里程碑完成 ' + GP.pct + '%" style="background:conic-gradient(from -90deg,var(--run) 0 ' + GP.pct + '%,var(--track) ' + GP.pct + '% 100%)">'
  + '<span class="veil"></span>'
  + '<span class="donut-in"><b>' + GP.pct + '<span style="font-size:12px">%</span></b><em>节点达成</em></span></div>';

const __DAYCOUNT_SYNC__ = (() => { try { return fs.readFileSync(path.join(RS, '_daycount_sync.js'), 'utf8'); } catch (e) { return ''; } })();
const html = `<!doctype html>
<html lang="zh-CN" data-theme="dark"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>项目看板 · ${TODAY}</title>
<style>${CSS}</style></head><body>

<div class="strip">
  <span class="brand">${icon(IC.grid, 13)}项目简报</span>
  <span class="dotline"></span>
  <span class="k">${TODAY.slice(5)} ${WD}</span>
  <span class="k"><i class="sdot${warnings.length ? ' bad' : ''}"></i>数据 <b>${DATA_AT ? DATA_AT.slice(5) : "—"}</b></span>
  <span class="k opt">在管 <b>${K.total}</b></span>
  <span class="k opt2">健康 <b>${HP}%</b></span>
  ${healthMeter}
  <span class="k opt">群 <b>${K.chatGroups}</b> 活跃${EM_GROUPS > K.chatGroups ? ' / ' + EM_GROUPS : ''}</span>
  <span class="r">
    <button class="tb" id="theme">${icon(IC.moon, 12)}<span id="theme-t">浅色</span></button>
    <button class="tb" onclick="location.reload()" aria-label="刷新">${icon(IC.refresh, 12)}</button>
  </span>
</div>

<div class="wrap">
${warnHtml}
${integHtml}

<h2 id="sec-must"><span class="ix">01</span>今天必须处理<em>30 秒扫完 · 点任意行下钻</em></h2>
<div class="ugrid">${ugTiles}</div>
${mustHtml}

<h2 id="sec-all"><span class="ix">02</span>项目健康<em>${K.total} 个项目 · 全量里程碑 ${GP.total} · 未来 14 天节点 ${DAY_TOTAL_NODES}</em></h2>
<div class="kpis">${KPIS}</div>
<div class="row3">
  <div class="panel"><div class="pt"><b>优先级分布</b><span>${K.total} 项</span></div>
    <div class="ringwrap">${ring}
      <div class="lgs">
        <span class="lg"><i class="sw" style="background:var(--p0)"></i>P0 必盯<b>${K.p0}</b></span>
        <span class="lg"><i class="sw" style="background:var(--p1)"></i>P1 推进<b>${K.p1}</b></span>
        <span class="lg"><i class="sw" style="background:var(--p2)"></i>P2 观察<b>${K.p2}</b></span>
      </div>
    </div>
  </div>
  <div class="panel"><div class="pt"><b>里程碑完成</b><span>${GP.done} / ${GP.total}</span></div>
    <div class="ringwrap">${ring2}
      <div class="lgs">
        <span class="lg"><i class="sw run"></i>已完成<b>${GP.done}</b></span>
        <span class="lg"><i class="sw other"></i>排期中<b>${GP.total - GP.done}</b></span>
        <span class="lg"><i class="sw delay"></i>已逾期<b>${MUST.lateTotal}</b></span>
      </div>
    </div>
  </div>
  <div class="panel"><div class="pt"><b>运行状态</b><span>健康比 ${HP}%</span></div>
    <div class="sbar">${statusBar}</div>
    <div class="lgs">${statusLegend}</div>
    <div class="sline">窗口 <b>${WIN_START.slice(5)} → ${WIN_END.slice(5)}</b> · 节点 <b>${DAY_TOTAL_NODES}</b> · 群聊 <b>${K.chatMsgs}</b> 条</div>
  </div>
</div>

<h2 id="sec-focus"><span class="ix">03</span>项目<em>${ALL.length} 个 · 按负责人分组</em></h2>
<div class="tools">
  <label class="srch">${icon(IC.search, 13)}<input id="q" type="search" aria-label="搜索"></label>
  <div class="fs" id="fp"><button class="on" data-p="ALL">全部</button><button data-p="P0">P0</button><button data-p="P1">P1</button><button data-p="P2">P2</button></div>
  <div class="fs" id="fs"><button class="on" data-s="ALL">全状态</button><button data-s="run">进行中</button><button data-s="delay">延期</button></div>
  <div class="fs" id="fo"><button class="on" data-o="ALL">全部人</button>${OWNERS.map(o => '<button data-o="' + o.k + '">' + q(o.label) + '</button>').join('')}</div>
  <span class="cnt" id="fc"></span>
</div>
<div id="groups">${cardsHtml}</div>

<h2 id="sec-nodes"><span class="ix">04</span>时间轴<em><span id="glab">全部日期</span> <button class="tb" id="gclear">重置</button></em></h2>
<div class="panel">
  <div class="pt"><b>未来 14 天节点密度</b><span>${DAY_TOTAL_NODES} 条 · 覆盖 ${new Set(CAL.map(x => x.model)).size} 台机型 · 点柱看当天</span></div>
  <div class="heat">${heat}</div>
</div>
<div class="panel daypanel" id="daybox" hidden></div>
<div class="gantt">
  <div class="ghead"><span class="gname">项目</span><span class="gcells">${ganttHead}</span></div>
  ${ganttRows}
  <div class="glegend"><span title="未到期的计划节点"><i class="lg plan"></i>计划</span><span title="按期完成"><i class="lg done"></i>已完成</span><span title="完成时间晚于计划"><i class="lg late"></i>超期完成</span><span title="同族同节点的另一台机型（合并计 1 个节点）"><i class="lg dup"></i>同族重复（空心）</span><span class="gleg-tip">点圆点或项目名下钻 · 点日期列只看这天</span></div>
</div>

<h2 id="sec-chat"><span class="ix">05</span>群聊与私聊<em>近 7 天 ${K.chatMsgs} 条 / ${K.chatGroups} 群${EM_GROUPS > K.chatGroups ? ' · ' + (EM_GROUPS - K.chatGroups) + ' 沉默' : ''}</em></h2>
<div class="cols2 chat${PRIV.convs.length ? "" : " solo"}">
  <div class="panel feed"><div class="pt"><b>群聊流</b><span>E-Mobile ${emFeed.length} 条 / 企微 ${wxFeed.length} 条 · 各显示最近 ${FEED_EACH} 条</span></div>
    <div class="fcols">
      <div class="fcol"><div class="fch">E-Mobile<em>${emTop.length} / ${emFeed.length}</em></div><ul>${emHtml || '<li class="fempty">近 7 天没有 E-Mobile 群聊消息</li>'}</ul></div>
      <div class="fcol"><div class="fch">企业微信<b>群 ${WX_GROUPS} · 私聊 ${WX_DIRECT}</b><em>${wxTop.length} / ${wxFeed.length}</em></div><ul>${wxHtml || '<li class="fempty">近 7 天没有企微消息（客户端未登录或未同步）</li>'}</ul></div>
    </div>
  </div>
  ${privHtml}
</div>

<h2 id="sec-diag"><span class="ix">06</span>诊断<em>文档 × 群聊 · 未入台账机型</em></h2>
<div class="diag">
  ${xchkHtml}
  ${pendingHtml}
</div>

<div class="foot">
口径：微信「已立项 / 预立项汇总」为在线实时值，重点 / 卡点 / 节点走 <code>research/brief_focus.json</code> 策展台账（全量保底，页签波动不漏项）；E-Mobile 为 7070 全量聊天，结论以正式文档为准。<br>
刷新：<code>pwsh -File 每日项目简报.ps1</code> · 数据源 ${DATA_AT ? DATA_AT.slice(5) : "—"} · 生成 ${STAMP.slice(5)} · 内部口径，请勿外发。
</div>
</div>

<div class="ov" id="ov"></div>
<div class="dw" id="dw" role="dialog" aria-modal="true" aria-hidden="true" aria-label="项目详情">
  <div class="dw-h"><b id="dw-t">—</b><span id="dw-p" style="display:flex;align-items:center;gap:7px"></span><button class="dw-x" id="dw-x" aria-label="关闭">${icon(IC.x, 15)}</button></div>
  <div class="dw-b" id="dw-b"></div>
</div>
<div class="lm" id="lm" role="dialog" aria-modal="true" aria-hidden="true" aria-label="节点清单">
  <div class="lm-box">
    <div class="lm-h"><b id="lm-t"></b><em id="lm-c"></em><button class="dw-x" id="lm-x" aria-label="关闭">${icon(IC.x, 15)}</button></div>
    <div class="lm-b" id="lm-b"></div>
  </div>
</div>
${GSAP_SRC ? '<script>' + GSAP_SRC + '</script>' : ''}
<script>${JS}</script><script>${__DAYCOUNT_SYNC__}</script>
</body></html>`;


fs.writeFileSync(OUT_HTML, html, 'utf8');
fs.writeFileSync(path.join(ROOT, '每日简报_' + TODAY + '.html'), html, 'utf8');
// 归档只保留最近 2 天（含今天），避免 每日简报_<日期>.html 无限堆积
try {
  const arch = fs.readdirSync(ROOT)
    .filter(f => /^每日简报_\d{4}-\d{2}-\d{2}\.html$/.test(f))
    .sort().reverse();
  for (const old of arch.slice(2)) {
    try { fs.unlinkSync(path.join(ROOT, old)); console.log('ARCHIVE_PRUNED', old); } catch (e) {}
  }
} catch (e) { console.log('ARCHIVE_PRUNE_SKIP', e.message); }
// 不再产出「老 UI」中间页（项目数据看板_生成.html）：V4 现役页由 5a 注入生成，
// 这份重复渲染的页面只会让人误以为是现役看板，2026-11-16 起停写。


// ---------- 断言自检（缺项 / 占位符泄漏） ----------
const ASSERT_MISS = FOCUS_LIST.filter(p => !html.includes(q(p.model))).map(p => p.model);
const ASSERT_TOKEN = /\{\{[A-Za-z0-9_]+\}\}/.test(html);
const assertStatus = {
  at: new Date().toISOString(), ledger: FOCUS_LIST.length, cards: ALL.length,
  missing: ASSERT_MISS, tokenLeak: ASSERT_TOKEN,
  ok: ASSERT_MISS.length === 0 && !ASSERT_TOKEN
};
fs.writeFileSync(path.join(ROOT, 'research', 'assert_status.json'), JSON.stringify(assertStatus, null, 2), 'utf8');
console.log(assertStatus.ok
  ? 'ASSERT OK | ledger=' + FOCUS_LIST.length + ' cards=' + ALL.length
  : 'ASSERT FAIL | missing=' + JSON.stringify(ASSERT_MISS) + ' tokenLeak=' + ASSERT_TOKEN);

// 轻量 Markdown 摘要（便于贴群/邮件）
const mdLines = [
  `# 项目简报 ${TODAY}`, '',
  `> ${K.total} 个项目 / ${K.p0} 个 P0 / ${K.nodesSoon} 个未来节点 / ${K.blockers} 项待解卡点 / 近7天 ${K.chatMsgs} 条群聊`, '',
  `## 今日必看`, '',
  `- 逾期未完成 ${MUST.lateTotal} 项${MUST.late.length ? '：' + MUST.late.slice(0, 3).map(x => x.m + '「' + x.n + '」逾期' + x.days + '天').join('；') : ''}`,
  `- 今天到期 ${MUST.todayTotal} 项${MUST.todayN.length ? '：' + MUST.todayN.slice(0, 3).map(x => x.m + '·' + x.n).join('；') : ''}`,
  `- 明天 ${MUST.tomTotal} 项${MUST.tomN.length ? '：' + MUST.tomN.slice(0, 3).map(x => x.m + '·' + x.n).join('；') : ''}`,
  `- P0 卡点 ${MUST.p0.length} 项${MUST.p0.length ? '：' + MUST.p0.slice(0, 3).map(x => x.m + '·' + x.bl).join('；') : ''}`,
  '', `## 文档 × 群聊 一致性`, '',
  ...(XROWS.length ? XROWS.flatMap(r => [`- **${r.model}**（${r.priority}）节点 ${r.nodeFilled} · 群聊 ${r.chatMsgs21d} 条`, ...r.conflicts.map(c => '  - ! ' + c)]) : ['- 一致，无冲突']),
  '', `## 待确认机型`, '',
  ...(PENDING.length ? PENDING.map(x => `- ${x.model} · ${x.tabs.join('/')}${x.chat ? ' · 群 ' + x.chat + ' 条' : ''}`) : ['- 无']),
  '', `## 本周重点关注（${FOCUS_LIST.length}）`, '',
  '| 级别 | 项目 | 状态 | 卡点 | 下一节点 | 本周动作 |',
  '|---|---|---|---|---|---|',
  ...FOCUS_LIST.map(p => `| ${p.priority} | ${p.model} | ${p.status} | ${p.blocker.replace(/\|/g, '/')} | ${p.next.replace(/\|/g, '/')} | ${p.action.replace(/\|/g, '/')} |`),
  '', `## 其它项目（${OTHER_LIST.length}）`, '',
  ...OTHER_LIST.map(p => `- **${p.model}**（${p.status}）${clip(p.latest, 90)}`),
  '', `## 近期节点`, '',
  ...CAL_G.slice(0, 20).map(x => `- ${x.date} ${x.model} · ${x.node}（${x.kind}）`),
  '', `## 近 7 天群聊（节选）`, '',
  ...CHAT.groups.slice(0, 6).flatMap(g => [
    `**${g.group}**`,
    ...g.msgs.slice(-3).map(m => `- ${m.time.slice(5)} ${m.sender}：${clip(m.text, 100)}`)
  ]),
  '', `---`, `数据：微信表格实时 + E-Mobile ${CHAT.total} 条；生成 ${STAMP}。`
];
fs.writeFileSync(OUT_MD, mdLines.join('\n'), 'utf8');

console.log(`==== BRIEF OK | focus=${FOCUS_LIST.length} other=${OTHER_LIST.length} discovered=${DISCOVERED.length} nodes=${DAYN_TOTAL} chatGroups=${CHAT.groups.length} http=${HTTP.length} warnings=${warnings.length} ====`);
console.log('OUT', OUT_HTML);
