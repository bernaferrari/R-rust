//! Exercise the real checked .C path, owning publication, and NAOK admission.
use super::*;

fn values(factory: &SessionNodeFactory<'_>) -> Vec<Sexp<'static>> {
    vec![
        real(factory, &[20., 19., 21., 23., 20., 24., 26.]),
        integer(factory, &[7]),
        real(factory, &[0.3]),
        real(factory, &[0.1]),
        real(factory, &[0.2]),
        integer(factory, &[2]),
        integer(factory, &[1]),
        integer(factory, &[3]),
        integer(factory, &[1]),
        integer(factory, &[1]),
        real(factory, &[17.5]),
        real(factory, &[-0.5]),
        real(factory, &[0.5, 1.5, -0.25]),
        real(factory, &[3.75]),
        real(factory, &[-111.; 9]),
        real(factory, &[5.25; 9]),
        real(factory, &[2.25; 11]),
    ]
}

#[test]
fn holtwinters_checked_dispatch_owns_results_and_does_not_mutate_aliased_sources_after_gc() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let nil = factory.nil().into_owned().unwrap();
    let name = factory
        .strings(&["C_HoltWinters"])
        .unwrap()
        .into_owned()
        .unwrap();
    let mut sources = values(&factory);
    // Repeated R inputs still receive independent native buffers.
    sources[15] = sources[14].clone();
    let output_inputs = sources[14].clone();
    let mut arguments: Vec<_> = sources.into_iter().map(|v| (v, nil.clone())).collect();
    arguments.insert(
        4,
        (
            factory.strings(&["stats"]).unwrap().into_owned().unwrap(),
            symbol(&session, "PACKAGE"),
        ),
    );
    arguments.insert(7, (integer(&factory, &[0]), symbol(&session, "DUP")));
    let call = request(&factory, &name, &arguments);
    drop(arguments);
    let result = invoke_request(&session, &call, BufferInterface::C)
        .unwrap()
        .into_owned()
        .unwrap();
    assert_eq!(
        result.len(),
        17,
        "control arguments are not native result payloads"
    );
    let level = result.try_vector_elt(14).unwrap().into_owned().unwrap();
    let trend = result.try_vector_elt(15).unwrap().into_owned().unwrap();
    assert_ne!(level.as_raw(), trend.as_raw());
    assert_ne!(level.as_raw(), output_inputs.as_raw());
    assert_eq!(output_inputs.try_real_elt(0).unwrap(), -111.0);
    assert_eq!(level.try_real_elt(0).unwrap(), 17.5);
    assert_eq!(trend.try_real_elt(0).unwrap(), -0.5);
    assert!(
        (result.try_vector_elt(13).unwrap().try_real_elt(0).unwrap() - 90.24895215249988).abs()
            < 1e-12
    );
    // Remove source argument graphs and incidental result-container ownership.
    drop(call);
    drop(result);
    drop(output_inputs);
    drop(name);
    session.with_active(crate::sexp::gengc::full_gc);
    assert!((level.try_real_elt(6).unwrap() - 21.447622356265).abs() < 1e-12);
    assert!((trend.try_real_elt(6).unwrap() - 0.0771669641815).abs() < 1e-12);
}

#[test]
fn holtwinters_checked_dispatch_preserves_naok_and_rejects_bad_shape_before_invocation() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let nil = factory.nil().into_owned().unwrap();
    let name = factory
        .strings(&["HoltWinters"])
        .unwrap()
        .into_owned()
        .unwrap();
    let mut inputs = values(&factory);
    inputs[0] = real(&factory, &[20., f64::NAN, 21., 23., 20., 24., 26.]);
    let mut arguments: Vec<_> = inputs.into_iter().map(|v| (v, nil.clone())).collect();
    let before = buffers::invocation_count();
    assert!(
        invoke_request(
            &session,
            &request(&factory, &name, &arguments),
            BufferInterface::C
        )
        .is_err()
    );
    assert_eq!(buffers::invocation_count(), before);
    arguments.insert(3, (integer(&factory, &[1]), symbol(&session, "NAOK")));
    let output = invoke_request(
        &session,
        &request(&factory, &name, &arguments),
        BufferInterface::C,
    )
    .unwrap();
    assert!(
        output
            .try_vector_elt(13)
            .unwrap()
            .try_real_elt(0)
            .unwrap()
            .is_nan()
    );
    assert!(
        output
            .try_vector_elt(14)
            .unwrap()
            .try_real_elt(1)
            .unwrap()
            .is_nan()
    );
    assert_eq!(buffers::invocation_count(), before + 1);
    let mut short = values(&factory);
    short[16] = real(&factory, &[0.; 8]);
    let short: Vec<_> = short.into_iter().map(|v| (v, nil.clone())).collect();
    assert!(
        invoke_request(
            &session,
            &request(&factory, &name, &short),
            BufferInterface::C
        )
        .is_err()
    );
    assert_eq!(buffers::invocation_count(), before + 1);
}
