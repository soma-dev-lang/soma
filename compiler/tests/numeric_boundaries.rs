//! Regressions for reductions and clamps at the Int/Float boundary.
use std::process::Command;

fn passes(name: &str, source: &str) {
    let dir = std::env::temp_dir().join(format!("soma_numeric_{}_{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["test", "app.cell"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn means_do_not_overflow_before_dividing_or_round_before_cancellation() {
    passes(
        "means",
        r#"
cell test T { rules {
 let huge=ipow(10,400)
 assert avg([1e308,1e308])==1e308
 assert avg([-1e308,-1e308]) == -1e308
 assert avg([huge,1.0,-huge])==1/3
 assert avg([huge,-huge,1.0])==1/3
 assert avg([1.0,huge,-huge])==1/3
 assert avg([9007199254740993, -9007199254740992.0])==0.5
 assert avg([1,3])==2
 assert type_of(avg([1,3]))=="Int"
 assert type_of(avg([1,3.0]))=="Float"
 assert avg([])==()
} }
"#,
    );
}

#[test]
fn pipeline_and_grouped_means_share_numeric_boundary_semantics() {
    passes(
        "pipeline_mean",
        r#"
cell test T { rules {
 let rows=[map("group","r","v",1e308),map("group","r","v",1e308)]
 assert avg_by(rows,"v")==1e308
 assert agg(rows,"group","v:avg")[0].v_avg==1e308
 let huge=ipow(10,400)
 let mixed=[map("v",to_string(huge)),map("v","1.0"),map("v",to_string(-huge))]
 assert avg_by(mixed,"v")==1/3
 assert avg_by([map("v",2),map("v",4)],"v")==3
 assert avg_by([],"v")==()
} }
"#,
    );
}

#[test]
fn means_handle_actual_nonfinite_inputs_without_fabricating_them() {
    passes(
        "nonfinite_mean",
        r#"
cell test T { rules {
 let inf=exp(1000.0)
 let huge=ipow(10,400)
 assert avg([-huge,inf])==inf
 assert avg([huge,-inf]) == -inf
 let conflict=avg([inf,-inf])
 assert conflict!=conflict
 let invalid=avg([1.0,sqrt(-1.0)])
 assert invalid!=invalid
 assert_fails avg([1,"bad"]) matching "type"
} }
"#,
    );
}

#[test]
fn deviations_keep_finite_results_when_the_variance_overflows() {
    passes(
        "large_deviation",
        r#"
cell test T { rules {
 assert pstdev([-1e308,1e308])==1e308
 assert stddev([-1e308,1e308])==1e308
 let sample=stdev([-1e308,1e308])
 assert sample>1.4e308 && sample<1.5e308
 let big=ipow(10,308)
 assert pstdev([0,big])==5e307
 assert stdev([0,big])>7e307 && stdev([0,big])<7.1e307
 assert pvariance([-1e308,1e308])==exp(1000.0)
 assert pstdev([3,3,3])==0.0
} }
"#,
    );
}

#[test]
fn deviations_keep_small_results_and_round_halfway_values_correctly() {
    passes(
        "small_deviation",
        r#"
cell test T { rules {
 assert pstdev([0.0,2e-200])==1e-200
 assert stddev([0.0,2e-200])==1e-200
 assert stdev([0.0,2e-200])>1.4e-200 && stdev([0.0,2e-200])<1.5e-200
 assert pstdev([0.0,1e-323])==5e-324
 assert pstdev([0.0,5e-324])==0.0
 assert stdev([0.0,5e-324])==5e-324
 assert pstdev([0,18014398509481986])==9007199254740992.0
 assert pstdev([0,18014398509481990])==9007199254740996.0
} }
"#,
    );
}

#[test]
fn mixed_medians_preserve_selected_integers_and_average_middle_values() {
    passes(
        "mixed_median",
        r#"
cell test T { rules {
 let b=9007199254740993
 assert median([0.0,b,b+2])==b
 assert type_of(median([0.0,b,b+2]))=="Int"
 assert median([0.0,b,b+2,1e20])==b+1
 let huge=ipow(10,400)
 assert median([0.0,huge,huge])==huge
 assert median([-huge,0.0,1.0,huge])==0.5
 assert median([1,2.0])==1.5
 assert median([1,2,3])==2
 assert median([1e308,1e308])==1e308
} }
"#,
    );
}

#[test]
fn median_nan_is_indeterminate_regardless_of_input_order() {
    passes(
        "nan_median",
        r#"
cell test T { rules {
 let nan=sqrt(-1.0)
 let a=median([nan,1,2])
 let b=median([1,nan,2])
 let c=median([1,2,nan])
 assert a!=a
 assert b!=b
 assert c!=c
 let inf=exp(1000.0)
 assert median([1,inf,inf])==inf
 assert_fails median([]) matching "empty"
 assert_fails median([1,"bad"]) matching "type"
} }
"#,
    );
}

#[test]
fn clamp_rejects_invalid_operands_and_unordered_bounds() {
    passes(
        "clamp_validation",
        r#"
cell test T { rules {
 assert_fails clamp("bad",0.0,1.0) matching "type"
 assert_fails clamp(0.5,"bad",1.0) matching "type"
 assert_fails clamp(0.5,0.0,()) matching "type"
 assert_fails clamp(0.5,sqrt(-1.0),1.0) matching "range"
 assert_fails clamp(0.5,0.0,sqrt(-1.0)) matching "range"
 assert_fails clamp(0.0,9007199254740993,9007199254740992.0)
 let nan=clamp(sqrt(-1.0),0.0,1.0)
 assert nan!=nan
 assert clamp(0.5,0.0,1.0)==0.5
} }
"#,
    );
}

#[test]
fn mixed_clamps_return_an_exact_operand_inside_the_requested_bounds() {
    passes(
        "clamp_exact",
        r#"
cell test T { rules {
 let b=9007199254740993
 assert clamp(0.0,b,b+1)==b
 assert clamp(b,0.0,b+2)==b
 assert clamp(b+4,0.0,b+2)==b+2
 assert type_of(clamp(0.0,b,b+1))=="Int"
 let huge=ipow(10,400)
 assert clamp(1.0,huge,exp(1000.0))==huge
 assert clamp(huge,0.0,huge+1)==huge
 assert clamp(-5,0,10)==0
 assert clamp(15,0,10)==10
 assert clamp(5,0,10)==5
 assert_fails clamp(5,10,0)
} }
"#,
    );
}
