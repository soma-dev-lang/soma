//! Storage failures must remain distinguishable from absent or empty values.
use std::path::PathBuf;
use std::process::{Command, Output};
const SOURCE: &str = r#"cell A {
 memory { data: Map<String,Any> [persistent] rows: List<Any> [persistent] }
 on seed() { data.set("broken",7) rows.push(10) rows.push(20) return 1 }
 on get() { data.set("marker",1) return data.get("broken") }
 on all() { data.set("marker",1) return rows.all }
 on index() { data.set("marker",1) return rows[0] }
 on bare() { data.set("marker",1) return rows }
 on values() { data.set("marker",1) return data.values() }
 on entries() { data.set("marker",1) return data.entries() }
 on remove() { rows.delete(0) return rows.all }
 on replace() { rows[0]=99 return rows.all }
 on append() { rows.push(30) return rows.all }
 on read() { return rows.all }
}"#;
struct Fixture {
    dir: PathBuf,
}
impl Fixture {
    fn new(name: &str, source: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "soma_read_boundary_{}_{}",
            std::process::id(),
            name
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.cell"), source).unwrap();
        Self { dir }
    }
    fn seeded(name: &str) -> Self {
        let f = Self::new(name, SOURCE);
        assert!(f.run("seed").status.success());
        f
    }
    fn run(&self, handler: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["run", "app.cell", handler])
            .current_dir(&self.dir)
            .output()
            .unwrap()
    }
    fn sql(&self, s: &str) {
        rusqlite::Connection::open(self.dir.join(".soma_data/soma.db"))
            .unwrap()
            .execute_batch(s)
            .unwrap();
    }
    fn refused(&self, handler: &str) {
        let o = self.run(handler);
        assert!(
            !o.status.success() && text(&o).contains("storage"),
            "{}",
            text(&o)
        );
        let db = rusqlite::Connection::open(self.dir.join(".soma_data/soma.db")).unwrap();
        let n: i64 = db
            .query_row("SELECT count(*) FROM A_data WHERE key='marker'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 0, "read failure committed earlier writes");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}
macro_rules! corrupt_read {
    ($name:ident,$table:expr,$handler:expr) => {
        #[test]
        fn $name() {
            let f = Fixture::seeded(stringify!($name));
            f.sql(&format!("UPDATE {} SET value=X'ff'", $table));
            f.refused($handler);
        }
    };
}
corrupt_read!(
    sqlite_get_does_not_turn_an_unreadable_row_into_null,
    "A_data",
    "get"
);
corrupt_read!(
    sqlite_list_does_not_discard_unreadable_rows,
    "A_rows_log",
    "all"
);
corrupt_read!(
    sqlite_index_does_not_turn_an_unreadable_row_into_null,
    "A_rows_log",
    "index"
);
corrupt_read!(
    bare_list_reads_propagate_storage_errors,
    "A_rows_log",
    "bare"
);
corrupt_read!(
    sqlite_values_does_not_discard_unreadable_rows,
    "A_data",
    "values"
);
corrupt_read!(
    sqlite_entries_does_not_discard_unreadable_rows,
    "A_data",
    "entries"
);
fn legacy(name: &str) -> Fixture {
    let f = Fixture::seeded(name);
    f.sql("DELETE FROM A_rows_log; INSERT INTO A_rows VALUES('0','10','int')");
    f
}
#[test]
fn removing_the_last_legacy_list_item_does_not_resurrect_it() {
    let f = legacy("legacy_remove");
    let o = f.run("remove");
    assert!(
        o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "[]",
        "{}",
        text(&o)
    );
    let o = f.run("read");
    assert!(
        o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "[]",
        "{}",
        text(&o)
    );
}
#[test]
fn appending_to_a_legacy_list_preserves_its_existing_values() {
    let f = legacy("legacy_append");
    f.sql("INSERT INTO A_rows VALUES('1','20','int')");
    let o = f.run("append");
    assert!(
        o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "[10, 20, 30]",
        "{}",
        text(&o)
    );
}
#[test]
fn replacing_then_deleting_a_legacy_list_never_reveals_old_values() {
    let f = legacy("legacy_replace");
    assert!(f.run("replace").status.success());
    let o = f.run("remove");
    assert!(
        o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "[]",
        "{}",
        text(&o)
    );
}
const FILE_SOURCE: &str = r#"cell backend JsonStore {rules {matches [persistent,local] native "file"}}
cell A {memory {data: Map<String,Int> [persistent,local]}
 on write(){data.set("balance",99) return data.get("balance")}
 on read(){return data.get("balance")}
}"#;
fn refused_file(name: &str, contents: &str) {
    let f = Fixture::new(name, FILE_SOURCE);
    std::fs::create_dir_all(f.dir.join(".soma_data")).unwrap();
    let p = f.dir.join(".soma_data/A_data.json");
    std::fs::write(&p, contents).unwrap();
    let o = f.run("write");
    assert!(
        !o.status.success() && text(&o).contains("storage"),
        "{}",
        text(&o)
    );
    assert_eq!(
        std::fs::read_to_string(p).unwrap(),
        contents,
        "corrupt data was overwritten"
    );
}
#[test]
fn malformed_json_storage_is_not_replaced_by_an_empty_store() {
    refused_file("broken_json", "{\"map\":{\"balance\":7");
}
#[test]
fn invalid_file_map_shape_is_not_silently_discarded() {
    refused_file("bad_map", "{\"map\":[7],\"log\":[]}");
}
#[test]
fn invalid_file_log_shape_is_not_silently_discarded() {
    refused_file("bad_log", "{\"map\":{},\"log\":{}}");
}
#[test]
fn file_read_errors_are_not_treated_as_a_missing_file() {
    let f = Fixture::new("directory", FILE_SOURCE);
    std::fs::create_dir_all(f.dir.join(".soma_data/A_data.json")).unwrap();
    let o = f.run("write");
    assert!(
        !o.status.success() && text(&o).contains("storage"),
        "{}",
        text(&o)
    );
}
