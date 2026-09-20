//! Keep the provider demo compatible with the typed HTTP adapter protocol.
use std::process::Command;

#[test]
fn python_sidecar_obeys_the_storage_protocol() {
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/sidecar/test_server.py");
    let output = Command::new("python3").arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn provider_demo_preserves_values_and_isolates_cells_and_slots() {
    let dir = std::env::temp_dir().join(format!("soma_provider_demo_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let demo = include_str!("../../tools/mock_provider.cell");
    let checks = r#"
cell Client {
 on rpc(path: String, cell_name: String, field: String, key: String, value: Any) {
  return request("POST",path,to_json(map("cell",cell_name,"field",field,"key",key,"value",value)))
 }
}
cell test Protocol {
 rules {
  let typed = map("type","map","value",map("nested",map("type","int","value",42)))
  assert rpc("/set","A:B","C","é",typed).ok == true
  assert rpc("/get","A:B","C","é",()).value == typed
  assert rpc("/get","A","B:C","é",()).value == ()
  assert rpc("/get","A:B","D","é",()).value == ()
  assert rpc("/set","A:B","C","__meta",typed).ok == true
  assert rpc("/keys","A:B","C","",()).keys == ["é"]
  assert rpc("/values","A:B","C","",()).values == [typed]
  assert rpc("/len","A:B","C","",()).len == 1
  assert rpc("/has","A:B","C","é",()).exists == true
  assert rpc("/delete","A:B","C","é",()).deleted == true
  assert rpc("/delete","A:B","C","é",()).deleted == false
  assert rpc("/has","A:B","C","é",()).exists == false
  assert rpc("/append","A","rows","",typed).ok == true
  assert rpc("/append","A","rows","",map("type","null")).ok == true
  assert rpc("/list","A","rows","",()).items == [typed,map("type","null")]
  assert rpc("/list","A","other","",()).items == []
  assert rpc("/len","A","rows","",()).len == 2
  assert rpc("/unappend","A","rows","",()).ok == true
  assert rpc("/list","A","rows","",()).items == [typed]
  assert rpc("/unappend","A","rows","",()).ok == true
  assert rpc("/unappend","A","rows","",()).ok == true
  assert rpc("/list","A","rows","",()).items == []
 }
}
"#;
    std::fs::write(dir.join("app.cell"), format!("{demo}\n{checks}")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["test", "app.cell"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(dir);
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
