// 把小黄条(yynote)的待办一次性同步进挂件状态文件的 department 段
// 幂等：同 id 已存在则保留本地 done 状态，只更新 text/due
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SRC = process.argv[3] || path.join(HERE, 'yynote_todos.json');
const ST = process.argv[2] || path.join(HERE, 'widget_state.json');

const src = JSON.parse(fs.readFileSync(SRC, 'utf8'));
const st = fs.existsSync(ST) ? JSON.parse(fs.readFileSync(ST, 'utf8')) : {};
st.custom = st.custom || [];
st.department = st.department || [];
st.done = st.done || {};
st.archive = st.archive || [];
st.ui = st.ui || { tab: 'proj', win_mode: 'float' };

const now = new Date().toISOString();
let added = 0, updated = 0, kept = 0;
const sameKey = (a, b) => a === b || String(a).replace(/^y/, '') === String(b).replace(/^y/, '');
const legalDue = d => {
  const s = String(d || '').trim();
  if (!s) return '';
  const m = s.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (!m) return '';
  const dt = new Date(s + 'T00:00:00Z');
  return isNaN(dt.getTime()) ? '' : s;
};

for (const it of src.items || []) {
  if (it.deleted) continue;
  const id = it.id || ('ynt-' + Buffer.from(it.text || '').toString('hex').slice(0, 12));
  const prev = st.department.find(d => sameKey(d.id, id)) || st.department.find(d => (d.text || '').trim() === ((it.text || '').trim() || '(无标题)'));
  const done = prev ? (prev.done === true || it.done === true) : !!it.done;
  const text = (it.text || '').trim() || '(无标题)';
  const due = legalDue(it.due);
  const note = String(it.note || '').replace(/\r\n/g, '\n').trim();
  if (prev) { prev.text = text; prev.due = due; prev.note = note || prev.note || ''; prev.done = done; prev.syncedAt = now; updated++; }
  else { st.department.push({ id, text, due, done, note, src: 'yynote', importedAt: now, syncedAt: now }); added++; }
  if (done) st.done[id] = true;
}
// 清理：源里已删除的项（deleted=true）从 department 移除
const live = new Set((src.items || []).filter(i => !i.deleted).map(i => i.id));
st.department = st.department.filter(d => !(d.src === 'yynote' && String(d.id).replace(/^y/, '').startsWith('PC_Todo_') && !live.has(String(d.id).replace(/^y/, ''))));

const tmp = ST + '.tmp';
fs.writeFileSync(tmp, JSON.stringify(st, null, 2), 'utf8');
fs.renameSync(tmp, ST);
console.log(`SYNC ok | added=${added} updated=${updated} | department=${st.department.length} custom=${st.custom.length} archive=${st.archive.length}`);
