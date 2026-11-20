// em_ws_auth.cjs —— 对 7070 做「真认证」探活（SCRAM-SHA-1），不发任何业务 IQ、不新建会话
// 用法：const { authProbe } = require('./em_ws_auth.cjs');
//       const ok = await authProbe({ ws_url, msg_domain, authcid }, token, 8000);
const crypto = require('crypto');

function authProbe(conn, token, ms) {
  return new Promise(resolve => {
    const WS = globalThis.WebSocket;
    if (!WS || !conn || !conn.ws_url || !token) return resolve({ ok: false, why: 'no-ws' });
    const timeout = ms || 8000;
    let settled = false, stage = 0, cnonce = '', cbare = '', authcid = conn.authcid || '';
    let ws;
    const fin = (ok, why) => {
      if (settled) return; settled = true;
      try { ws && ws.close(); } catch (e) {}
      resolve({ ok, why: why || (ok ? 'ok' : 'fail') });
    };
    const t = setTimeout(() => fin(false, 'timeout'), timeout);
    try { ws = new WS(conn.ws_url, 'xmpp'); } catch (e) { return fin(false, 'open-fail'); }
    ws.onerror = () => fin(false, 'ws-error');
    ws.onopen = () => {
      ws.send('<open xmlns="urn:ietf:params:xml:ns:xmpp-framing" to="' + (conn.msg_domain || 'weaver') + '" version="1.0"/>');
    };
    ws.onmessage = ev => {
      const d = String(ev.data);
      if (stage === 0) {
        if (d.indexOf('stream:features') < 0) return;
        if (d.indexOf('SCRAM-SHA-1') < 0) return fin(false, 'no-scram');
        cnonce = crypto.randomBytes(12).toString('hex');
        cbare = 'n=' + authcid + ',r=' + cnonce;
        const init = Buffer.from('n,,' + cbare, 'utf8').toString('base64');
        stage = 1;
        ws.send('<auth xmlns="urn:ietf:params:xml:ns:xmpp-sasl" mechanism="SCRAM-SHA-1">' + init + '</auth>');
        return;
      }
      if (stage === 1) {
        const m = d.match(/<challenge[^>]*>([^<]+)<\/challenge>/);
        if (!m) return fin(false, 'no-challenge');
        const sf = Buffer.from(m[1], 'base64').toString('utf8');
        const r = (sf.match(/r=([^,]+)/) || [])[1];
        const s = (sf.match(/s=([^,]+)/) || [])[1];
        const i = parseInt((sf.match(/i=(\d+)/) || [])[1] || '4096', 10);
        if (!r || !s) return fin(false, 'bad-challenge');
        const salted = crypto.pbkdf2Sync(token, Buffer.from(s, 'base64'), i, 20, 'sha1');
        const ck = crypto.createHmac('sha1', salted).update('Client Key').digest();
        const sk = crypto.createHash('sha1').update(ck).digest();
        const am = cbare + ',' + sf + ',c=biws,r=' + r;
        const cs = crypto.createHmac('sha1', sk).update(am).digest();
        const proof = Buffer.alloc(ck.length);
        for (let k = 0; k < ck.length; k++) proof[k] = ck[k] ^ cs[k];
        const resp = 'c=biws,r=' + r + ',p=' + proof.toString('base64');
        stage = 2;
        ws.send('<response xmlns="urn:ietf:params:xml:ns:xmpp-sasl">' + Buffer.from(resp, 'utf8').toString('base64') + '</response>');
        return;
      }
      if (d.indexOf('<success') >= 0) { clearTimeout(t); return fin(true, 'auth-ok'); }
      if (d.indexOf('<failure') >= 0) { clearTimeout(t); return fin(false, 'auth-rejected'); }
    };
  });
}

module.exports = { authProbe };
