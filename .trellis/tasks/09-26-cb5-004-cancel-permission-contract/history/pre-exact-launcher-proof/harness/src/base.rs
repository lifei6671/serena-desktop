use super::*;
/// 递归读取全部文件（包括隐藏项）与目录；链接不能伪装成隔离证明。
pub fn manifest(root: &Path) -> io::Result<BTreeMap<String, Value>> {
    fn visit(root: &Path, dir: &Path, out: &mut BTreeMap<String, Value>) -> io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path)?;
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            #[cfg(windows)]
            let reparse = {
                use std::os::windows::fs::MetadataExt;
                meta.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let reparse = false;
            if meta.file_type().is_symlink() || reparse {
                return Err(io::Error::other("workspace contains link"));
            }
            if meta.is_dir() {
                out.insert(relative, json!({"kind":"directory"}));
                visit(root, &path, out)?;
            } else {
                let bytes = std::fs::read(&path)?;
                out.insert(relative, json!({"kind":"file","bytes":bytes}));
            }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result)?;
    Ok(result)
}

/// 比较完整路径集合与内容；新增隐藏目录同样是 delta。
pub fn delta(before: &BTreeMap<String, Value>, after: &BTreeMap<String, Value>) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|key| before.get(*key) != after.get(*key))
        .cloned()
        .collect()
}

/// metadata/providerData 只保留公开关联 ID，任意私有字段不落盘。
fn retain_correlations(value: &mut Value) {
    if let Some(map) = value.as_object_mut() {
        map.retain(|key, value| {
            let field = key.rsplit('/').next().unwrap_or(key).to_ascii_lowercase();
            if [
                "conversationrequestid",
                "requestid",
                "sessionid",
                "toolcallid",
            ]
            .contains(&field.as_str())
            {
                return value.is_string() || value.is_number();
            }
            if value.is_object() {
                retain_correlations(value);
                return value.as_object().is_some_and(|m| !m.is_empty());
            }
            false
        });
    } else {
        *value = Value::Null;
    }
}

/// 只剔除秘密/任意 metadata；公开协议、受控测试正文与身份保持原值。
fn sanitize(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                let lower = key.to_lowercase();
                if [
                    "token",
                    "accesstoken",
                    "refreshtoken",
                    "authorization",
                    "password",
                    "secret",
                    "apikey",
                    "api_key",
                    "credential",
                    "credentials",
                    "env",
                ]
                .contains(&lower.as_str())
                {
                    *value = json!("[REDACTED]");
                } else if key == "_meta" || lower == "providerdata" {
                    retain_correlations(value);
                } else {
                    sanitize(value);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(sanitize),
        _ => {}
    }
}

/// 逐分片恢复 NDJSON；保存完成帧的跨方向次序，非 JSON 只计字节避免泄密。
pub fn wire_records(wire: &Wire) -> Vec<Value> {
    let (mut tx, mut rx, mut rows) = (Vec::new(), Vec::new(), Vec::new());
    for (direction, bytes) in wire.lock().unwrap().iter() {
        let buffer = if direction == "request" {
            &mut tx
        } else {
            &mut rx
        };
        buffer.extend(bytes);
        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            if let Ok(raw) = serde_json::from_slice::<Value>(&line) {
                let mut safe = raw.clone();
                sanitize(&mut safe);
                let redacted = raw != safe;
                let raw_line = if redacted {
                    format!("{safe}\n")
                } else {
                    String::from_utf8_lossy(&line).into_owned()
                };
                rows.push(json!({"sequence":rows.len()+1,"direction":direction,"message":safe,"rawLine":raw_line,"redacted":redacted}));
            } else {
                rows.push(json!({"sequence":rows.len()+1,"direction":direction,"nonJsonBytes":line.len()}));
            }
        }
    }
    for (direction, bytes) in [("request", tx), ("response", rx)] {
        if !bytes.is_empty() {
            rows.push(json!({"sequence":rows.len()+1,"direction":direction,"incompleteFrameBytes":bytes.len()}));
        }
    }
    rows
}

/// 通过实际 RPC id 找响应，避免 update 或相邻 request 被误认为 terminal。
pub fn exchange(rows: &[Value], method: &str) -> Value {
    let request = rows
        .iter()
        .find(|r| r["direction"] == "request" && r["message"]["method"] == method);
    let response = request.and_then(|q| {
        rows.iter().find(|r| {
            r["direction"] == "response"
                && r["message"].get("id") == q["message"].get("id")
                && r["message"].get("method").is_none()
        })
    });
    json!({"request":request,"response":response})
}
