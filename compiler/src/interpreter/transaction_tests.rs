//! Isolated processes keep SQLite fault injection out of unrelated unit tests.
use super::*;
use crate::runtime::storage::{MemoryBackend, SqliteBackend};
use std::process::Command;

fn isolated(name: &str, test: impl FnOnce()) {
    if std::env::var("SOMA_TRANSACTION_TEST").as_deref() == Ok(name) {
        let dir =
            std::env::temp_dir().join(format!("soma_transaction_{}_{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        crate::runtime::storage::set_data_dir_beside(&dir.join("app.cell"));
        test();
        let _ = std::fs::remove_dir_all(dir);
    } else {
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("interpreter::transaction_tests::{name}"),
                "--nocapture",
            ])
            .env("SOMA_TRANSACTION_TEST", name)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
fn fixture() -> (Interpreter, Arc<MemoryBackend>, Arc<SqliteBackend>) {
    let source = r#"cell A {
 memory { temp: Map<String,Int> persist: Map<String,Int> [persistent] }
 on write() { temp.set("x",1) persist.set("x",1) publish("news",1) return 7 }
 on only_memory() { temp.set("x",1) return 7 }
}"#;
    let tokens = crate::lexer::Lexer::new(source).tokenize().unwrap();
    let program = crate::parser::Parser::new(tokens).parse_program().unwrap();
    let mut interp = Interpreter::new(&program);
    let memory = Arc::new(MemoryBackend::new());
    let sqlite = Arc::new(SqliteBackend::new("A", "persist"));
    let slots: HashMap<String, Arc<dyn StorageBackend>> = HashMap::from([
        ("temp".into(), memory.clone() as Arc<dyn StorageBackend>),
        ("persist".into(), sqlite.clone() as Arc<dyn StorageBackend>),
    ]);
    interp.set_storage("A", &slots);
    interp.atomically(|_| Ok::<_, RuntimeError>(())).unwrap();
    (interp, memory, sqlite)
}
fn sql(sql: &str) {
    crate::runtime::storage::shared_connection()
        .unwrap()
        .lock()
        .unwrap()
        .execute_batch(sql)
        .unwrap();
}
fn deferred_failure() {
    sql("PRAGMA foreign_keys=ON;
         CREATE TABLE parent(id INTEGER PRIMARY KEY);
         CREATE TABLE child(id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED);
         CREATE TRIGGER deferred_failure AFTER INSERT ON A_persist BEGIN INSERT INTO child VALUES (99); END;");
}
#[test]
fn failed_begin_does_not_run_a_handler_outside_a_transaction() {
    isolated(
        "failed_begin_does_not_run_a_handler_outside_a_transaction",
        || {
            let (mut interp, memory, _) = fixture();
            sql("PRAGMA query_only=ON");
            let result = interp.call_signal("A", "only_memory", vec![]);
            assert!(
                result.is_err(),
                "a refused BEGIN still ran the handler: {result:?}"
            );
            assert!(memory.get("x").is_none());
        },
    );
}
#[test]
fn failed_commit_rolls_back_memory_and_withholds_events() {
    isolated(
        "failed_commit_rolls_back_memory_and_withholds_events",
        || {
            let (mut interp, memory, sqlite) = fixture();
            deferred_failure();
            let (tx, rx) = std::sync::mpsc::sync_channel(4);
            let bus = new_event_bus();
            bus.lock().unwrap().push(tx);
            interp.event_bus = Some(bus);
            let result = interp.call_signal("A", "write", vec![]);
            assert!(
                result.is_err(),
                "a refused COMMIT was reported as success: {result:?}"
            );
            assert!(memory.get("x").is_none() && sqlite.get("x").is_none());
            assert!(rx.try_recv().is_err(), "uncommitted event escaped");
            assert_eq!(interp.last_commit_writes, 0);
            assert!(crate::runtime::storage::shared_connection()
                .unwrap()
                .lock()
                .unwrap()
                .is_autocommit());
            sql("DROP TRIGGER deferred_failure");
            assert!(interp.call_signal("A", "write", vec![]).is_ok());
        },
    );
}
#[test]
fn failed_task_commit_does_not_execute_the_external_step() {
    isolated(
        "failed_task_commit_does_not_execute_the_external_step",
        || {
            let (mut interp, memory, sqlite) = fixture();
            deferred_failure();
            let called = std::cell::Cell::new(false);
            let result = interp.run_task(|me| -> Result<(), RuntimeError> {
                me.call_signal_inner("A", "write", vec![])?;
                let _ = me.outside_unit(|| called.set(true));
                Ok(())
            });
            assert!(result.is_err(), "failed task step was accepted");
            assert!(
                !called.get(),
                "external work ran before a successful commit"
            );
            assert!(memory.get("x").is_none() && sqlite.get("x").is_none());
        },
    );
}
#[test]
fn pending_backend_errors_cannot_commit_an_atomic_unit() {
    isolated(
        "pending_backend_errors_cannot_commit_an_atomic_unit",
        || {
            let (mut interp, memory, sqlite) = fixture();
            let result = interp.atomically(|me| -> Result<(), RuntimeError> {
                me.call_signal_inner("A", "write", vec![])?;
                crate::runtime::storage::note_write_error("injected backend", &"refused");
                Ok(())
            });
            assert!(result.is_err(), "pending storage error was ignored");
            assert!(memory.get("x").is_none() && sqlite.get("x").is_none());
        },
    );
}
#[test]
fn returning_from_a_tick_commits_its_successful_writes() {
    isolated(
        "returning_from_a_tick_commits_its_successful_writes",
        || {
            let (mut interp, memory, sqlite) = fixture();
            let body = interp.handler_cache[&("A".to_string(), "write".to_string())]
                .1
                .clone();
            let result = interp
                .exec_tick(&body, &mut Env::default(), "A", false)
                .unwrap();
            assert_eq!(result.as_int().unwrap(), 7);
            assert!(
                memory.get("x").is_some() && sqlite.get("x").is_some(),
                "return rolled back a successful tick"
            );
        },
    );
}

#[test]
fn the_first_auxiliary_write_joins_a_real_transaction() {
    isolated("the_first_auxiliary_write_joins_a_real_transaction", || {
        PERSIST_MACHINES.store(true, std::sync::atomic::Ordering::Relaxed);
        let tokens =
            crate::lexer::Lexer::new("cell A { on save() { remember(\"x\",1) return 1 } }")
                .tokenize()
                .unwrap();
        let program = crate::parser::Parser::new(tokens).parse_program().unwrap();
        let mut interp = Interpreter::new(&program);
        let r = interp.atomically(|me| -> Result<(), RuntimeError> {
            let conn = crate::runtime::storage::shared_connection()
                .expect("auxiliary storage opened after BEGIN");
            assert!(!conn.lock().unwrap().is_autocommit());
            me.call_signal_inner("A", "save", vec![])?;
            Err(RuntimeError::TypeError("abort".into()))
        });
        assert!(r.is_err());
        assert!(builtins::storage::agent_memory(&mut interp, "A")
            .get("x")
            .is_none());
        assert!(
            interp.call_signal("A", "save", vec![]).is_ok(),
            "rollback removed a cached auxiliary table"
        );
    });
}

#[test]
fn a_failed_task_restart_preserves_only_the_committed_step() {
    isolated(
        "a_failed_task_restart_preserves_only_the_committed_step",
        || {
            let (mut interp, memory, sqlite) = fixture();
            let r = interp.run_task(|me| -> Result<(), RuntimeError> {
                me.call_signal_inner("A", "write", vec![])?;
                let _ = me.outside_unit(|| sql("PRAGMA query_only=ON"));
                assert!(me.check_transaction().is_err());
                Ok(())
            });
            assert!(r.is_err());
            assert!(memory.get("x").is_some() && sqlite.get("x").is_some());
            sql("PRAGMA query_only=OFF");
            assert!(interp.call_signal("A", "write", vec![]).is_ok());
        },
    );
}

#[test]
fn raw_backend_writes_cannot_escape_an_implicitly_aborted_unit() {
    isolated(
        "raw_backend_writes_cannot_escape_an_implicitly_aborted_unit",
        || {
            let (mut interp, _, sqlite) = fixture();
            sql("CREATE TRIGGER abort_unit BEFORE INSERT ON A_persist WHEN NEW.key='x' BEGIN SELECT RAISE(ROLLBACK,'abort'); END");
            let r = interp.atomically(|_| -> Result<(), RuntimeError> {
                sqlite.set("x", StoredValue::Int(1));
                sqlite.set("escaped", StoredValue::Int(9));
                Ok(())
            });
            assert!(r.is_err());
            assert!(
                sqlite.get("escaped").is_none(),
                "a backend write escaped into autocommit mode"
            );
        },
    );
}

#[test]
fn indexed_list_cache_observes_external_commits() {
    isolated("indexed_list_cache_observes_external_commits", || {
        let (_, _, sqlite) = fixture();
        for n in [10, 20, 30, 40] {
            sqlite.append(StoredValue::Int(n));
        }
        assert_eq!(sqlite.list_get(2).unwrap().to_string(), "30");
        let other = rusqlite::Connection::open(crate::runtime::storage::data_dir().join("soma.db"))
            .unwrap();
        other
            .execute("DELETE FROM A_persist_log WHERE id=2", [])
            .unwrap();
        assert_eq!(sqlite.list_get(2).unwrap().to_string(), "40");
    });
}

#[test]
fn indexed_list_cache_discards_a_rolled_back_layout() {
    isolated("indexed_list_cache_discards_a_rolled_back_layout", || {
        let (mut interp, _, sqlite) = fixture();
        for n in [10, 20, 30] {
            sqlite.append(StoredValue::Int(n));
        }
        sql("DELETE FROM A_persist_log WHERE id=2");
        let r = interp.atomically(|_| -> Result<(), RuntimeError> {
            sql("DELETE FROM A_persist_log WHERE id=1");
            assert_eq!(sqlite.list_get(0).unwrap().to_string(), "30");
            Err(RuntimeError::TypeError("abort".into()))
        });
        assert!(r.is_err());
        interp
            .atomically(|_| -> Result<(), RuntimeError> {
                assert_eq!(sqlite.list_get(0).unwrap().to_string(), "10");
                assert_eq!(sqlite.list_get(1).unwrap().to_string(), "30");
                Ok(())
            })
            .unwrap();
    });
}
