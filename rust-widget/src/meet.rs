//! 会议安排数据：读 `<data 同目录>/meeting_data.json`（由 research/ec_meeting_fetch.py 生成）。
//! 结构见该脚本输出：{at, today/week/month:{items:[{date,start,end,name}]}, all[]}
use crate::view::MeetRow;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Meet {
    pub at: String,
    /// 当前档位：0 当日 / 1 当周 / 2 当月
    pub span: u8,
    pub today: Vec<MeetRow>,
    pub week: Vec<MeetRow>,
    pub month: Vec<MeetRow>,
}

pub fn path_for(data: &Path) -> PathBuf {
    match data.parent() {
        Some(d) => d.join("meeting_data.json"),
        None => PathBuf::from("meeting_data.json"),
    }
}

fn rows(v: Option<&serde_json::Value>) -> Vec<MeetRow> {
    let mut out = Vec::new();
    let items = match v.and_then(|x| x.get("items")).and_then(|x| x.as_array()) {
        Some(a) => a,
        None => return out,
    };
    for it in items {
        let s = |k: &str| {
            it.get(k)
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let name = s("name");
        if name.is_empty() {
            continue;
        }
        out.push(MeetRow {
            date: s("date"),
            start: s("start"),
            end: s("end"),
            name,
            place: s("place"),
            people: s("people"),
        });
    }
    out
}

pub fn load(p: &Path) -> Option<Meet> {
    let txt = std::fs::read_to_string(p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&txt).ok()?;
    let at = v.get("at").and_then(|x| x.as_str()).unwrap_or("").to_string();
    Some(Meet {
        at,
        span: 0,
        today: rows(v.get("today")),
        week: rows(v.get("week")),
        month: rows(v.get("month")),
    })
}

#[cfg(test)]
mod tests {
    use super::rows;

    #[test]
    fn parses_items_and_skips_nameless() {
        let v = serde_json::json!({
            "items": [
                {"date": "2026-05-06", "start": "09:00", "end": "09:30", "name": " A ", "place": "R1", "people": "x,y"},
                {"date": "2026-05-06", "start": "10:00", "name": "  "},
                {"date": "2026-05-07"}
            ]
        });
        let got = rows(Some(&v));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "A");
        assert_eq!(got[0].place, "R1");
        assert_eq!(got[0].people, "x,y");
        assert_eq!(got[0].start, "09:00");
    }

    #[test]
    fn tolerates_missing_block() {
        let v = serde_json::json!({"at": "2026-05-06 10:00"});
        assert!(rows(Some(&v)).is_empty());
        assert!(rows(None).is_empty());
    }
}
