use r_embed::RSession;

#[test]
fn portable_unit_length_counts_values_and_arithmetic_results() {
    let mut session = RSession::new().unwrap();
    let value = session
        .eval("library(grid); a <- unit(c(1,2,3), 'npc'); b <- unit(c(4,5), 'cm'); c <- a + unit(c(1,2,3), 'npc'); length(a)==3L && length(b)==2L && length(c)==3L && length.unit(a)==3L")
        .unwrap();
    assert_eq!(value, "[1] TRUE");
}
