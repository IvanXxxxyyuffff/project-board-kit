//! 内置 HTTP/HTTPS 客户端 —— 直接调 Windows 自带 WinHTTP，不引入 TLS 依赖。
//!
//! 目的：让「项目看板.exe」自己就能抓企业微信文档 / 打 E-Mobile 接口，
//! 不再依赖 pwsh + node + 第三方库。TLS、代理、证书链全部由系统组件负责。

use std::ffi::{c_void, OsStr};
use std::os::windows::ffi::OsStrExt;

#[link(name = "winhttp")]
extern "system" {
    fn WinHttpOpen(
        psz_agent: *const u16,
        dw_access_type: u32,
        psz_proxy: *const u16,
        psz_bypass: *const u16,
        dw_flags: u32,
    ) -> *mut c_void;
    fn WinHttpConnect(
        h_session: *mut c_void,
        pswz_server: *const u16,
        n_server_port: u16,
        dw_reserved: u32,
    ) -> *mut c_void;
    fn WinHttpOpenRequest(
        h_connect: *mut c_void,
        pwsz_verb: *const u16,
        pwsz_object: *const u16,
        pwsz_version: *const u16,
        pwsz_referrer: *const u16,
        ppwsz_accept_types: *const *const u16,
        dw_flags: u32,
    ) -> *mut c_void;
    fn WinHttpSendRequest(
        h_request: *mut c_void,
        lpsz_headers: *const u16,
        dw_headers_length: u32,
        lp_optional: *const c_void,
        dw_optional_length: u32,
        dw_total_length: u32,
        dw_context: usize,
    ) -> i32;
    fn WinHttpReceiveResponse(h_request: *mut c_void, lp_reserved: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(
        h_request: *mut c_void,
        dw_info_level: u32,
        pwsz_name: *const u16,
        lp_buffer: *mut c_void,
        lpdw_buffer_length: *mut u32,
        lpdw_index: *mut u32,
    ) -> i32;
    fn WinHttpReadData(
        h_request: *mut c_void,
        lp_buffer: *mut c_void,
        dw_number_of_bytes_to_read: u32,
        lpdw_number_of_bytes_read: *mut u32,
    ) -> i32;
    fn WinHttpCloseHandle(h_internet: *mut c_void) -> i32;
    fn WinHttpSetTimeouts(
        h_internet: *mut c_void,
        n_resolve_timeout: i32,
        n_connect_timeout: i32,
        n_send_timeout: i32,
        n_receive_timeout: i32,
    ) -> i32;
}

const ACCESS_TYPE_AUTOMATIC_PROXY: u32 = 4;
const FLAG_SECURE: u32 = 0x0080_0000;
const QUERY_STATUS_CODE: u32 = 19;
const QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
const QUERY_RAW_HEADERS_CRLF: u32 = 22;

// 连接复用池：key = scheme://host:port，值为 (session, connect) 原始句柄。
// 不关闭是刻意的——它们就是 Keep-Alive 的连接池；进程退出由系统回收。
static POOL: std::sync::Mutex<Vec<(String, usize, usize)>> = std::sync::Mutex::new(Vec::new());
const POOL_CAP: usize = 8;

unsafe fn conn_for(
    secure: bool,
    host: &str,
    port: u16,
    agent: &[u16],
    timeout_ms: i32,
) -> Result<(*mut c_void, *mut c_void), String> {
    let key = format!(
        "{}://{}:{}",
        if secure { "https" } else { "http" },
        host,
        port
    );
    {
        let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, ses, con)) = pool.iter().find(|(k, _, _)| k == &key) {
            return Ok((*ses as *mut c_void, *con as *mut c_void));
        }
        // 池满：关掉最旧的一条，避免句柄无限增长
        if pool.len() >= POOL_CAP {
            let (_, ses, con) = pool.remove(0);
            WinHttpCloseHandle(con as *mut c_void);
            WinHttpCloseHandle(ses as *mut c_void);
        }
    }
    let wh = wide(host);
    let session = WinHttpOpen(
        agent.as_ptr(),
        ACCESS_TYPE_AUTOMATIC_PROXY,
        std::ptr::null(),
        std::ptr::null(),
        0,
    );
    if session.is_null() {
        return Err("WinHttpOpen 失败".into());
    }
    WinHttpSetTimeouts(session, timeout_ms, timeout_ms, timeout_ms, timeout_ms);
    let connect = WinHttpConnect(session, wh.as_ptr(), port, 0);
    if connect.is_null() {
        WinHttpCloseHandle(session);
        return Err(format!("连接 {host}:{port} 失败"));
    }
    POOL.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((key, session as usize, connect as usize));
    Ok((session, connect))
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

pub struct Response {
    pub status: u16,
    pub headers: String,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    pub fn header(&self, name: &str) -> Option<String> {
        let lname = name.to_ascii_lowercase();
        for line in self.headers.split("\r\n") {
            if let Some((k, v)) = line.split_once(':') {
                if k.trim().to_ascii_lowercase() == lname {
                    return Some(v.trim().to_string());
                }
            }
        }
        None
    }
}

/// 解析 `http(s)://host[:port]/path?query`
fn split_url(url: &str) -> Result<(bool, String, u16, String), String> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(format!("不支持的地址：{url}"));
    };
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let default_port = if secure { 443 } else { 80 };
    let (host, port) = if let Some(rest) = hostport.strip_prefix('[') {
        // IPv6 字面量：[::1]:8080 / [::1]
        match rest.split_once(']') {
            Some((h, tail)) => {
                let p = match tail.strip_prefix(':') {
                    Some(t) => t.parse::<u16>().map_err(|_| "端口不合法")?,
                    None => default_port,
                };
                (h.to_string(), p)
            }
            None => return Err("IPv6 地址缺少右方括号".into()),
        }
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| "端口不合法")?),
            None => (hostport.to_string(), default_port),
        }
    };
    if host.is_empty() {
        return Err("地址缺主机名".into());
    }
    Ok((secure, host, port, path.to_string()))
}

/// 发一次请求。`headers` 传 `"k: v\r\nk2: v2"`（不需要结尾空行）。
pub fn request(
    method: &str,
    url: &str,
    headers: &str,
    body: Option<&[u8]>,
    timeout_ms: i32,
) -> Result<Response, String> {
    let (secure, host, port, path) = split_url(url)?;
    let agent = wide("PROJECT_ROOT/1.0");
    let wh = wide(&host);
    let wp = wide(&path);
    let wm = wide(method);
    let _ = &wh; // 连接池自己拼宽字符，这里保留变量以免改动调用点
    unsafe {
        let (_session, connect) = conn_for(secure, &host, port, &agent, timeout_ms)?;
        let flags = if secure { FLAG_SECURE } else { 0 };
        let req = WinHttpOpenRequest(
            connect,
            wm.as_ptr(),
            wp.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            flags,
        );
        if req.is_null() {
            return Err("WinHttpOpenRequest 失败".into());
        }
        let whdr = wide(headers);
        let (bptr, blen) = match body {
            Some(b) => (b.as_ptr() as *const c_void, b.len() as u32),
            None => (std::ptr::null(), 0),
        };
        let ok = WinHttpSendRequest(
            req,
            if headers.is_empty() { std::ptr::null() } else { whdr.as_ptr() },
            if headers.is_empty() { 0 } else { u32::MAX },
            bptr,
            blen,
            blen,
            0,
        );
        if ok == 0 {
            WinHttpCloseHandle(req);
            return Err("WinHttpSendRequest 失败".into());
        }
        if WinHttpReceiveResponse(req, std::ptr::null_mut()) == 0 {
            WinHttpCloseHandle(req);
            return Err("WinHttpReceiveResponse 失败".into());
        }
        let mut status: u32 = 0;
        let mut len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            req,
            QUERY_STATUS_CODE | QUERY_FLAG_NUMBER,
            std::ptr::null(),
            &mut status as *mut u32 as *mut c_void,
            &mut len,
            std::ptr::null_mut(),
        );
        // 两遍法：先问需要多大，再按需分配（原来固定 8192 u16 会截断超长响应头）
        let mut need: u32 = 0;
        WinHttpQueryHeaders(
            req,
            QUERY_RAW_HEADERS_CRLF,
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut need,
            std::ptr::null_mut(),
        );
        let cap_chars = ((need as usize / 2).max(1024) + 16).min(1 << 20);
        let mut hbuf = vec![0u16; cap_chars];
        let mut hlen = (hbuf.len() * 2) as u32;
        let hres = WinHttpQueryHeaders(
            req,
            QUERY_RAW_HEADERS_CRLF,
            std::ptr::null(),
            hbuf.as_mut_ptr() as *mut c_void,
            &mut hlen,
            std::ptr::null_mut(),
        );
        let headers_s = if hres != 0 {
            let n = (hlen as usize / 2).min(hbuf.len());
            String::from_utf16_lossy(&hbuf[..n])
        } else {
            String::new()
        };
        let mut body_out: Vec<u8> = Vec::new();
        let mut buf = vec![0u8; 65536];   // 循环外分配一次，复用
        let mut truncated = false;
        loop {
            let mut read: u32 = 0;
            let r = WinHttpReadData(req, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut read);
            if r == 0 || read == 0 {
                break;
            }
            body_out.extend_from_slice(&buf[..read as usize]);
            if body_out.len() > 64 * 1024 * 1024 {
                truncated = true;   // 不静默截断：下面返回错误
                break;
            }
        }
        WinHttpCloseHandle(req);
        if truncated {
            return Err("响应体超过 64MB 上限，已中止（不返回截断内容）".into());
        }
        Ok(Response { status: status as u16, headers: headers_s, body: body_out })
    }
}

pub fn get(url: &str, headers: &str) -> Result<Response, String> {
    request("GET", url, headers, None, 20000)
}

/// `--netcheck <url>`：验证 TLS / 内网 HTTP 是否可用（不碰任何业务数据）
pub fn selfcheck(url: &str) {
    let t0 = std::time::Instant::now();
    match get(url, "User-Agent: PROJECT_ROOT/1.0\r\nAccept: */*") {
        Ok(r) => {
            let body = r.text();
            let head: String = body.chars().take(160).collect();
            println!(
                "NETCHECK ok status={} bytes={} ms={} type={:?}",
                r.status,
                r.body.len(),
                t0.elapsed().as_millis(),
                r.header("content-type")
            );
            println!("--- 前 160 字 ---\n{}", head.replace('\n', " "));
        }
        Err(e) => {
            println!("NETCHECK fail err={} ms={}", e, t0.elapsed().as_millis());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_url;

    #[test]
    fn http_default_port() {
        assert_eq!(
            split_url("http://host/a/b").unwrap(),
            (false, "host".to_string(), 80, "/a/b".to_string())
        );
    }

    #[test]
    fn https_default_port() {
        assert_eq!(
            split_url("https://host").unwrap(),
            (true, "host".to_string(), 443, "/".to_string())
        );
    }

    #[test]
    fn explicit_port() {
        assert_eq!(
            split_url("http://host:8080/x").unwrap(),
            (false, "host".to_string(), 8080, "/x".to_string())
        );
    }

    #[test]
    fn ipv6_literal() {
        assert_eq!(
            split_url("http://[::1]:8080/api").unwrap(),
            (false, "::1".to_string(), 8080, "/api".to_string())
        );
        assert_eq!(
            split_url("http://[::1]").unwrap(),
            (false, "::1".to_string(), 80, "/".to_string())
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(split_url("ftp://host/x").is_err());
        assert!(split_url("http://host:abc/x").is_err());
        assert!(split_url("http://[::1/x").is_err());
        assert!(split_url("http:///x").is_err());
    }
}
