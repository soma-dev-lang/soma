//! Real SQLite refusals must surface as errors without committing partial state.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SOURCE: &str = r#"
cell A {
 memory { data: Map<String,Int> [persistent] rows: List<Int> [persistent] }
 state life { initial: idle idle -> done }
 on seed() { data.set("baseline",7) rows.push(10) rows.push(20) rows.push(30) rows.push(40) remember("seed",1) let id=next_id() transition("seed","done") return id }
 on mem() { data.set("temp",1) remember("blocked",4) return "success" }
 on counter() { data.set("temp",1) return next_id() }
 on move() { data.set("temp",1) return transition("new","done") }
 on edit() { rows[0]=99 return rows.all }
 on remove() { rows.delete(0) return rows.all }
 on caught() { data.set("outside",1) let r=try { data.set("inside",2) remember("blocked",3) } data.set("after",4) return r.kind }
 on failed_push() { let r=try { rows.push(99) } return r.kind }
 on sparse() { let r=try { rows.push(50) fail("stop","undo") } rows.push(60) rows.push(70) return [rows[4],rows[5]] }
 on rollback_list() { rows.push(50) data.set("temp",1) return "success" }
 on caught_rollback() { let r=try { rows.push(50) data.set("temp",1) } data.set("after",4) return "success" }
 on read() { return map("data",data.entries(),"rows",rows.all,"index",[rows[0],rows[1],rows[2]],"stored",recall("blocked"),"state",get_status("new")) }
}
"#;
struct Fixture {
    dir: PathBuf,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("soma_storage_{}_{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.cell"), SOURCE).unwrap();
        let fixture = Self { dir };
        assert!(fixture.call("seed").status.success());
        fixture
    }
    fn call(&self, handler: &str) -> Output {
        invoke(&self.dir, handler)
    }
    fn db(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.dir.join(".soma_data/soma.db")).unwrap()
    }
    fn sql(&self, sql: &str) {
        self.db().execute_batch(sql).unwrap();
    }
    fn reject(&self, table: &str, operation: &str) {
        self.sql(&format!("CREATE TRIGGER reject_write BEFORE {operation} ON \"{table}\" BEGIN SELECT RAISE(ABORT,'injected write refusal'); END"));
    }
    fn value(&self, table: &str, key: &str) -> Option<String> {
        self.db()
            .query_row(
                &format!("SELECT value FROM \"{table}\" WHERE key=?1"),
                [key],
                |r| r.get(0),
            )
            .ok()
    }
    fn rows(&self) -> Vec<String> {
        let db = self.db();
        let mut stmt = db
            .prepare("SELECT value FROM A_rows_log ORDER BY id")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
    fn refused(&self, handler: &str, kind: &str) {
        let out = self.call(handler);
        let text = output(&out);
        assert!(!out.status.success() && text.contains(kind), "{text}");
        assert_eq!(self.value("A_data", "temp"), None);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn invoke(dir: &Path, handler: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["run", "app.cell", handler])
        .current_dir(dir)
        .output()
        .unwrap()
}
fn output(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn refused_remember_rolls_back_the_handler() {
    let f = Fixture::new("remember");
    f.reject("A__agent_memory", "INSERT");
    f.refused("mem", "storage");
    assert_eq!(f.value("A__agent_memory", "blocked"), None);
}
#[test]
fn refused_counter_write_does_not_issue_an_id_or_commit_other_writes() {
    let f = Fixture::new("counter");
    f.reject("A__counters", "INSERT");
    f.refused("counter", "storage");
    assert_eq!(f.value("A__counters", "next_id"), Some("1".into()));
}
#[test]
fn refused_transition_does_not_report_a_successful_move() {
    let f = Fixture::new("transition");
    f.reject("A__sm_life", "INSERT");
    f.refused("move", "storage");
    assert_eq!(f.value("A__sm_life", "new"), None);
}
#[test]
fn caught_remember_failure_rolls_back_its_savepoint_and_allows_later_writes() {
    let f = Fixture::new("catch");
    f.reject("A__agent_memory", "INSERT");
    let out = f.call("caught");
    assert!(
        out.status.success() && output(&out).trim() == "storage",
        "{}",
        output(&out)
    );
    assert_eq!(f.value("A_data", "outside"), Some("1".into()));
    assert_eq!(f.value("A_data", "inside"), None);
    assert_eq!(f.value("A_data", "after"), Some("4".into()));
}
#[test]
fn list_update_stops_when_the_row_update_is_refused() {
    // `rows[0] = 99` updates one row (it used to delete and re-insert the
    // whole log): a refused UPDATE is a storage error and the log is intact
    let f = Fixture::new("replace_delete");
    f.reject("A_rows_log", "UPDATE");
    f.refused("edit", "storage");
    assert_eq!(f.rows(), ["10", "20", "30", "40"]);
}
#[test]
fn list_update_touches_no_other_row() {
    // rejecting DELETE and INSERT on the log leaves the one-row update free
    let f = Fixture::new("replace_untouched");
    f.reject("A_rows_log", "DELETE");
    f.sql("CREATE TRIGGER reject_insert BEFORE INSERT ON \"A_rows_log\" BEGIN SELECT RAISE(ABORT,'injected write refusal'); END");
    let out = f.call("edit");
    assert!(out.status.success(), "{}", output(&out));
    assert_eq!(f.rows(), ["99", "20", "30", "40"]);
}
#[test]
fn list_remove_stops_when_deleting_the_old_log_is_refused() {
    let f = Fixture::new("remove_delete");
    f.reject("A_rows_log", "DELETE");
    f.refused("remove", "storage");
    assert_eq!(f.rows(), ["10", "20", "30", "40"]);
}
#[test]
fn list_update_refuses_the_new_value_and_keeps_the_original_log() {
    let f = Fixture::new("replace_insert");
    f.sql("CREATE TRIGGER reject_write BEFORE UPDATE ON A_rows_log WHEN NEW.value='99' BEGIN SELECT RAISE(ABORT,'injected write refusal'); END");
    f.refused("edit", "storage");
    assert_eq!(f.rows(), ["10", "20", "30", "40"]);
}
#[test]
fn indexed_reads_of_a_legacy_log_with_gaps_keep_list_order() {
    let f = Fixture::new("gaps");
    f.sql("DELETE FROM A_rows_log WHERE id=2");
    let out = f.call("read");
    assert!(out.status.success(), "{}", output(&out));
    assert!(
        output(&out).contains("\"index\": [10, 30, 40]"),
        "{}",
        output(&out)
    );
}
#[test]
fn next_id_refuses_i64_exhaustion_without_wrapping() {
    let f = Fixture::new("max_id");
    f.sql("UPDATE A__counters SET value='9223372036854775807' WHERE key='next_id'");
    f.refused("counter", "range");
    assert_eq!(
        f.value("A__counters", "next_id"),
        Some("9223372036854775807".into())
    );
}
#[test]
fn next_id_refuses_corrupt_counter_values_instead_of_restarting_at_one() {
    let f = Fixture::new("corrupt_id");
    f.sql("UPDATE A__counters SET value='not-a-number' WHERE key='next_id'");
    f.refused("counter", "storage");
    assert_eq!(
        f.value("A__counters", "next_id"),
        Some("not-a-number".into())
    );
}
#[test]
fn next_id_refuses_negative_counter_values() {
    let f = Fixture::new("negative_id");
    f.sql("UPDATE A__counters SET value='-7' WHERE key='next_id'");
    f.refused("counter", "storage");
    assert_eq!(f.value("A__counters", "next_id"), Some("-7".into()));
}

#[test]
fn a_refused_append_inside_try_preserves_the_previous_last_item() {
    let f = Fixture::new("failed_push");
    f.reject("A_rows_log", "INSERT");
    let out = f.call("failed_push");
    assert!(
        out.status.success() && output(&out).trim() == "storage",
        "{}",
        output(&out)
    );
    assert_eq!(f.rows(), ["10", "20", "30", "40"]);
}
#[test]
fn indexed_reads_after_an_append_is_undone_keep_list_order() {
    let f = Fixture::new("sparse");
    let out = f.call("sparse");
    assert!(
        out.status.success() && output(&out).trim() == "[60, 70]",
        "{}",
        output(&out)
    );
}
#[test]
fn a_database_rollback_does_not_delete_previously_committed_list_items() {
    let f = Fixture::new("rollback_list");
    f.sql("CREATE TRIGGER reject_write BEFORE INSERT ON A_data WHEN NEW.key='temp' BEGIN SELECT RAISE(ROLLBACK,'injected transaction rollback'); END");
    f.refused("rollback_list", "storage");
    assert_eq!(f.rows(), ["10", "20", "30", "40"]);
}
#[test]
fn try_cannot_continue_writing_after_sqlite_ends_the_transaction() {
    let f = Fixture::new("caught_rollback");
    f.sql("CREATE TRIGGER reject_write BEFORE INSERT ON A_data WHEN NEW.key='temp' BEGIN SELECT RAISE(ROLLBACK,'injected transaction rollback'); END");
    f.refused("caught_rollback", "storage");
    assert_eq!(f.rows(), ["10", "20", "30", "40"]);
    assert_eq!(f.value("A_data", "after"), None);
}

#[test]
fn a_caught_sqlite_fail_rolls_back_all_changes_of_the_failed_statement() {
    let f = Fixture::new("fail_policy");
    f.sql("CREATE TRIGGER reject_write AFTER INSERT ON A__agent_memory WHEN NEW.key='blocked' BEGIN INSERT INTO A_data VALUES ('triggered','9','int'); SELECT RAISE(FAIL,'injected partial statement'); END");
    let out = f.call("caught");
    assert!(
        out.status.success() && output(&out).trim() == "storage",
        "{}",
        output(&out)
    );
    assert_eq!(f.value("A__agent_memory", "blocked"), None);
    assert_eq!(f.value("A_data", "triggered"), None);
    assert_eq!(f.value("A_data", "inside"), None);
    assert_eq!(f.value("A_data", "after"), Some("4".into()));
}

#[test]
fn next_id_does_not_migrate_another_cells_legacy_counter() {
    let f = Fixture::new("foreign_legacy_counter");
    std::fs::write(
        f.dir.join("app.cell"),
        format!("{SOURCE}\ncell B {{ on fresh() {{ return next_id() }} }}"),
    )
    .unwrap();
    f.sql("INSERT INTO A__sm_life VALUES ('__next_id','77','int')");
    let out = f.call("fresh");
    assert!(
        out.status.success() && output(&out).trim() == "1",
        "{}",
        output(&out)
    );
}
#[test]
fn next_id_keeps_its_own_legacy_counter_during_migration() {
    let f = Fixture::new("own_legacy_counter");
    f.sql("DELETE FROM A__counters; INSERT INTO A_data VALUES ('__next_id','41','int')");
    let out = f.call("counter");
    assert!(
        out.status.success() && output(&out).trim() == "42",
        "{}",
        output(&out)
    );
}

#[test]
fn concurrent_processes_can_initialize_the_same_database() {
    for batch in 0..8 {
        let dir = std::env::temp_dir().join(format!(
            "soma_storage_startup_{}_{}",
            std::process::id(),
            batch
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(24));
        let jobs: Vec<_> = (0..24)
            .map(|i| {
                let app = dir.join(format!("app{i}.cell"));
                std::fs::write(
                    &app,
                    format!("cell A{i} {{ on run() {{ remember(\"x\",1) return next_id() }} }}"),
                )
                .unwrap();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    Command::new(env!("CARGO_BIN_EXE_soma"))
                        .args(["run", app.to_str().unwrap()])
                        .output()
                        .unwrap()
                })
            })
            .collect();
        let outputs: Vec<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
        let _ = std::fs::remove_dir_all(dir);
        for out in outputs {
            assert!(
                out.status.success() && output(&out).trim() == "1",
                "{}",
                output(&out)
            );
        }
    }
}
