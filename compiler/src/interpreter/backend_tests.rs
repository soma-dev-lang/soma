//! Fault injection runs in child processes to isolate process-wide storage.
use super::*;
use crate::provider::HttpBackend;
use crate::runtime::storage::{take_write_error, FileBackend, MemoryBackend, SqliteBackend};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
fn isolated(name: &str, test: impl FnOnce()) {
    if std::env::var("SOMA_BACKEND_TEST").as_deref() == Ok(name) {
        let dir =
            std::env::temp_dir().join(format!("soma_backend_{}_{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        crate::runtime::storage::set_data_dir_beside(&dir.join("app.cell"));
        test();
        let _ = std::fs::remove_dir_all(dir);
    } else {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("interpreter::backend_tests::{name}"),
                "--nocapture",
            ])
            .env("SOMA_BACKEND_TEST", name)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() > deadline {
                let _ = child.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
fn file() -> (FileBackend, std::path::PathBuf) {
    let path = crate::runtime::storage::data_dir().join("A_data.json");
    (FileBackend::new("A", "data"), path)
}
fn block_target(path: &std::path::Path) {
    std::fs::remove_file(path).unwrap();
    std::fs::create_dir(path).unwrap();
}
#[test]
fn refused_file_set_preserves_the_previous_in_memory_value() {
    isolated(
        "refused_file_set_preserves_the_previous_in_memory_value",
        || {
            let (b, p) = file();
            b.set("x", StoredValue::Int(7));
            block_target(&p);
            b.set("x", StoredValue::Int(9));
            assert!(take_write_error().is_some());
            assert_eq!(b.get("x").unwrap().to_string(), "7");
        },
    );
}
#[test]
fn refused_file_delete_preserves_the_previous_in_memory_value() {
    isolated(
        "refused_file_delete_preserves_the_previous_in_memory_value",
        || {
            let (b, p) = file();
            b.set("x", StoredValue::Int(7));
            block_target(&p);
            assert!(!b.delete("x"));
            assert!(take_write_error().is_some());
            assert_eq!(b.get("x").unwrap().to_string(), "7");
        },
    );
}
#[test]
fn refused_file_append_preserves_the_previous_list() {
    isolated("refused_file_append_preserves_the_previous_list", || {
        let (b, p) = file();
        b.append(StoredValue::Int(7));
        block_target(&p);
        b.append(StoredValue::Int(9));
        assert!(take_write_error().is_some());
        assert_eq!(b.list().len(), 1);
    });
}
#[test]
fn refused_file_unappend_preserves_the_previous_list() {
    isolated("refused_file_unappend_preserves_the_previous_list", || {
        let (b, p) = file();
        b.append(StoredValue::Int(7));
        block_target(&p);
        b.unappend();
        assert!(take_write_error().is_some());
        assert_eq!(b.list().len(), 1);
    });
}
#[test]
fn refused_file_list_replacement_preserves_the_whole_list() {
    isolated(
        "refused_file_list_replacement_preserves_the_whole_list",
        || {
            let (b, p) = file();
            b.append(StoredValue::Int(7));
            b.append(StoredValue::Int(8));
            block_target(&p);
            b.replace_list(vec![StoredValue::Int(9)]);
            assert!(take_write_error().is_some());
            assert_eq!(
                b.list().iter().map(ToString::to_string).collect::<Vec<_>>(),
                ["7", "8"]
            );
        },
    );
}
#[test]
fn memory_list_replacement_handles_legacy_keyed_values_without_looping() {
    isolated(
        "memory_list_replacement_handles_legacy_keyed_values_without_looping",
        || {
            let b = MemoryBackend::new();
            b.set("0", StoredValue::Int(7));
            b.replace_list(vec![]);
            assert!(b.list().is_empty());
        },
    );
}
#[test]
fn file_list_replacement_handles_legacy_keyed_values_without_looping() {
    isolated(
        "file_list_replacement_handles_legacy_keyed_values_without_looping",
        || {
            let (b, _) = file();
            b.set("0", StoredValue::Int(7));
            b.replace_list(vec![]);
            assert!(b.list().is_empty());
            assert!(FileBackend::new("A", "data").list().is_empty());
        },
    );
}
#[test]
fn memory_append_preserves_legacy_keyed_values() {
    isolated("memory_append_preserves_legacy_keyed_values", || {
        let b = MemoryBackend::new();
        b.set("0", StoredValue::Int(7));
        b.append(StoredValue::Int(9));
        assert_eq!(
            b.list().iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["7", "9"]
        );
    });
}
#[test]
fn file_append_preserves_legacy_keyed_values() {
    isolated("file_append_preserves_legacy_keyed_values", || {
        let (b, _) = file();
        b.set("0", StoredValue::Int(7));
        b.append(StoredValue::Int(9));
        assert_eq!(
            b.list().iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["7", "9"]
        );
    });
}
fn sidecar(body: &str, status: u16) -> (HttpBackend, std::thread::JoinHandle<serde_json::Value>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let body = body.to_string();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            let mut b = [0];
            stream.read_exact(&mut b).unwrap();
            headers.push(b[0]);
        }
        let headers = String::from_utf8(headers).unwrap();
        let len = headers
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                (k.eq_ignore_ascii_case("content-length"))
                    .then(|| v.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        let mut request = vec![0; len];
        stream.read_exact(&mut request).unwrap();
        write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        serde_json::from_slice(&request).unwrap()
    });
    (HttpBackend::new(&url, "A", "data"), server)
}
#[test]
fn http_write_failure_is_an_error() {
    isolated("http_write_failure_is_an_error", || {
        let (b, s) = sidecar("{\"error\":\"unavailable\"}", 503);
        b.set("x", StoredValue::Int(1));
        s.join().unwrap();
        assert!(take_write_error().is_some());
    });
}
#[test]
fn http_read_failure_is_not_a_missing_key() {
    isolated("http_read_failure_is_not_a_missing_key", || {
        let (b, s) = sidecar("{\"error\":\"unavailable\"}", 503);
        assert!(b.get("x").is_none());
        s.join().unwrap();
        assert!(take_write_error().is_some());
    });
}
#[test]
fn http_false_acknowledgement_is_not_success() {
    isolated("http_false_acknowledgement_is_not_success", || {
        let (b, s) = sidecar("{\"ok\":false}", 200);
        b.set("x", StoredValue::Int(1));
        s.join().unwrap();
        assert!(take_write_error().is_some());
    });
}
#[test]
fn http_malformed_json_is_an_error() {
    isolated("http_malformed_json_is_an_error", || {
        let (b, s) = sidecar("not JSON", 200);
        b.get("x");
        s.join().unwrap();
        assert!(take_write_error().is_some());
    });
}
#[test]
fn http_typed_values_reject_invalid_payloads_instead_of_fabricating_values() {
    isolated(
        "http_typed_values_reject_invalid_payloads_instead_of_fabricating_values",
        || {
            for value in [
                r#"{"type":"int","value":"7"}"#,
                r#"{"type":"int"}"#,
                r#"{"type":"bigint","value":"oops"}"#,
                r#"{"type":"float","value":null}"#,
                r#"{"type":"bool","value":1}"#,
                r#"{"type":"string","value":7}"#,
                r#"{"type":"list","value":{}}"#,
                r#"{"type":"map","value":[]}"#,
                r#"{"type":"unknown","value":7}"#,
                r#"{"type":"variant","kind":"unknown"}"#,
                r#"{"type":"variant","type_name":"A","variant":"V","kind":"tuple","fields":[{"type":"int"}]}"#,
            ] {
                let (b, s) = sidecar(&format!("{{\"value\":{value}}}"), 200);
                b.get("x");
                s.join().unwrap();
                assert!(
                    take_write_error().is_some(),
                    "accepted malformed typed value: {value}"
                );
            }
        },
    );
}
#[test]
fn http_collections_reject_malformed_items_and_counts() {
    isolated("http_collections_reject_malformed_items_and_counts", || {
        let (b, s) = sidecar(
            r#"{"items":[{"type":"int","value":1},{"type":"int"}]}"#,
            200,
        );
        assert!(b.list().is_empty());
        s.join().unwrap();
        assert!(take_write_error().is_some());
        let (b, s) = sidecar(r#"{"keys":["x",7]}"#, 200);
        assert!(b.keys().is_empty());
        s.join().unwrap();
        assert!(take_write_error().is_some());
        let (b, s) = sidecar(r#"{"len":-1}"#, 200);
        b.len();
        s.join().unwrap();
        assert!(take_write_error().is_some());
    });
}
#[test]
fn http_nonfinite_floats_keep_their_values_on_the_wire() {
    isolated(
        "http_nonfinite_floats_keep_their_values_on_the_wire",
        || {
            for (n, tag) in [
                (f64::NAN, "NaN"),
                (f64::INFINITY, "inf"),
                (f64::NEG_INFINITY, "-inf"),
            ] {
                let (b, s) = sidecar(r#"{"ok":true}"#, 200);
                b.set("x", StoredValue::Float(n));
                let r = s.join().unwrap();
                assert_eq!(r["value"]["value"], tag);
                assert!(take_write_error().is_none());
                let (b, s) = sidecar(
                    &format!("{{\"value\":{{\"type\":\"float\",\"value\":\"{tag}\"}}}}"),
                    200,
                );
                let v = b.get("x").unwrap();
                s.join().unwrap();
                let StoredValue::Float(x) = v else {
                    panic!("not Float")
                };
                assert!(if n.is_nan() { x.is_nan() } else { x == n });
            }
        },
    );
}
#[test]
fn sql_read_failures_report_errors_without_panicking() {
    isolated("sql_read_failures_report_errors_without_panicking", || {
        let b = SqliteBackend::new("A", "data");
        crate::runtime::storage::shared_connection()
            .unwrap()
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE A_data; DROP TABLE A_data_log")
            .unwrap();
        for op in 0..7 {
            match op {
                0 => {
                    b.get("x");
                }
                1 => {
                    b.list();
                }
                2 => {
                    b.keys();
                }
                3 => {
                    b.values();
                }
                4 => {
                    b.has("x");
                }
                5 => {
                    b.len();
                }
                _ => {
                    b.list_get(0);
                }
            }
            assert!(take_write_error().is_some(), "silent read failure: {op}");
        }
    });
}

#[test]
fn http_absence_null_and_legacy_primitives_remain_distinct() {
    isolated(
        "http_absence_null_and_legacy_primitives_remain_distinct",
        || {
            let (b, s) = sidecar(r#"{"value":null}"#, 200);
            assert!(b.get("x").is_none());
            s.join().unwrap();
            assert!(take_write_error().is_none());
            let (b, s) = sidecar(r#"{"value":{"type":"null"}}"#, 200);
            assert!(matches!(b.get("x"), Some(StoredValue::Null)));
            s.join().unwrap();
            assert!(take_write_error().is_none());
            for (payload, expected) in [
                ("42", "42"),
                ("42.5", "42.5"),
                ("true", "true"),
                ("\"text\"", "text"),
                ("18446744073709551616", "18446744073709551616"),
            ] {
                let (b, s) = sidecar(&format!("{{\"value\":{payload}}}"), 200);
                assert_eq!(b.get("x").unwrap().to_string(), expected);
                s.join().unwrap();
                assert!(take_write_error().is_none());
            }
        },
    );
}

#[test]
fn http_nested_values_round_trip_with_their_types() {
    isolated("http_nested_values_round_trip_with_their_types", || {
        use crate::runtime::storage::StoredVariantFields;
        let value = StoredValue::List(vec![
            StoredValue::Map(indexmap::IndexMap::from([
                (
                    "__variant__".into(),
                    StoredValue::String("ordinary map".into()),
                ),
                (
                    "n".into(),
                    StoredValue::BigInt("9999999999999999999999999".into()),
                ),
            ])),
            StoredValue::Variant {
                type_name: "A".into(),
                variant: "Reading".into(),
                fields: StoredVariantFields::Struct(vec![(
                    "value".into(),
                    StoredValue::Float(f64::NAN),
                )]),
            },
        ]);
        let (b, s) = sidecar(r#"{"ok":true}"#, 200);
        b.set("x", value.clone());
        let request = s.join().unwrap();
        assert!(take_write_error().is_none());
        let (b, s) = sidecar(
            &serde_json::json!({"value":request["value"]}).to_string(),
            200,
        );
        assert_eq!(format!("{:?}", b.get("x").unwrap()), format!("{value:?}"));
        s.join().unwrap();
        assert!(take_write_error().is_none());
    });
}

struct FaultyBackend {
    deletes: std::sync::atomic::AtomicUsize,
    writes: std::sync::atomic::AtomicUsize,
    unreadable: bool,
}
impl StorageBackend for FaultyBackend {
    fn get(&self, _: &str) -> Option<StoredValue> {
        if self.unreadable {
            crate::runtime::storage::note_read_error("test read", &"unavailable");
        }
        None
    }
    fn set(&self, _: &str, _: StoredValue) {
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn delete(&self, _: &str) -> bool {
        self.deletes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        false
    }
    fn append(&self, _: StoredValue) {}
    fn unappend(&self) {
        assert!(
            self.deletes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                < 2,
            "replacement loops when the provider makes no progress"
        );
    }
    fn list(&self) -> Vec<StoredValue> {
        vec![StoredValue::Int(7)]
    }
    fn keys(&self) -> Vec<String> {
        vec![]
    }
    fn values(&self) -> Vec<StoredValue> {
        vec![]
    }
    fn has(&self, _: &str) -> bool {
        false
    }
    fn len(&self) -> usize {
        0
    }
    fn backend_name(&self) -> &str {
        "http"
    }
}
#[test]
fn default_list_replacement_stops_when_a_provider_makes_no_progress() {
    isolated(
        "default_list_replacement_stops_when_a_provider_makes_no_progress",
        || {
            let backend = FaultyBackend {
                deletes: 0.into(),
                writes: 0.into(),
                unreadable: false,
            };
            backend.replace_list(vec![]);
            assert!(take_write_error().is_some());
            assert_eq!(
                backend.deletes.load(std::sync::atomic::Ordering::Relaxed),
                1
            );
        },
    );
}
#[test]
fn an_unreadable_previous_value_never_becomes_a_compensating_delete() {
    isolated(
        "an_unreadable_previous_value_never_becomes_a_compensating_delete",
        || {
            let tokens = crate::lexer::Lexer::new(
                "cell A { memory { data: Map<String,Int> } on put() { data.set(\"x\",7) } }",
            )
            .tokenize()
            .unwrap();
            let program = crate::parser::Parser::new(tokens).parse_program().unwrap();
            let mut interp = Interpreter::new(&program);
            let backend = Arc::new(FaultyBackend {
                deletes: 0.into(),
                writes: 0.into(),
                unreadable: true,
            });
            interp.set_storage(
                "A",
                &HashMap::from([("data".into(), backend.clone() as Arc<dyn StorageBackend>)]),
            );
            assert!(interp.call_signal("A", "put", vec![]).is_err());
            assert_eq!(backend.writes.load(std::sync::atomic::Ordering::Relaxed), 0);
            assert_eq!(
                backend.deletes.load(std::sync::atomic::Ordering::Relaxed),
                0
            );
        },
    );
}
