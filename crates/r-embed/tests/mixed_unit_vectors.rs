use r_embed::RSession;

#[test]
fn mixed_unit_arithmetic_recycles_per_coordinate_and_summary_stays_scalar() {
    let mut session = RSession::new().unwrap();
    let value = session
        .render_with_dimensions(
            "library(grid); grid.newpage(); a<-convertWidth(unit(c(1,2),'npc')+unit(c(1,2),'cm'),'inches',TRUE); b<-convertWidth(unit(1,'npc')+unit(c(1,2),'cm'),'inches',TRUE); d<-convertWidth(unit(c(1,2),'npc')+unit(1,'cm'),'inches',TRUE); e<-convertWidth(unit(c(1,2),'npc')-unit(c(1,2),'cm'),'inches',TRUE); f<-convertWidth((unit(c(1,2),'npc')+unit(c(1,2),'cm'))*2,'inches',TRUE); s<-convertWidth(sum(unit(c(1,2),'npc')),'inches',TRUE); scaled<-convertWidth(sum(unit(c(1,2),c('npc','cm')))*2,'inches',TRUE); nested<-convertWidth((unit(c(1,2),'npc')+unit(c(1,2),'cm'))+unit(1,'cm'),'inches',TRUE)",
            384,
            192,
        )
        .unwrap();
    let _ = value;
    let result = session
        .eval("all(abs(a-c(4.3937007874,8.7874015748))<1e-6) && all(abs(b-c(4.3937007874,4.7874015748))<1e-6) && all(abs(d-c(4.3937007874,8.3937007874))<1e-6) && all(abs(e-c(3.6062992126,7.2125984252))<1e-6) && all(abs(f-c(8.7874015748,17.5748031496))<1e-6) && length(s)==1L && abs(s-12)<1e-6 && abs(scaled-(8+4/2.54))<1e-6 && all(abs(nested-c(4+2/2.54,8+3/2.54))<1e-6)")
        .unwrap();
    assert_eq!(result, "[1] TRUE");
}
