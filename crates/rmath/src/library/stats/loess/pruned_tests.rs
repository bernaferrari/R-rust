use super::*;
use crate::library::stats::loess::Config;

fn cases() -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(include_str!("fixtures/pruned-surfaces.json"))
        .unwrap()["cases"].as_array().unwrap().clone()
}
fn reals(case: &serde_json::Value, name: &str) -> Vec<f64> {
    case[name].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect()
}
fn integers(case: &serde_json::Value, name: &str) -> Vec<i32> {
    case[name].as_array().unwrap().iter().map(|v| v.as_i64().unwrap() as i32).collect()
}
fn rows(flat: &[f64], n: usize, d: usize) -> Vec<Vec<f64>> {
    (0..n).map(|i| (0..d).map(|j| flat[i+j*n]).collect()).collect()
}
fn close(actual: &[f64], expected: &[f64]) {
    assert_eq!(actual.len(),expected.len());
    for (i,(a,b)) in actual.iter().zip(expected).enumerate() {
        assert!((a-b).abs() <= 2e-10 * (1.+b.abs()), "element {i}: {a} != {b}");
    }
}
fn predict(case: &serde_json::Value, execution: &Execution<'_>) -> Result<Vec<f64>,String> {
    let parameter=integers(case,"parameter");
    PrunedSurface::predict(&parameter,&integers(case,"a"),&reals(case,"xi"),
        &reals(case,"vert"),&reals(case,"vval"),
        &rows(&reals(case,"queries"),3,case["d"].as_u64().unwrap() as usize),execution)
}

#[test]
fn gnu_pruned_surfaces_restore_without_runtime_or_training_data() {
    for case in cases() {
        close(&predict(&case,&Execution::new(&|| Ok(()))).unwrap(),&reals(&case,"fit"));
    }
}

#[test]
fn exported_surface_workspaces_match_gnu_all_four_dimensions() {
    for case in cases() {
        let d=case["d"].as_u64().unwrap() as usize;
        let n=case["n"].as_u64().unwrap() as usize;
        let model=Model::fit(rows(&reals(&case,"x"),n,d),reals(&case,"y"),vec![1.;n],Config {
            span:0.85,degree:1,normalize:false,parametric:vec![false;d],drop_square:vec![false;d],
            interpolate:true,cell:0.2,iterations:1,exact:false,approximate_trace:false,
        }).unwrap();
        let surface=PrunedSurface::fit(&model,&Execution::new(&|| Ok(()))).unwrap();
        assert_eq!(surface.parameter.as_slice(),integers(&case,"parameter"),"d={d}");
        assert_eq!(surface.a,integers(&case,"a"),"d={d}");
        close(&surface.xi,&reals(&case,"xi"));
        close(&surface.vert,&reals(&case,"vert"));
        close(&surface.vval,&reals(&case,"vval"));
    }
}

#[test]
fn restored_surface_uses_its_own_jets_and_rejects_malformed_geometry() {
    for mut case in cases() {
        let baseline=predict(&case,&Execution::new(&|| Ok(()))).unwrap();
        let d=case["d"].as_u64().unwrap() as usize;
        for (i,value) in case["vval"].as_array_mut().unwrap().iter_mut().enumerate() {
            if i % (d+1)==0 { *value=serde_json::json!(value.as_f64().unwrap()+7.); }
        }
        close(&predict(&case,&Execution::new(&|| Ok(()))).unwrap(),
            &baseline.iter().map(|v| v+7.).collect::<Vec<_>>());
        let good=case.clone();
        for (field,index,value) in [
            ("parameter",0,-1.),("parameter",0,5.),("parameter",2,1.),
            ("parameter",3,2.),("parameter",4,1.),("parameter",5,0.),("parameter",6,0.),
            ("a",0,-1.),("a",0,5.),("xi",0,1e100),
        ] {
            let mut damaged=good.clone();
            damaged[field][index]=if field=="xi" { serde_json::json!(value) }
                else { serde_json::json!(value as i32) };
            assert!(predict(&damaged,&Execution::new(&|| Ok(()))).is_err(),"{field}[{index}]");
        }
        for field in ["parameter","a","xi","vert","vval"] {
            let mut damaged=good.clone();damaged[field].as_array_mut().unwrap().clear();
            // Admission is independent of the query's valid original width.
            let parameter=integers(&damaged,"parameter");
            assert!(PrunedSurface::predict(&parameter,&integers(&damaged,"a"),&reals(&damaged,"xi"),
                &reals(&damaged,"vert"),&reals(&damaged,"vval"),&[vec![0.5;d]],
                &Execution::new(&|| Ok(()))).is_err(),"truncated {field}");
        }
    }
}

#[test]
fn restored_surface_honors_cancellation_and_storage_limits() {
    let case=cases().remove(0);
    let cancelled=Execution::new(&|| Err("cancelled surface".into()));
    assert_eq!(predict(&case,&cancelled).unwrap_err(),"cancelled surface");
    let mut limited=Execution::new(&|| Ok(()));limited.workspace_limit=1;
    assert!(predict(&case,&limited).unwrap_err().contains("workspace limit"));
}
