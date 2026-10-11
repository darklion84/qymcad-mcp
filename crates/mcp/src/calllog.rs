//! Local, bounded JSONL diagnostics shared by the synchronous transport and the panic hook.

use serde_json::{json, Value};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RECORD_LIMIT: usize = 256 * 1024;
const FILE_LIMIT: u64 = 64 * 1024 * 1024;
const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
// Bound work even when each pass makes only a tiny reduction in a wide record.
const MAX_TRUNCATION_ITERATIONS: usize = 32;
// Prefer a structural prefix when no single string can remove a useful chunk.
const STRING_SHRINK_CHUNK: usize = 64 * 1024;
// Keep a useful prefix and marker rather than repeatedly cutting tiny strings.
const MIN_STRING_BUDGET: usize = 64;
// Fixed-width timestamp/envelope lets normal writes reserve the final line without building it.
const LOG_LIMIT_ENVELOPE_BYTES: u64 = b"{\"kind\":\"log_limit\",\"limit\":,\"t\":\"1970-01-01T00:00:00.000Z\"}\n".len() as u64;

/// Cloneable handle to one process's unbuffered call log. Failures disable only logging.
#[derive(Clone)]
pub struct CallLog {
    path: PathBuf,
    writer: Arc<Mutex<LogWriter>>,
}

struct LogWriter {
    file: Option<File>,
    bytes: u64,
    limit: u64,
}

impl CallLog {
    /// Choose the environment-configured directory, prune old logs, and write the startup record.
    /// Returns `None` when explicitly off or when initialization fails, reporting on stderr only.
    pub fn start(app_note: &str) -> Option<Self> {
        let directory = log_directory(
            std::env::var_os("QYMCAD_MCP_LOG").as_deref(),
            std::env::var_os("HOME").as_deref(),
            std::env::var_os("XDG_STATE_HOME").as_deref(),
            cfg!(target_os = "macos"),
        );
        Self::start_with(directory, app_note, Self::open)
    }

    fn start_with(
        directory: io::Result<Option<PathBuf>>,
        app_note: &str,
        open: impl FnOnce(PathBuf, u64) -> io::Result<Self>,
    ) -> Option<Self> {
        let result = directory.and_then(|directory| {
            let Some(directory) = directory else {
                note("qymcad-mcp: call log: off");
                return Ok(None);
            };
            fs::create_dir_all(&directory)?;
            prune(&directory, SystemTime::now());
            let path = directory.join(file_name(SystemTime::now(), std::process::id()));
            let log = open(path, FILE_LIMIT)?;
            if !log.record(start_record(app_note, log.path(), std::env::current_dir())) {
                return Ok(None); // The writer already reported the failure once.
            }
            note(&format!("qymcad-mcp: call log: {}", log.path.display()));
            Ok(Some(log))
        });
        match result {
            Ok(log) => log,
            Err(error) => {
                note(&format!("qymcad-mcp: call log disabled: {error}"));
                None
            }
        }
    }

    fn open(path: PathBuf, limit: u64) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let bytes = file.metadata()?.len();
        Ok(Self { path, writer: Arc::new(Mutex::new(LogWriter { file: Some(file), bytes, limit })) })
    }

    /// File announced at startup; also usable when constructing client instructions.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write one complete timestamped line, visible on disk before this method returns.
    /// Returns whether the requested record was written; false when logging is disabled.
    pub fn record(&self, record: Value) -> bool {
        let line = record_line(record);
        self.writer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).write(&line)
    }

    /// Install the process panic hook, preserving the previous hook and avoiding a recursive lock.
    /// Main calls this only when a log has been opened.
    pub fn install_panic_hook(&self) {
        let log = self.clone();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let message = info
                .payload()
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| info.payload().downcast_ref::<&str>().copied())
                .unwrap_or("non-string panic payload");
            let thread = std::thread::current();
            let line = record_line(json!({
                "kind": "panic", "message": message,
                "location": info.location().map(|location| format!("{}:{}", location.file(), location.line())),
                "thread": thread.name().map(str::to_owned).unwrap_or_else(|| format!("{:?}", thread.id())),
            }));
            match log.writer.try_lock() {
                Ok(mut writer) => {
                    writer.write(&line);
                }
                Err(TryLockError::Poisoned(poisoned)) => {
                    poisoned.into_inner().write(&line);
                }
                Err(TryLockError::WouldBlock) => {} // Never deadlock on a panic inside a logging write.
            }
            previous(info);
        }));
    }
}

impl LogWriter {
    fn write(&mut self, line: &[u8]) -> bool {
        let Some(file) = self.file.as_mut() else { return false };
        let reserved_bytes = LOG_LIMIT_ENVELOPE_BYTES + u64::from(self.limit.checked_ilog10().unwrap_or(0) + 1);
        let at_limit = self.bytes.saturating_add(line.len() as u64).saturating_add(reserved_bytes) > self.limit;
        let final_line = if at_limit { record_line(json!({"kind": "log_limit", "limit": self.limit})) } else { Vec::new() };
        let writing = if at_limit { final_line.as_slice() } else { line };
        // Reserve the final line on normal writes; an existing append file may already be full.
        let result = if self.bytes.saturating_add(writing.len() as u64) <= self.limit { file.write_all(writing) } else { Ok(()) };
        match result {
            Err(error) => {
                self.file = None;
                note(&format!("qymcad-mcp: call log disabled: {error}"));
            }
            Ok(()) if at_limit => {
                self.file = None;
                note(&format!("qymcad-mcp: call log disabled: file limit ({} bytes)", self.limit));
            }
            Ok(()) => {
                self.bytes += writing.len() as u64;
                return true;
            }
        }
        false
    }
}

fn start_record(app_note: &str, log_path: &Path, cwd: io::Result<PathBuf>) -> Value {
    json!({
        "kind": "start", "version": env!("CARGO_PKG_VERSION"),
        "qymcad": qymcad_engine::QYMCAD_VERSION, "pid": std::process::id(),
        "cwd": cwd.ok().map(|path| path.to_string_lossy().into_owned()), "app_note": app_note,
        "log_path": log_path.to_string_lossy(),
    })
}

fn note(message: &str) {
    let _ = writeln!(io::stderr().lock(), "{message}");
}

fn log_directory(log: Option<&OsStr>, home: Option<&OsStr>, state: Option<&OsStr>, macos: bool) -> io::Result<Option<PathBuf>> {
    if let Some(value) = log.filter(|value| !value.is_empty()) {
        if value.to_str().is_some_and(|value| ["off", "0", "false", "no"].iter().any(|off| value.eq_ignore_ascii_case(off))) {
            return Ok(None);
        }
        return Ok(Some(PathBuf::from(value)));
    }
    if !macos {
        if let Some(state) = state.filter(|value| !value.is_empty()) {
            return Ok(Some(PathBuf::from(state).join("qymcad-mcp")));
        }
    }
    let home = home.filter(|value| !value.is_empty()).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    Ok(Some(PathBuf::from(home).join(if macos { "Library/Logs/qymcad-mcp" } else { ".local/state/qymcad-mcp" })))
}

// Howard Hinnant's civil_from_days: Gregorian date from days relative to 1970-01-01.
fn civil_date(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn timestamp(time: SystemTime) -> String {
    let millis = match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as i128,
        Err(error) => -(error.duration().as_millis() as i128),
    };
    let seconds = millis.div_euclid(1000);
    let (year, month, day) = civil_date(seconds.div_euclid(86400) as i64);
    let tod = seconds.rem_euclid(86400);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", tod / 3600, tod / 60 % 60, tod % 60, millis.rem_euclid(1000))
}

fn file_name(time: SystemTime, pid: u32) -> String {
    let t = timestamp(time);
    format!("qymcad-mcp-{}T{}Z-{pid}.jsonl", t[..10].replace('-', ""), t[11..19].replace(':', ""))
}

fn retention_name(name: &str) -> bool {
    name.starts_with("qymcad-mcp-") && name.ends_with(".jsonl")
}

fn retention_selection(files: &[(String, SystemTime)], now: SystemTime) -> Vec<String> {
    let mut candidates: Vec<_> = files.iter().filter(|(name, _)| retention_name(name)).collect();
    candidates.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let recent_count = candidates.iter().filter(|(_, mtime)| now.duration_since(*mtime).unwrap_or_default() <= MAX_AGE).count();
    let mut excess = recent_count.saturating_sub(200);
    candidates
        .into_iter()
        .filter(|(_, mtime)| {
            if now.duration_since(*mtime).unwrap_or_default() > MAX_AGE {
                true
            } else if excess > 0 {
                excess -= 1;
                true
            } else {
                false
            }
        })
        .map(|(name, _)| name.clone())
        .collect()
}

fn prune(directory: &Path, now: SystemTime) {
    let mut failed = false;
    let mut files = Vec::new();
    match fs::read_dir(directory) {
        Ok(entries) => {
            for entry in entries {
                let result = entry.and_then(|entry| {
                    let kind = entry.file_type()?;
                    if !kind.is_file() {
                        return Ok(());
                    }
                    let name = entry.file_name();
                    let Some(name) = name.to_str().filter(|name| retention_name(name)) else { return Ok(()) };
                    files.push((name.to_owned(), entry.metadata()?.modified()?));
                    Ok(())
                });
                failed |= result.is_err();
            }
        }
        Err(_) => failed = true,
    }
    for name in retention_selection(&files, now) {
        failed |= fs::remove_file(directory.join(name)).is_err();
    }
    if failed {
        note("qymcad-mcp: call log retention: some files could not be inspected or deleted");
    }
}

/// Redact images from a result's content without changing text or the protocol response.
/// `base64_bytes` counts UTF-8 bytes in the original base64 string, not decoded image bytes.
pub fn redact_images(content: &Value) -> Value {
    let Some(items) = content.as_array() else { return content.clone() };
    Value::Array(
        items
            .iter()
            .map(|item| {
                if item.get("type").and_then(Value::as_str) == Some("image") {
                    json!({"type": "image", "mimeType": item.get("mimeType").cloned().unwrap_or(Value::Null),
                "base64_bytes": item.get("data").and_then(Value::as_str).map_or(0, str::len)})
                } else {
                    item.clone()
                }
            })
            .collect(),
    )
}

fn record_line(mut record: Value) -> Vec<u8> {
    if !record.is_object() {
        record = json!({"kind":"diagnostic", "value":record});
    }
    if !record.get("kind").is_some_and(Value::is_string) {
        record["kind"] = json!("diagnostic");
    }
    record["t"] = json!(timestamp(SystemTime::now()));
    bounded_line(record, RECORD_LIMIT)
}

// Largest strings are cut on UTF-8 boundaries using their serialized (escaped) size.
// If structural JSON alone exceeds the budget, retain a marked JSON-text prefix of the largest field.
fn bounded_line(mut record: Value, limit: usize) -> Vec<u8> {
    let line = format!("{record}\n").into_bytes();
    if line.len() < limit {
        return line;
    }
    let Some(fields) = record.as_object() else { return truncated_envelope(&record, line.len()) };
    let mut sizes: Vec<_> =
        fields.iter().filter(|(key, _)| *key != "t" && *key != "kind").map(|(key, value)| (key.clone(), value.to_string().len())).collect();
    let mut line_bytes = line.len();
    for _ in 0..MAX_TRUNCATION_ITERATIONS {
        let excess = line_bytes - limit + 1;
        let Some((key, size)) = sizes.iter_mut().max_by_key(|(_, size)| *size) else {
            return truncated_envelope(&record, line_bytes);
        };
        let biggest = &mut record[key.as_str()];
        let budget = size.saturating_sub(excess);
        if !shrink_string(biggest, budget, *size) {
            let serialized = biggest.to_string();
            *biggest = Value::String(cut_string(&serialized, budget));
        }
        let new_size = biggest.to_string().len();
        // Oversized top-level keys or protected fields cannot be fixed by cutting values.
        if new_size >= *size {
            return truncated_envelope(&record, line_bytes);
        }
        // Only this field changed; JSON separators and keys retain their serialized size.
        line_bytes -= *size - new_size;
        *size = new_size;
        if line_bytes < limit {
            return format!("{record}\n").into_bytes();
        }
    }
    truncated_envelope(&record, line_bytes)
}

fn truncated_envelope(record: &Value, bytes: usize) -> Vec<u8> {
    let kind = record.get("kind").and_then(Value::as_str).filter(|kind| kind.len() <= 24).unwrap_or("truncated");
    let t = record.get("t").and_then(Value::as_str).filter(|t| t.len() <= 24).unwrap_or("1970-01-01T00:00:00.000Z");
    format!("{}\n", json!({"kind":kind, "t":t, "message":format!("…[truncated {bytes} bytes]")})).into_bytes()
}

fn largest_string(value: &mut Value) -> Option<&mut String> {
    match value {
        Value::String(string) => Some(string),
        Value::Array(items) => items.iter_mut().filter_map(largest_string).max_by_key(|s| json!(s).to_string().len()),
        Value::Object(fields) => fields.values_mut().filter_map(largest_string).max_by_key(|s| json!(s).to_string().len()),
        _ => None,
    }
}

fn shrink_string(value: &mut Value, budget: usize, total: usize) -> bool {
    let Some(string) = largest_string(value) else { return false };
    let string_size = json!(string).to_string().len();
    let overhead = total - string_size;
    // Do not iterate over thousands of tiny strings when structural fallback can bound the field once.
    if string_size < total.saturating_sub(budget).min(STRING_SHRINK_CHUNK) || string_size <= MIN_STRING_BUDGET {
        return false;
    }
    *string = cut_string(string, budget.saturating_sub(overhead).max(MIN_STRING_BUDGET));
    true
}

fn cut_string(string: &str, budget: usize) -> String {
    let mut low = 0;
    let mut high = string.len();
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        let mut end = mid;
        while !string.is_char_boundary(end) {
            end -= 1
        }
        let candidate = format!("{}…[truncated {} bytes]", &string[..end], string.len() - end);
        if json!(candidate).to_string().len() <= budget {
            low = mid
        } else {
            high = mid - 1
        }
    }
    while !string.is_char_boundary(low) {
        low -= 1
    }
    format!("{}…[truncated {} bytes]", &string[..low], string.len() - low)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("qymcad-calllog-unit-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn log(&self, limit: u64) -> CallLog {
            CallLog::open(self.0.join("log.jsonl"), limit).unwrap()
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn startup_write_failure_returns_none_and_reports_once() {
        const CHILD_DIR: &str = "QYMCAD_TEST_START_DIR";
        if let Some(directory) = std::env::var_os(CHILD_DIR) {
            let log = CallLog::start_with(Ok(Some(PathBuf::from(directory))), "app note", |path, limit| {
                fs::write(&path, "")?;
                let writer = LogWriter { file: Some(File::open(&path)?), bytes: 0, limit };
                Ok(CallLog { path, writer: Arc::new(Mutex::new(writer)) })
            });
            assert!(log.is_none(), "failed startup record must not return an active log");
            assert!(!crate::instructions_with_log("app note", log.as_ref().map(CallLog::path)).contains("Call log of this server process"));
            return;
        }
        let temp = TempDir::new();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "calllog::tests::startup_write_failure_returns_none_and_reports_once", "--nocapture"])
            .env(CHILD_DIR, &temp.0)
            .env("QYMCAD_MCP_LOG", "off")
            .output()
            .unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{stderr}");
        let notes: Vec<_> = stderr.lines().collect();
        assert_eq!(notes.len(), 1, "exactly one stderr line: {stderr}");
        assert!(notes[0].starts_with("qymcad-mcp: call log disabled: "), "{stderr}");
        for entry in fs::read_dir(&temp.0).unwrap() {
            assert!(fs::read(entry.unwrap().path()).unwrap().is_empty());
        }
    }

    #[test]
    fn pathological_medium_strings_are_bounded_quickly() {
        let args = Value::Array((0..2048).map(|i| json!("x".repeat(65 + i % 936))).collect());
        let record = json!({"kind":"call", "t":"1970-01-01T00:00:00.000Z", "args":args});
        assert_pathological_record(record, "medium strings", false);
    }

    #[test]
    fn pathological_nested_fields_hit_iteration_cap_quickly() {
        let mut record = json!({"kind":"call", "t":"1970-01-01T00:00:00.000Z"});
        for i in 0..2048 {
            record[format!("args_{i:04}")] = json!({"outer":{"inner":["x".repeat(512)]}});
        }
        assert_pathological_record(record, "nested fields", true);
    }

    fn assert_pathological_record(record: Value, name: &str, expect_envelope: bool) {
        let original_bytes = record.to_string().len() + 1;
        let start = std::time::Instant::now();
        let line = bounded_line(record, RECORD_LIMIT);
        let elapsed = start.elapsed();
        eprintln!("{name}: {original_bytes} input bytes, {} output bytes, {elapsed:?}", line.len());
        assert!(line.len() < RECORD_LIMIT);
        assert_eq!(line.last(), Some(&b'\n'));
        let record: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(record["kind"], "call");
        assert_eq!(record["t"], "1970-01-01T00:00:00.000Z");
        assert!(elapsed < Duration::from_millis(200), "{name} took {elapsed:?}, must finish in <200 ms");
        if expect_envelope {
            assert!(
                record["message"].as_str().is_some_and(|message| message.contains("…[truncated ")),
                "iteration cap returns a truncated envelope"
            );
            assert_eq!(record.as_object().unwrap().len(), 3);
        }
    }

    #[test]
    fn start_record_keeps_logging_when_cwd_is_unavailable() {
        let path = Path::new("/logs/process.jsonl");
        let record = start_record("app note", path, Err(io::Error::new(io::ErrorKind::NotFound, "deleted working directory")));
        assert_eq!(record["cwd"], Value::Null, "unavailable cwd is null");
        assert_eq!(record["kind"], "start");
        assert_eq!(record["log_path"], json!(path));
        assert_eq!(record["app_note"], "app note");
        assert_eq!(record["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(record["qymcad"], qymcad_engine::QYMCAD_VERSION);
        assert_eq!(record["pid"], std::process::id());
        let record = start_record("app note", path, Ok(PathBuf::from("/existing/cwd")));
        assert_eq!(record["cwd"], "/existing/cwd", "available cwd is preserved");
    }

    #[test]
    fn fixed_utc_instants_and_filename() {
        for (ms, expected) in [
            (0, "1970-01-01T00:00:00.000Z"),
            (951782400000, "2000-02-29T00:00:00.000Z"),
            (1791727387123, "2026-10-11T14:03:07.123Z"),
            (2208988800000, "2040-01-01T00:00:00.000Z"),
        ] {
            assert_eq!(timestamp(UNIX_EPOCH + Duration::from_millis(ms)), expected);
        }
        assert_eq!(timestamp(UNIX_EPOCH - Duration::from_millis(1)), "1969-12-31T23:59:59.999Z");
        assert_eq!(file_name(UNIX_EPOCH + Duration::from_millis(1791727387123), 42), "qymcad-mcp-20261011T140307Z-42.jsonl");
        assert!(retention_name("qymcad-mcp-anything.jsonl"));
        assert!(!retention_name("other-qymcad-mcp-a.jsonl"));
        assert!(!retention_name("qymcad-mcp-a.jsonl.bak"));
    }

    #[test]
    fn directory_selection() {
        let home = Some(OsStr::new("/home/test"));
        let state = Some(OsStr::new("/state"));
        for off in ["off", "OFF", "fAlSe", "0", "no", "NO"] {
            assert_eq!(log_directory(Some(OsStr::new(off)), None, None, true).unwrap(), None);
        }
        for log in [None, Some(OsStr::new(""))] {
            assert_eq!(log_directory(log, home, state, true).unwrap().unwrap(), Path::new("/home/test/Library/Logs/qymcad-mcp"));
            assert_eq!(log_directory(log, home, state, false).unwrap().unwrap(), Path::new("/state/qymcad-mcp"));
            assert_eq!(log_directory(log, home, None, false).unwrap().unwrap(), Path::new("/home/test/.local/state/qymcad-mcp"));
        }
        assert_eq!(log_directory(Some(OsStr::new("custom")), None, None, true).unwrap().unwrap(), Path::new("custom"));
        assert!(log_directory(None, None, None, true).is_err());
    }

    #[test]
    fn truncation_preserves_json_utf8_and_small_fields() {
        let original = "é\n\"\\".repeat(100_000);
        let value =
            json!({"kind":"call", "t":"2026-10-11T14:03:07.123Z", "seq":1, "tool":"test", "args":{"large":original, "small":"kept"}});
        let line = bounded_line(value, RECORD_LIMIT);
        assert!(line.len() < RECORD_LIMIT);
        assert_eq!(line.last(), Some(&b'\n'));
        let record: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(record["seq"], 1);
        assert_eq!(record["tool"], "test");
        assert_eq!(record["args"]["small"], "kept");
        let cut = record["args"]["large"].as_str().unwrap();
        assert!(cut.contains("…[truncated "));
        let marker = cut.rfind("…[truncated ").unwrap();
        let count: usize = cut[marker + "…[truncated ".len()..].strip_suffix(" bytes]").unwrap().parse().unwrap();
        assert_eq!(count, original.len() - marker);
        let small = json!({"kind":"call", "args":{"x":"\""}});
        assert_eq!(bounded_line(small.clone(), RECORD_LIMIT), format!("{small}\n").into_bytes());
    }

    #[test]
    fn truncation_bounds_structural_json_and_huge_keys() {
        for args in [json!([0]), Value::Array(vec![json!(0); 200_000]), json!({"x".repeat(300_000): 0})] {
            let line = bounded_line(json!({"kind":"call", "t":"1970-01-01T00:00:00.000Z", "seq":1, "args":args}), RECORD_LIMIT);
            assert!(line.len() < RECORD_LIMIT);
            let record: Value = serde_json::from_slice(&line).unwrap();
            assert_eq!(record["kind"], "call");
            assert_eq!(record["seq"], 1);
            assert!(record["t"].is_string());
        }
    }

    #[test]
    fn images_have_only_mime_and_base64_byte_count() {
        let content = json!([
            {"type":"image", "mimeType":"image/png", "data":"YWJjZA==", "extra":"removed"},
            {"type":"text", "text":"kept", "annotations":{"audience":["user"]}},
            {"type":"image", "mimeType":"image/jpeg", "data":""}
        ]);
        let redacted = redact_images(&content);
        assert_eq!(redacted[0], json!({"type":"image", "mimeType":"image/png", "base64_bytes":8}));
        assert_eq!(redacted[1], content[1]);
        assert_eq!(redacted[2], json!({"type":"image", "mimeType":"image/jpeg", "base64_bytes":0}));
        assert_eq!(content[0]["data"], "YWJjZA==");
    }

    #[test]
    fn retention_age_cap_foreign_files_and_new_file_exclusion() {
        let now = UNIX_EPOCH + Duration::from_secs(100 * 86400);
        let mut files = vec![
            ("qymcad-mcp-old.jsonl".into(), now - MAX_AGE - Duration::from_secs(1)),
            ("foreign.jsonl".into(), UNIX_EPOCH),
            ("qymcad-mcp-note.txt".into(), UNIX_EPOCH),
        ];
        for i in 0..202 {
            files.push((format!("qymcad-mcp-{i:03}.jsonl"), now - MAX_AGE + Duration::from_secs(i)));
        }
        let deleted = retention_selection(&files, now);
        assert_eq!(deleted, ["qymcad-mcp-old.jsonl", "qymcad-mcp-000.jsonl", "qymcad-mcp-001.jsonl"]);
        let kept: Vec<_> = files.into_iter().filter(|(name, _)| !deleted.contains(name)).collect();
        assert!(retention_selection(&kept, now).is_empty(), "200 existing files retained before new file is created");
        assert!(retention_selection(
            &[("qymcad-mcp-boundary.jsonl".into(), now - MAX_AGE), ("qymcad-mcp-future.jsonl".into(), now + MAX_AGE)],
            now
        )
        .is_empty());
    }

    #[test]
    fn call_is_on_disk_before_result_without_closing_log() {
        let temp = TempDir::new();
        let log = temp.log(FILE_LIMIT);
        log.record(json!({"kind":"call", "seq":1, "tool":"blocked", "args":{}}));
        let text = fs::read_to_string(log.path()).unwrap();
        let records: Vec<Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["kind"], "call");
        log.record(json!({"kind":"result", "seq":1}));
        assert_eq!(fs::read_to_string(log.path()).unwrap().lines().count(), 2);
    }

    #[test]
    fn file_limit_reserves_exactly_one_final_line() {
        let temp = TempDir::new();
        let path = temp.0.join("boundary.jsonl");
        let limit = 512;
        let call = json!({"kind":"call", "seq":1});
        let call_bytes = record_line(call.clone()).len();
        let final_bytes = record_line(json!({"kind":"log_limit", "limit":limit})).len();
        let existing_bytes = limit as usize - call_bytes - final_bytes;
        fs::write(&path, vec![b' '; existing_bytes]).unwrap();
        let log = CallLog::open(path, limit).unwrap();
        assert!(log.record(call), "a call fitting exactly with the reserved line must be written");
        assert!(!log.record(json!({"kind":"result", "seq":1})), "the requested result is replaced by log_limit");
        assert!(!log.record(json!({"kind":"stop"})), "disabled writes return false");
        let bytes = fs::read(log.path()).unwrap();
        assert_eq!(bytes.len(), limit as usize);
        let records: Vec<Value> = bytes[existing_bytes..]
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["kind"], "call");
        assert_eq!(records[1]["kind"], "log_limit");
    }

    #[test]
    fn cap_writes_one_final_line_and_disables_writes() {
        let temp = TempDir::new();
        let log = temp.log(512);
        for seq in 0..20 {
            log.record(json!({"kind":"call", "seq":seq, "args":{"value":"x".repeat(80)}}));
        }
        let before = fs::read(log.path()).unwrap();
        assert!(before.len() <= 512);
        let records: Vec<Value> =
            before.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()).map(|line| serde_json::from_slice(line).unwrap()).collect();
        assert_eq!(records.last().unwrap()["kind"], "log_limit");
        assert_eq!(records.iter().filter(|record| record["kind"] == "log_limit").count(), 1);
        assert!(log.writer.lock().unwrap().file.is_none());
        log.record(json!({"kind":"stop", "reason":"eof"}));
        assert_eq!(before, fs::read(log.path()).unwrap());
    }

    #[test]
    fn append_mode_keeps_existing_lines() {
        let temp = TempDir::new();
        let log = temp.log(FILE_LIMIT);
        log.record(json!({"kind":"call", "seq":1}));
        drop(log);
        let log = temp.log(FILE_LIMIT);
        log.record(json!({"kind":"result", "seq":1}));
        assert_eq!(fs::read_to_string(log.path()).unwrap().lines().count(), 2);
    }

    #[test]
    fn write_failure_disables_future_writes() {
        let temp = TempDir::new();
        let path = temp.0.join("readonly.jsonl");
        fs::write(&path, "").unwrap();
        let writer = LogWriter { file: Some(File::open(&path).unwrap()), bytes: 0, limit: FILE_LIMIT };
        let log = CallLog { path, writer: Arc::new(Mutex::new(writer)) };
        assert!(!log.record(json!({"kind":"call"})));
        assert!(log.writer.lock().unwrap().file.is_none());
        assert!(!log.record(json!({"kind":"result"})));
        assert_eq!(fs::read(log.path()).unwrap().len(), 0);
    }

    #[test]
    fn poisoned_log_mutex_is_recovered() {
        let temp = TempDir::new();
        let log = temp.log(FILE_LIMIT);
        let writer = log.writer.clone();
        let _ = std::thread::spawn(move || {
            let _guard = writer.lock().unwrap();
            panic!("poison test");
        })
        .join();
        log.record(json!({"kind":"panic", "message":"recovered"}));
        assert_eq!(serde_json::from_str::<Value>(&fs::read_to_string(log.path()).unwrap()).unwrap()["message"], "recovered");
    }

    #[test]
    fn transport_logs_read_write_and_flush_errors() {
        use crate::{tools::Registry, transport::Server};
        struct FailingOutput(bool);
        impl Write for FailingOutput {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.0 {
                    Ok(bytes.len())
                } else {
                    Err(io::Error::other("write failed"))
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("flush failed"))
            }
        }
        let message = b"{\"id\":1,\"method\":\"initialize\"}\n";
        for fail_flush in [false, true] {
            let temp = TempDir::new();
            let log = temp.log(FILE_LIMIT);
            let mut server = Server::with_log(Registry::new(), String::new(), Some(log.clone()));
            let error = server.run(&message[..], FailingOutput(fail_flush)).unwrap_err();
            let records: Vec<Value> =
                fs::read_to_string(log.path()).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
            assert_eq!(records.len(), 2);
            assert_eq!(records[0]["kind"], "initialize");
            assert_eq!(records[1]["kind"], "stop");
            assert_eq!(records[1]["reason"], "io_error");
            assert_eq!(records[1]["error"], error.to_string());
        }
        let temp = TempDir::new();
        let log = temp.log(FILE_LIMIT);
        let mut server = Server::with_log(Registry::new(), String::new(), Some(log.clone()));
        let error = server.run(&b"\xff\n"[..], Vec::new()).unwrap_err();
        let record: Value = serde_json::from_str(&fs::read_to_string(log.path()).unwrap()).unwrap();
        assert_eq!(record["kind"], "stop");
        assert_eq!(record["reason"], "io_error");
        assert_eq!(record["error"], error.to_string());
    }

    // Run global panic-hook changes in an isolated test process, not alongside other Rust tests.
    #[test]
    fn panic_hook_records_chains_and_recovers_poison_without_deadlock() {
        const CHILD_LOG: &str = "QYMCAD_TEST_PANIC_LOG";
        if let Some(path) = std::env::var_os(CHILD_LOG) {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                note("previous panic hook ran");
                previous(info);
            }));
            let log = CallLog::open(PathBuf::from(path), FILE_LIMIT).unwrap();
            log.install_panic_hook();
            let _ = std::panic::catch_unwind(|| panic!("first panic"));
            let writer = log.writer.clone();
            let _ = std::thread::Builder::new()
                .name("poison-worker".into())
                .spawn(move || {
                    let _guard = writer.lock().unwrap();
                    panic!("panic while holding log lock");
                })
                .unwrap()
                .join();
            let _ = std::panic::catch_unwind(|| panic!("panic after poison"));
            return;
        }
        let temp = TempDir::new();
        let path = temp.0.join("panic.jsonl");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "calllog::tests::panic_hook_records_chains_and_recovers_poison_without_deadlock", "--nocapture"])
            .env(CHILD_LOG, &path)
            .env("QYMCAD_MCP_LOG", "off")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stderr).matches("previous panic hook ran").count(), 3);
        let records: Vec<Value> = fs::read_to_string(path).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(records.len(), 2, "a hook entered while the log is locked must skip logging instead of deadlocking");
        assert_eq!(records[0]["message"], "first panic");
        assert_eq!(records[1]["message"], "panic after poison");
        for record in records {
            assert_eq!(record["kind"], "panic");
            assert!(record["location"].as_str().unwrap().contains("calllog.rs:"));
            assert!(record["thread"].as_str().unwrap().contains("panic_hook_records"));
            assert!(record["t"].as_str().unwrap().ends_with('Z'));
        }
    }

    #[test]
    fn truncation_handles_oversized_envelope_without_losing_timestamp() {
        for record in [
            json!({"kind":"x".repeat(300_000), "t":"1970-01-01T00:00:00.000Z"}),
            json!({"kind":"call", "t":"1970-01-01T00:00:00.000Z", "x".repeat(300_000):null}),
        ] {
            let line = bounded_line(record, RECORD_LIMIT);
            assert!(line.len() < RECORD_LIMIT);
            let record: Value = serde_json::from_slice(&line).unwrap();
            assert_eq!(record["t"], "1970-01-01T00:00:00.000Z", "fallback preserves timestamp");
            assert!(record["kind"].is_string());
            assert!(record.to_string().contains("…[truncated "));
        }
    }

    #[test]
    fn truncation_cuts_multiple_large_strings_without_replacing_their_object() {
        let line = bounded_line(
            json!({"kind":"call", "t":"1970-01-01T00:00:00.000Z", "args":{"a":"a".repeat(300_000), "b":"b".repeat(300_000)}}),
            RECORD_LIMIT,
        );
        assert!(line.len() < RECORD_LIMIT);
        let record: Value = serde_json::from_slice(&line).unwrap();
        assert!(record["args"].is_object());
        assert!(record["args"]["a"].as_str().unwrap().contains("…[truncated "));
        assert!(record["args"]["b"].as_str().unwrap().contains("…[truncated "));
    }

    #[test]
    fn retention_io_prunes_before_new_log_and_preserves_foreign_entries() {
        let temp = TempDir::new();
        let now = SystemTime::now();
        let old = temp.0.join("qymcad-mcp-old.jsonl");
        fs::write(&old, "old").unwrap();
        File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(now - MAX_AGE - Duration::from_secs(1)))
            .unwrap();
        let foreign = temp.0.join("notes.txt");
        fs::write(&foreign, "keep").unwrap();
        let directory = temp.0.join("qymcad-mcp-directory.jsonl");
        fs::create_dir(&directory).unwrap();
        let symlink = temp.0.join("qymcad-mcp-link.jsonl");
        std::os::unix::fs::symlink(&foreign, &symlink).unwrap();
        for i in 0..201 {
            let path = temp.0.join(format!("qymcad-mcp-{i:03}.jsonl"));
            fs::write(&path, "recent").unwrap();
            File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(now - Duration::from_secs(1000 - i)))
                .unwrap();
        }
        prune(&temp.0, now);
        assert!(!old.exists());
        assert!(!temp.0.join("qymcad-mcp-000.jsonl").exists());
        assert!(directory.is_dir());
        assert!(fs::symlink_metadata(symlink).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(foreign).unwrap(), "keep");
        let new_path = temp.0.join(file_name(now, std::process::id()));
        let _log = CallLog::open(new_path, FILE_LIMIT).unwrap();
        assert_eq!(
            fs::read_dir(&temp.0)
                .unwrap()
                .filter(|entry| {
                    let entry = entry.as_ref().unwrap();
                    entry.file_type().unwrap().is_file() && retention_name(&entry.file_name().to_string_lossy())
                })
                .count(),
            201,
            "new process log is not included in the 200-file retention cap"
        );
    }
}
