//! Genuine public no-host contracts from pinned GNU R r90451. Dataset values
//! enter through the runtime package machinery, never test-injected bindings.
use r_embed::{RSession, RValue, RuntimePathPolicy};

fn no_host() -> RSession {
    RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        .expect("explicit no-host session")
}

fn true_result(session: &mut RSession, source: &str) {
    let output = session.eval_result(source).expect(source);
    assert_eq!(output.value, RValue::Logical(Some(true)), "{source}");
}

#[test]
fn portable_policy_initializes_real_base_and_isolates_sessions() {
    let mut left = no_host();
    let mut right = RSession::new_with_path_policy(RuntimePathPolicy::new(
        vec!["/rport/no-installed-R".into()],
        "/tmp/rport-other-policy",
    ))
    .expect("second explicit policy");
    assert_ne!(left.session_id(), right.session_id());
    assert!(left.runtime_info().library_paths.is_empty());
    assert_eq!(
        right.runtime_info().library_paths,
        ["/rport/no-installed-R"]
    );
    true_result(
        &mut left,
        "x<-1L; identical(1L+1L,2L)&&length(.libPaths())==0L",
    );
    true_result(
        &mut right,
        "x<-2L;identical(tempdir(),'/tmp/rport-other-policy')",
    );
    assert!(left.eval("stop('original no-host error')").is_err());
    true_result(&mut right, "identical(x,2L)");
    true_result(&mut left, "identical(x,1L)");
}

#[test]
fn portable_policy_gnu_namespace_exports_handles_base_ordinary_and_invalid_namespaces() {
    let mut session = no_host();
    true_result(
        &mut session,
        r#"{
        exports <- getNamespaceExports('base')
        identical(exports, names(.BaseNamespaceEnv)) && '+' %in% exports &&
        identical(getNamespaceExports('datasets'), character())
    }"#,
    );
    true_result(
        &mut session,
        r#"{
        ns <- new.env(parent=baseenv()); info <- new.env(parent=baseenv()); exported <- new.env(parent=baseenv())
        exported$beta <- TRUE; exported$alpha <- TRUE; info$exports <- exported
        assign('.__NAMESPACE__.', info, ns)
        identical(sort(getNamespaceExports(ns)), c('alpha','beta'))
    }"#,
    );
    assert!(session.eval("getNamespaceExports('no_such_pkg')").is_err());
    assert!(session.eval("getNamespaceExports(globalenv())").is_err());
    true_result(&mut session, "length(getNamespaceExports('datasets'))==0L");
}

#[test]
fn portable_datasets_all_108_types_and_attributes_match_pinned_gnu() {
    let mut session = no_host();
    let contracts = include_str!("../../rmath/src/library/datasets/assets/object-contracts.tsv");
    let mut names = Vec::new();
    for line in contracts.lines().skip(1) {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 6);
        names.push(fields[0]);
        let source = format!(
            "v<-get('{}',getNamespaceInfo('datasets','lazydata'),inherits=FALSE);paste(typeof(v),length(v),paste(class(v),collapse=','),paste(dim(v),collapse=','),paste(names(attributes(v)),collapse=','),sep='\\t')",
            fields[0]
        );
        let output = session.eval_result(&source).expect(fields[0]);
        assert_eq!(
            output.value,
            RValue::StringVector(vec![Some(fields[1..].join("\t"))]),
            "{}",
            fields[0]
        );
    }
    assert_eq!(names.len(), 108);
    let expected = names
        .iter()
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(",");
    true_result(
        &mut session,
        &format!("identical(sort(ls(getNamespaceInfo('datasets','lazydata'))),c({expected}))"),
    );
}

#[test]
fn portable_datasets_default_attachment_preserves_lazy_code_and_topic_groups() {
    let mut session = no_host();
    true_result(&mut session, "'package:datasets'%in%search()");
    true_result(
        &mut session,
        r#"{
        lazy<-getNamespaceInfo('datasets','lazydata'); attached<-as.environment('package:datasets')
        code<-quote(lazyLoadDBfetch(KEY,datafile,compressed,envhook));code[[2L]]<-c(59685L,1102L)
        before<-substitute(mtcars,attached)
        value<-datasets::mtcars
        identical(before,code)&&identical(substitute(mtcars,attached),code)&&
        identical(value,get('mtcars',lazy))&&length(getNamespaceExports('datasets'))==0L&&
        identical(tryCatch(datasets:::mtcars,error=function(e)conditionMessage(e)),"object 'mtcars' not found")
    }"#,
    );
    true_result(
        &mut session,
        r#"{
        target<-new.env(); loaded<-withVisible(data('BJsales',package='datasets',envir=target))
        identical(loaded$value,'BJsales')&&!loaded$visible&&
        identical(sort(ls(target)),c('BJsales','BJsales.lead'))&&
        identical(target$BJsales,datasets::BJsales)&&identical(target$BJsales.lead,datasets::BJsales.lead)
    }"#,
    );
    true_result(
        &mut session,
        r#"{
        index <- data(package='datasets')$results
        identical(dim(index), c(108L,4L)) &&
        identical(colnames(index),c('Package','LibPath','Item','Title')) &&
        identical(index[1L,c('Package','Item','Title')],
            c(Package='datasets',Item='AirPassengers',Title='Monthly Airline Passenger Numbers 1949-1960')) &&
        identical(index[3L,c('Package','Item','Title')],
            c(Package='datasets',Item='BJsales.lead (BJsales)',Title='Sales Data with Leading Indicator'))
    }"#,
    );
    true_result(
        &mut session,
        r#"{
        copy<-datasets::mtcars;copy$mpg[[1L]]<--99;gc()
        identical(datasets::mtcars$mpg[[1L]],21)&&identical(substitute(mtcars,attached),code)
    }"#,
    );
}

#[test]
fn portable_datasets_all_91_topic_groups_match_original_gnu_values() {
    // Names only, generated from the authenticated original inventory.json.
    // Every value is loaded by production data(), independently of lazydata.
    const TOPICS: &[(&str, &[&str])] = &[
        ("AirPassengers", &["AirPassengers"]),
        ("BJsales", &["BJsales", "BJsales.lead"]),
        ("BOD", &["BOD"]),
        ("CO2", &["CO2"]),
        ("ChickWeight", &["ChickWeight"]),
        ("DNase", &["DNase"]),
        ("EuStockMarkets", &["EuStockMarkets"]),
        ("Formaldehyde", &["Formaldehyde"]),
        ("HairEyeColor", &["HairEyeColor"]),
        ("Harman23.cor", &["Harman23.cor"]),
        ("Harman74.cor", &["Harman74.cor"]),
        ("Indometh", &["Indometh"]),
        ("InsectSprays", &["InsectSprays"]),
        ("JohnsonJohnson", &["JohnsonJohnson"]),
        ("LakeHuron", &["LakeHuron"]),
        ("LifeCycleSavings", &["LifeCycleSavings"]),
        ("Loblolly", &["Loblolly"]),
        ("Nile", &["Nile"]),
        ("Orange", &["Orange"]),
        ("OrchardSprays", &["OrchardSprays"]),
        ("PlantGrowth", &["PlantGrowth"]),
        ("Puromycin", &["Puromycin"]),
        ("Seatbelts", &["Seatbelts"]),
        ("Theoph", &["Theoph"]),
        ("Titanic", &["Titanic"]),
        ("ToothGrowth", &["ToothGrowth"]),
        ("UCBAdmissions", &["UCBAdmissions"]),
        ("UKDriverDeaths", &["UKDriverDeaths"]),
        ("UKLungDeaths", &["fdeaths", "ldeaths", "mdeaths"]),
        ("UKgas", &["UKgas"]),
        ("USAccDeaths", &["USAccDeaths"]),
        ("USArrests", &["USArrests"]),
        ("USJudgeRatings", &["USJudgeRatings"]),
        ("USPersonalExpenditure", &["USPersonalExpenditure"]),
        ("UScitiesD", &["UScitiesD"]),
        ("VADeaths", &["VADeaths"]),
        ("WWWusage", &["WWWusage"]),
        ("WorldPhones", &["WorldPhones"]),
        ("ability.cov", &["ability.cov"]),
        ("airmiles", &["airmiles"]),
        ("airquality", &["airquality"]),
        ("anscombe", &["anscombe"]),
        ("attenu", &["attenu"]),
        ("attitude", &["attitude"]),
        ("austres", &["austres"]),
        ("beavers", &["beaver1", "beaver2"]),
        ("cars", &["cars"]),
        ("chickwts", &["chickwts"]),
        ("co2", &["co2"]),
        ("crimtab", &["crimtab"]),
        ("discoveries", &["discoveries"]),
        ("esoph", &["esoph"]),
        ("euro", &["euro", "euro.cross"]),
        ("eurodist", &["eurodist"]),
        ("faithful", &["faithful"]),
        ("freeny", &["freeny", "freeny.x", "freeny.y"]),
        ("gait", &["gait"]),
        ("infert", &["infert"]),
        ("iris", &["iris"]),
        ("iris3", &["iris3"]),
        ("islands", &["islands"]),
        ("lh", &["lh"]),
        ("longley", &["longley"]),
        ("lynx", &["lynx"]),
        ("morley", &["morley"]),
        ("mtcars", &["mtcars"]),
        ("nhtemp", &["nhtemp"]),
        ("nottem", &["nottem"]),
        ("npk", &["npk"]),
        ("occupationalStatus", &["occupationalStatus"]),
        ("penguins", &["penguins", "penguins_raw"]),
        ("precip", &["precip"]),
        ("presidents", &["presidents"]),
        ("pressure", &["pressure"]),
        ("quakes", &["quakes"]),
        ("randu", &["randu"]),
        ("rivers", &["rivers"]),
        ("rock", &["rock"]),
        ("sleep", &["sleep"]),
        ("stackloss", &["stack.loss", "stack.x", "stackloss"]),
        (
            "state",
            &[
                "state.abb",
                "state.area",
                "state.center",
                "state.division",
                "state.name",
                "state.region",
                "state.x77",
            ],
        ),
        ("sunspot.month", &["sunspot.m2014", "sunspot.month"]),
        ("sunspot.year", &["sunspot.year"]),
        ("sunspots", &["sunspots"]),
        ("swiss", &["swiss"]),
        ("treering", &["treering"]),
        ("trees", &["trees"]),
        ("uspop", &["uspop"]),
        ("volcano", &["volcano"]),
        ("warpbreaks", &["warpbreaks"]),
        ("women", &["women"]),
    ];
    let mut session = no_host();
    true_result(
        &mut session,
        "lazy<-getNamespaceInfo('datasets','lazydata');TRUE",
    );
    assert_eq!(TOPICS.len(), 91);
    assert_eq!(
        TOPICS
            .iter()
            .map(|(topic, _)| *topic)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        91
    );
    assert_eq!(
        TOPICS
            .iter()
            .flat_map(|(_, names)| names.iter().copied())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        108
    );
    for &(topic, names) in TOPICS {
        let expected = names
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(",");
        true_result(
            &mut session,
            &format!(
                "target<-new.env();loaded<-withVisible(data('{topic}',package='datasets',envir=target));\
             identical(loaded$value,'{topic}')&&!loaded$visible&&\
             identical(sort(ls(target)),sort(c({expected})))&&\
             all(vapply(c({expected}),function(name)identical(get(name,target),get(name,lazy)),logical(1L)))"
            ),
        );
    }
}

#[test]
fn portable_datasets_real_mtcars_model_and_covratio_match_gnu() {
    let mut session = no_host();
    true_result(
        &mut session,
        r#"{
        fit<-lm(mpg~wt+hp,data=mtcars)
        expected<-c(1.04303730954591,1.11197516466917,1.06750970889065,1.16604607726619,
            1.15098025821712,1.08372768147286,1.22162448034606,1.20696169975472,
            1.16942160263852,1.1543068219349,1.07860516824836,1.168456969743,
            1.15757982314696,1.11040481124057,1.3644478256944,1.37535253780841,
            0.722665308328846,0.647648859338367,1.24067727287683,0.643380489886664,
            1.00090577614009,0.962172572957545,0.888099097102707,1.20557569360369,
            1.05459961438794,1.22171278564254,1.19561720770636,1.1544834399644,
            1.38150029634068,1.16161919986146,1.60618779991019,1.11298877602903)
        got<-covratio(fit)
        length(got)==32L&&all(abs(as.numeric(got)-expected)<1e-8)&&
        identical(names(got),row.names(mtcars))
    }"#,
    );
}

#[test]
fn portable_datasets_serialized_values_match_independent_gnu_graph() {
    let mut session = no_host();
    let output = session.eval_result(
        "lazy<-getNamespaceInfo('datasets','lazydata');serialize(mget(sort(ls(lazy)),lazy,inherits=FALSE),NULL,version=2)"
    ).expect("serialize all 108 dataset values");
    let RValue::RawVector(actual) = output.value else {
        panic!("expected owning serialized byte snapshot");
    };
    let expected = include_bytes!("../../rmath/src/library/datasets/assets/values.rds");
    assert_eq!(
        actual.len(),
        expected.len(),
        "serialization size; values/attributes have separate contracts"
    );
    if actual.as_slice() != expected {
        let path = std::env::temp_dir().join(format!(
            "rport-datasets-actual-values-{}.rds",
            std::process::id()
        ));
        std::fs::write(&path, &actual).expect("save actual serializer diagnostic");
        let differing: Vec<_> = actual
            .iter()
            .zip(expected)
            .enumerate()
            .filter_map(|(offset, (actual, expected))| {
                (actual != expected).then_some((offset, *actual, *expected))
            })
            .collect();
        eprintln!(
            "actual dataset graph {}; {} differing bytes: {:?}",
            path.display(),
            differing.len(),
            differing
        );
    }
    assert!(
        actual.as_slice() == expected,
        "serialization bytes differ; preserve this as a separate serializer gap"
    );
}
