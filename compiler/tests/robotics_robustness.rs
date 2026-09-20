//! 100 adversarial scenarios for robotics-oriented use of Soma.
//! These test language/runtime behavior, not flight hardware or real-time deadlines.
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("soma_robotics_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn run(dir: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(args)
        .current_dir(dir)
        .env_remove("SOMA_SEEDS")
        .env_remove("SOMA_NODE_ID")
        .env_remove("SOMA_MOCK_TIME")
        .output()
        .expect("launch Soma");
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}
fn scenario(name: &str, source: &str, mode: &str, needle: &str) {
    let dir = scratch(name);
    std::fs::write(dir.join("app.cell"), source).unwrap();
    let args: &[&str] = match mode {
        "verify_ok" | "verify_fail" => &["verify", "app.cell", "--strict"],
        "check_fail" => &["check", "app.cell"],
        _ => &["test", "app.cell"],
    };
    let (code, out) = run(&dir, args);
    if mode.ends_with("_fail") {
        assert_eq!(code, 1, "{name}: expected refusal, got {code}:\n{out}");
    } else {
        assert_eq!(code, 0, "{name}:\n{out}");
    }
    assert!(out.contains(needle), "{name}: missing {needle:?}:\n{out}");
    let _ = std::fs::remove_dir_all(dir);
}
macro_rules! rover_case {
    ($name:ident,$rules:expr) => {
        #[test]
        fn $name() {
            scenario(
                stringify!($name),
                &format!("{}\ncell test T {{ rules {{ {} }} }}", ROVER, $rules),
                "runtime",
                "",
            );
        }
    };
}
macro_rules! source_case {
    ($name:ident,$mode:expr,$needle:expr,$source:expr) => {
        #[test]
        fn $name() {
            scenario(stringify!($name), $source, $mode, $needle);
        }
    };
}

const ROVER: &str = r###"
cell Rover {
 memory {
  battery: Map<String, Int>
  invariant battery >= 0 && battery <= 100
  motors: Map<String, Float>
  invariant motors >= 0.0 && motors <= 5.0
  samples: Map<String, Int>
  audit: Map<String, String> [immutable]
  scratch: Map<String, Any>
  queue: List<Int>
  invariant queue.size <= 8
 }
 state mission {
  initial: docked
  docked -> armed
  armed -> driving
  driving -> sampling
  sampling -> driving
  driving -> docked
  docked -> fault
  armed -> fault
  driving -> fault
  sampling -> fault
 }
 on phase(id: String) { return get_status(id) }
 on arm(id: String, power: Int) {
  require power >= 20 else LowPower
  battery.set(id,power)
  transition(id,"armed")
  return get_status(id)
 }
 on drive(id: String, speed: Float) {
  motors.set(id,speed)
  transition(id,"driving")
  return get_status(id)
 }
 on sample(id: String, n: Int) {
  transition(id,"sampling")
  samples.set(id,n)
  return get_status(id)
 }
 on resume(id: String) { transition(id,"driving") return get_status(id) }
 on park(id: String) { motors.set(id,0.0) transition(id,"docked") return get_status(id) }
 on abort(id: String) { motors.set(id,0.0) transition(id,"fault") return get_status(id) }
 on bad_sample(id: String) {
  transition(id,"sampling")
  samples.set(id,123)
  battery.set(id,-1)
 }
 on refused_batch() {
  scratch.set("a",1)
  scratch.set("b",2)
  queue.push(1)
  fail("hardware_timeout","actuator acknowledgement missing")
 }
 on changed_then_deleted() { scratch.set("a",123) scratch.delete("a") fail("refused","rollback") }
 on deleted_then_failed() { scratch.delete("a") fail("refused","rollback") }
 on overwritten_then_failed() { scratch.set("a",123) scratch.set("a",456) fail("refused","rollback") }
 on atomic_audit() { audit.set("a","tentative") fail("refused","rollback") }
 on bounded_sensor(x: Float) { require x >= -273.15 && x <= 125.0 else BadSensor return x }
 on stop_early() { for i in range(0,1000000000) { if i==3 { return i } } return -1 }
 on fuse(xs: List) { require len(xs)>0 else MissingSensors return avg(xs) }
 on fill_queue() { for i in range(0,8) { queue.push(i) } return queue.len() }
 on copy_positions() { let a=map("position",map("x",1)) let b=a b.position.x=2 return map("before",a.position.x,"after",b.position.x) }
 on short_circuit() { return false && (1/0 > 0) }
 on short_or() { return true || (1/0 > 0) }
}
"###;

rover_case!(
    r001_encoder_counts_above_float_precision,
    r###"assert 9007199254740993 > 9007199254740992.0
assert 9007199254740993 != 9007199254740992.0"###
);

rover_case!(
    r002_odometer_integer_promotion,
    r###"assert 9223372036854775807 + 1 == 9223372036854775808
assert type_of(9223372036854775807 + 1) == "Int""###
);

rover_case!(
    r003_negative_odometer_promotion,
    r###"assert -9223372036854775808 - 1 == -9223372036854775809"###
);

rover_case!(
    r004_wide_distance_products,
    r###"let d=ipow(10,40)
assert d*d == ipow(10,80)
assert idiv(d*d,d)==d"###
);

rover_case!(
    r005_large_finite_calibration_ratio,
    r###"let k=ipow(10,400)
assert (k+1)/k==1.0"###
);

rover_case!(
    r006_negative_fixed_point_division,
    r###"assert idiv(-17,5)==-3
assert idiv(17,-5)==-3
assert idiv(-17,-5)==3"###
);

rover_case!(
    r007_subnormal_measurement_ratio,
    r###"assert 1/shl(1,1074)==5e-324
assert 1/shl(1,1075)==0.0"###
);

rover_case!(
    r008_nearby_large_sensor_variance,
    r###"let b=ipow(10,100)
assert variance([b,b+2])==2.0
assert pvariance([b,b+2])==1.0"###
);

rover_case!(
    r009_high_dynamic_range_median,
    r###"assert median([1e308,1e308])==1e308
assert median([-1e308,1e308])==0.0"###
);

rover_case!(
    r010_exact_message_sequence_sorting,
    r###"assert sort([9007199254740993,9007199254740992.0,9007199254740994]) == [9007199254740992.0,9007199254740993,9007199254740994]"###
);

rover_case!(
    r011_divide_zero_is_catchable,
    r###"let r=try { 7/0 }
assert r.kind=="division_by_zero"
assert 7/2==3.5"###
);

rover_case!(
    r012_integer_divide_zero_is_catchable,
    r###"assert_fails idiv(7,0)
assert idiv(7,2)==3"###
);

rover_case!(
    r013_remainder_zero_is_catchable,
    r###"assert_fails 7 % 0
assert 7 % 3==1"###
);

rover_case!(
    r014_nan_rejected_by_sensor_guard,
    r###"assert_fails bounded_sensor(sqrt(-1.0))
assert bounded_sensor(20.0)==20.0"###
);

rover_case!(
    r015_positive_infinity_rejected_by_sensor_guard,
    r###"assert_fails bounded_sensor(exp(1000.0))"###
);

rover_case!(
    r016_negative_infinity_rejected_by_sensor_guard,
    r###"assert_fails bounded_sensor(-exp(1000.0))"###
);

rover_case!(
    r017_nan_cannot_become_integer_command,
    r###"assert_fails to_int(sqrt(-1.0))
assert_fails to_int(exp(1000.0))"###
);

rover_case!(
    r018_huge_integer_cannot_silently_become_infinity,
    r###"assert_fails to_float(ipow(10,400)) matching "range""###
);

rover_case!(
    r019_non_numeric_actuator_math_is_rejected,
    r###"assert_fails sqrt("bad") matching "type"
assert_fails abs(()) matching "type""###
);

rover_case!(
    r020_failed_math_does_not_poison_later_commands,
    r###"let r=try { idiv(3,0) }
assert r.error != ()
assert idiv(12,3)==4
assert bounded_sensor(0.0)==0.0"###
);

rover_case!(
    r021_sensor_guard_accepts_exact_endpoints,
    r###"assert bounded_sensor(-273.15)==-273.15
assert bounded_sensor(125.0)==125.0"###
);

rover_case!(
    r022_sensor_guard_rejects_out_of_range,
    r###"assert_fails bounded_sensor(-273.150001)
assert_fails bounded_sensor(125.000001)"###
);

rover_case!(
    r023_missing_telemetry_is_not_zero,
    r###"let m=map("temp",0)
assert m.missing == ()
assert m.temp == 0
assert_fails m.missing < 1"###
);

rover_case!(
    r024_false_telemetry_is_not_missing,
    r###"let m=map("ready",false)
assert (m.ready ?? true)==false
assert (m.missing ?? true)==true"###
);

rover_case!(
    r025_zero_measurement_survives_coalescing,
    r###"assert (0 ?? 99)==0
assert (0.0 ?? 99)==0.0
assert ("" ?? "absent")=="""###
);

rover_case!(
    r026_control_vector_arithmetic,
    r###"assert [1,2,3]+[4,5,6]==[5,7,9]
assert [7,8,9]-[1,2,3]==[6,6,6]"###
);

rover_case!(
    r027_vector_shape_mismatch_is_rejected,
    r###"assert_fails [1,2]+[3]
assert_fails [1,2]-[3]"###
);

rover_case!(
    r028_large_sequence_dedup_is_exact,
    r###"assert len(distinct([9007199254740993,9007199254740992.0,9007199254740993]))==2"###
);

rover_case!(
    r029_sensor_maps_compare_structurally,
    r###"assert map("a",1,"b",[2,3])==map("b",[2,3],"a",1)
assert map("x",9007199254740993)!=map("x",9007199254740992.0)"###
);

rover_case!(
    r030_no_sensor_fusion_from_empty_set,
    r###"assert_fails fuse([])
assert_fails variance([])
assert_fails median([])"###
);

rover_case!(
    r031_new_rover_is_docked,
    r###"assert phase("new")=="docked""###
);

rover_case!(
    r032_nominal_mission_round_trip,
    r###"assert arm("r",80)=="armed"
assert drive("r",2.0)=="driving"
assert sample("r",7)=="sampling"
assert resume("r")=="driving"
assert park("r")=="docked"
assert motors.get("r")==0.0"###
);

rover_case!(
    r033_cannot_drive_before_arming,
    r###"assert_fails drive("r",2.0)
assert phase("r")=="docked"
assert motors.get("r")==()"###
);

rover_case!(
    r034_repeated_arm_is_not_a_second_command,
    r###"assert arm("r",80)=="armed"
assert_fails arm("r",70)
assert battery.get("r")==80"###
);

rover_case!(
    r035_low_power_prevents_arming,
    r###"assert_fails arm("r",19)
assert phase("r")=="docked"
assert battery.get("r")==()"###
);

rover_case!(
    r036_abort_is_terminal,
    r###"assert arm("r",80)=="armed"
assert abort("r")=="fault"
assert_fails arm("r",80)
assert_fails drive("r",1.0)
assert phase("r")=="fault""###
);

rover_case!(
    r037_abort_stops_driving_motor,
    r###"assert arm("r",80)=="armed"
assert drive("r",3.0)=="driving"
assert abort("r")=="fault"
assert motors.get("r")==0.0"###
);

rover_case!(
    r038_different_robots_have_independent_states,
    r###"assert arm("a",80)=="armed"
assert phase("b")=="docked"
assert abort("a")=="fault"
assert arm("b",70)=="armed""###
);

rover_case!(
    r039_cannot_skip_to_sampling,
    r###"assert arm("r",80)=="armed"
assert_fails sample("r",5)
assert samples.get("r")==()
assert phase("r")=="armed""###
);

rover_case!(
    r040_unknown_transition_is_rejected,
    r###"assert_fails transition("r","teleported")
assert phase("r")=="docked""###
);

rover_case!(
    r041_battery_bounds_are_inclusive,
    r###"let setup = battery.set("r",0)
assert battery.get("r")==0
let setup = battery.set("r",100)
assert battery.get("r")==100"###
);

rover_case!(
    r042_battery_underflow_preserves_old_value,
    r###"let setup = battery.set("r",50)
assert_fails battery.set("r",-1)
assert battery.get("r")==50"###
);

rover_case!(
    r043_battery_overflow_preserves_old_value,
    r###"let setup = battery.set("r",50)
assert_fails battery.set("r",101)
assert battery.get("r")==50"###
);

rover_case!(
    r044_invalid_battery_type_is_rejected,
    r###"assert_fails battery.set("r","50")
assert battery.get("r")==()"###
);

rover_case!(
    r045_nan_motor_command_is_rejected,
    r###"let setup = motors.set("r",1.0)
assert_fails motors.set("r",sqrt(-1.0))
assert motors.get("r")==1.0"###
);

rover_case!(
    r046_infinite_motor_command_is_rejected,
    r###"assert_fails motors.set("r",exp(1000.0))
assert motors.get("r")==()"###
);

rover_case!(
    r047_motor_bounds_are_inclusive,
    r###"let setup = motors.set("r",0.0)
let setup = motors.set("r",5.0)
assert_fails motors.set("r",5.000001)
assert motors.get("r")==5.0"###
);

rover_case!(
    r048_command_queue_capacity_is_enforced,
    r###"let filled=fill_queue()
assert queue.len()==8
assert_fails queue.push(9)
assert queue.len()==8"###
);

rover_case!(
    r049_immutable_audit_cannot_be_rewritten,
    r###"let setup = audit.set("a","sent")
assert_fails audit.set("a","altered")
assert audit.get("a")=="sent""###
);

rover_case!(
    r050_immutable_audit_cannot_be_deleted,
    r###"let setup = audit.set("a","sent")
assert_fails audit.delete("a")
assert audit.get("a")=="sent""###
);

rover_case!(
    r051_failed_sample_rolls_back_state_and_data,
    r###"assert arm("r",80)=="armed"
assert drive("r",2.0)=="driving"
assert_fails bad_sample("r")
assert phase("r")=="driving"
assert samples.get("r")==()
assert battery.get("r")==80"###
);

rover_case!(
    r052_failed_batch_rolls_back_every_slot,
    r###"assert_fails refused_batch()
assert scratch.len()==0
assert queue.len()==0"###
);

rover_case!(
    r053_create_then_delete_is_undone,
    r###"assert_fails changed_then_deleted()
assert scratch.get("a")==()"###
);

rover_case!(
    r054_delete_rollback_restores_existing_record,
    r###"let setup = scratch.set("a",map("v",7))
assert_fails deleted_then_failed()
assert scratch.get("a")==map("v",7)"###
);

rover_case!(
    r055_repeated_overwrites_restore_original,
    r###"let setup = scratch.set("a",7)
assert_fails overwritten_then_failed()
assert scratch.get("a")==7"###
);

rover_case!(
    r056_failed_immutable_append_does_not_burn_id,
    r###"assert_fails atomic_audit()
let setup = audit.set("a","committed")
assert audit.get("a")=="committed""###
);

rover_case!(
    r057_try_is_a_nested_savepoint,
    r###"let setup = scratch.set("outside",5)
let r=try { refused_batch() }
assert r.error != ()
assert scratch.get("outside")==5
assert scratch.get("a")==()
assert queue.len()==0"###
);

rover_case!(
    r058_error_kind_and_detail_survive_catch,
    r###"let r=try { refused_batch() }
assert r.kind=="hardware_timeout"
assert r.detail=="actuator acknowledgement missing""###
);

rover_case!(
    r059_short_circuit_and_avoids_invalid_read,
    r###"assert short_circuit()==false"###
);

rover_case!(
    r060_short_circuit_or_avoids_invalid_read,
    r###"assert short_or()==true"###
);

rover_case!(
    r061_negative_list_index_reads_last,
    r###"assert [10,20,30][-1]==30
assert [10,20,30][-3]==10"###
);

rover_case!(
    r062_out_of_range_list_index_raises,
    r###"assert_fails [10,20][2]
assert_fails [10,20][-3]"###
);

rover_case!(
    r063_fractional_list_index_is_not_truncated,
    r###"assert_fails [10,20][1.5]"###
);

rover_case!(
    r064_invalid_range_step_is_rejected,
    r###"assert_fails range(0,10,0)
assert range(3,-1,-1)==[3,2,1,0]"###
);

rover_case!(
    r065_long_command_stream_can_stop_early,
    r###"assert stop_early()==3"###
);

rover_case!(
    r066_queue_elements_obey_declared_type,
    r###"let setup = queue.push(1)
assert_fails queue.push("2")
assert queue.values()==[1]"###
);

rover_case!(
    r067_map_cannot_silently_accept_list_push,
    r###"assert_fails samples.push(3)
assert samples.len()==0"###
);

rover_case!(
    r068_json_preserves_large_command_id,
    r###"let id=123456789012345678901234567890
let packet=from_json(to_json(map("id",id,"ok",false,"data",[1,()])))
assert packet.id==id
assert packet.ok==false
assert packet.data==[1,()]"###
);

rover_case!(
    r069_json_overflow_is_rejected,
    r###"assert_fails from_json("{\"reading\":1e999}") matching "json""###
);

rover_case!(
    r070_untrusted_json_cannot_forge_unknown_variants,
    r###"assert_fails from_json("{\"_type\":\"Trusted\",\"_variant\":\"Armed\",\"power\":100}")"###
);

rover_case!(
    r071_empty_string_value_is_present,
    r###"let setup = scratch.set("empty","")
assert scratch.has("empty")==true
assert scratch.get("empty")=="""###
);

rover_case!(
    r072_null_value_is_distinct_from_missing_key,
    r###"let setup = scratch.set("null",())
assert scratch.has("null")==true
assert scratch.has("missing")==false"###
);

rover_case!(
    r073_null_cannot_be_an_accidental_key,
    r###"assert_fails scratch.set((),1)
assert scratch.len()==0"###
);

rover_case!(
    r074_private_storage_keys_are_reserved,
    r###"assert_fails scratch.set("__state",1)
assert scratch.len()==0"###
);

rover_case!(
    r075_key_and_value_views_stay_consistent,
    r###"let setup = scratch.set("b",2)
let setup = scratch.set("a",1)
assert scratch.keys()==["a","b"]
assert scratch.values()==[1,2]
assert scratch.len()==2"###
);

rover_case!(
    r076_missing_delete_has_no_side_effect,
    r###"let setup = scratch.set("a",1)
assert scratch.delete("b")==false
assert scratch.len()==1"###
);

rover_case!(
    r077_text_command_ids_are_not_coerced,
    r###"let setup = scratch.set("001",1)
let setup = scratch.set("1",2)
assert scratch.len()==2
assert scratch.get("001")==1"###
);

rover_case!(
    r078_nested_map_update_does_not_alias_another_value,
    r###"let pair=copy_positions()
assert pair.before==1
assert pair.after==2"###
);

rover_case!(
    r079_unicode_command_payload_roundtrips,
    r###"let p=map("label","Lune 🚀", "line","a\nb")
assert from_json(to_json(p))==p"###
);

rover_case!(
    r080_integer_clock_difference_stays_exact,
    r###"let t=9223372036854775808
assert (t+12345)-t==12345
assert to_int(to_string(t))==t"###
);

source_case!(
    r081_unbounded_recursion_is_not_a_proof,
    "verify_fail",
    "recursive call without provable decreasing argument",
    r###"cell R { state flight { initial: ready ready -> landed } on land(id: String) { transition(id,"landed") } on spin(n: Int) { return spin(n+1) } }"###
);

source_case!(
    r082_guarded_counter_induction_is_provable,
    "verify_ok",
    "proven by induction",
    r###"cell R { memory { count: Map<String, Int> invariant count >= 0 } on step(k: String) { count.set(k,(count.get(k) ?? 0)+1) } }"###
);

source_case!(
    r083_invalid_state_target_is_static_error,
    "check_fail",
    "no such state",
    r###"cell R { state flight { initial: ready ready -> landed } on jump(id: String) { transition(id,"unknown") } }"###
);

source_case!(
    r084_unsafe_literal_write_cannot_verify,
    "verify_fail",
    "statically violated",
    r###"cell R { memory { fuel: Map<String, Int> invariant fuel >= 0 } on bad() { fuel.set("r",-1) } }"###
);

source_case!(
    r085_dynamic_state_target_cannot_pass_strict,
    "verify_fail",
    "VERIFY FAILED — --strict",
    r###"cell R { state flight { initial: ready ready -> landed } on jump(id: String,target: String) { transition(id,target) } }"###
);

source_case!(
    r086_replica_count_does_not_prove_consensus,
    "verify_fail",
    "not implemented",
    r###"cell R { memory { records: Map<String, Int> [persistent, consistent] } scale { replicas: 5 shard: records consistency: strong tolerance: 2 } }"###
);

source_case!(
    r087_eventual_cluster_is_not_a_strict_safety_proof,
    "verify_fail",
    "UNPROVEN",
    r###"cell R { memory { records: Map<String, Int> [persistent, consistent] } scale { replicas: 3 shard: records consistency: eventual tolerance: 1 } }"###
);

source_case!(
    r088_missing_handler_is_rejected,
    "check_fail",
    "undefined function",
    r###"cell R { on launch() { return missing_controller(1) } }"###
);

source_case!(
    r089_proven_decreasing_recursion_terminates,
    "verify_ok",
    "termination: all 2 handlers structurally terminate",
    r###"cell R { state flight { initial: ready ready -> landed } on land(id: String) { transition(id,"landed") } on descend(n: Int) { if n<=0 { return 0 } return descend(n-1) } }"###
);

source_case!(
    r090_typed_boundary_mismatch_is_reported,
    "runtime",
    "",
    r###"cell R { memory { ids: Map<String, Int> } }
cell test T { rules { assert_fails ids.set("x",1.5) assert ids.get("x")==() } }"###
);

source_case!(
    r091_native_encoder_add_promotion,
    "runtime",
    "",
    r###"cell N {
 face { signal interpreted(a: Int,b: Int) -> Int signal compiled(a: Int,b: Int) -> Int }
 on interpreted(a: Int,b: Int) { return a+b }
 on compiled(a: Int,b: Int) [native] { return a+b }
}
cell test T { rules { assert compiled(9223372036854775807,1)==interpreted(9223372036854775807,1) } }"###
);

source_case!(
    r092_native_distance_multiply_promotion,
    "runtime",
    "",
    r###"cell N {
 face { signal interpreted(a: Int,b: Int) -> Int signal compiled(a: Int,b: Int) -> Int }
 on interpreted(a: Int,b: Int) { return a*b }
 on compiled(a: Int,b: Int) [native] { return a*b }
}
cell test T { rules { assert compiled(9223372036854775807,100)==interpreted(9223372036854775807,100) } }"###
);

source_case!(
    r093_native_negative_integer_division,
    "runtime",
    "",
    r###"cell N {
 face { signal interpreted(a: Int,b: Int) -> Int signal compiled(a: Int,b: Int) -> Int }
 on interpreted(a: Int,b: Int) { return idiv(a,b) }
 on compiled(a: Int,b: Int) [native] { return idiv(a,b) }
}
cell test T { rules { assert compiled(-9223372036854775808,-1)==interpreted(-9223372036854775808,-1) } }"###
);

source_case!(
    r094_native_zero_divisor_is_catchable,
    "runtime",
    "",
    r###"cell N {
 face { signal interpreted(a: Int,b: Int) -> Int signal compiled(a: Int,b: Int) -> Int }
 on interpreted(a: Int,b: Int) { return idiv(a,b) }
 on compiled(a: Int,b: Int) [native] { return idiv(a,b) }
}
cell test T { rules { assert_fails compiled(1,0) assert compiled(9,3)==3 } }"###
);

source_case!(
    r095_native_wide_bit_shift_preserves_encoder_bits,
    "runtime",
    "",
    r###"cell N {
 face { signal interpreted(a: Int,b: Int) -> Int signal compiled(a: Int,b: Int) -> Int }
 on interpreted(a: Int,b: Int) { return shl(a,b) }
 on compiled(a: Int,b: Int) [native] { return shl(a,b) }
}
cell test T { rules { assert compiled(1,130)==interpreted(1,130) } }"###
);

const DURABLE: &str = r#"
cell Durable {
 memory {
  readings: Map<String, Int> [persistent, consistent]
  audit: Map<String, String> [persistent, immutable]
 }
 state mission { initial: docked docked -> armed armed -> landed }
 on write(n: Int) { readings.set("r",n) return n }
 on read() { return readings.get("r") ?? -1 }
 on refuse_write() { readings.set("r",999) fail("hardware_timeout","no ack") }
 on refuse_transition() { transition("r","armed") fail("hardware_timeout","no ack") }
 on phase() { return get_status("r") }
 on log(text: String) { audit.set("r",text) return text }
 on read_log() { return audit.get("r") }
}
"#;
fn durable(name: &str) -> PathBuf {
    let d = scratch(name);
    std::fs::write(d.join("app.cell"), DURABLE).unwrap();
    d
}
fn succeeds(dir: &Path, args: &[&str], value: &str) {
    let (code, out) = run(dir, args);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.lines().any(|l| l.trim() == value),
        "expected {value:?}: {out}"
    );
}
#[test]
fn r096_committed_telemetry_survives_process_restart() {
    let d = durable("r096");
    succeeds(&d, &["run", "app.cell", "write", "42"], "42");
    succeeds(&d, &["run", "app.cell", "read"], "42");
    let _ = std::fs::remove_dir_all(d);
}
#[test]
fn r097_failed_handler_leaves_no_persistent_writes() {
    let d = durable("r097");
    succeeds(&d, &["run", "app.cell", "write", "42"], "42");
    let (code, out) = run(&d, &["run", "app.cell", "refuse_write"]);
    assert_eq!(code, 1, "{out}");
    succeeds(&d, &["run", "app.cell", "read"], "42");
    let _ = std::fs::remove_dir_all(d);
}
#[test]
fn r098_failed_transition_leaves_no_persistent_state() {
    let d = durable("r098");
    let (code, out) = run(&d, &["run", "app.cell", "refuse_transition"]);
    assert_eq!(code, 1, "{out}");
    succeeds(&d, &["run", "app.cell", "phase"], "docked");
    let _ = std::fs::remove_dir_all(d);
}
#[test]
fn r099_immutable_audit_survives_process_restart() {
    let d = durable("r099");
    succeeds(&d, &["run", "app.cell", "log", "original"], "original");
    let (code, out) = run(&d, &["run", "app.cell", "log", "altered"]);
    assert_eq!(code, 1, "{out}");
    succeeds(&d, &["run", "app.cell", "read_log"], "original");
    let _ = std::fs::remove_dir_all(d);
}
#[test]
fn r100_record_replay_preserves_failure_classification() {
    let d = scratch("r100");
    std::fs::write(
        d.join("app.cell"),
        "cell R { on run() { fail(\"sensor_failure\",\"no packet\") } }",
    )
    .unwrap();
    let (code, out) = run(&d, &["run", "--record", "app.cell"]);
    assert_eq!(code, 1, "{out}");
    let (code, out) = run(&d, &["replay", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    std::fs::write(
        d.join("app.cell"),
        "cell R { on run() { return map(\"__error__\",\"sensor_failure: no packet\") } }",
    )
    .unwrap();
    let (code, out) = run(&d, &["replay", "app.cell"]);
    assert_eq!(code, 1, "{out}");
    let _ = std::fs::remove_dir_all(d);
}
