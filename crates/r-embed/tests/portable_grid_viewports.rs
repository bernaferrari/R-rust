use r_embed::RSession;

#[test]
fn up_preserves_named_child_and_down_restores_it() {
    let mut session = RSession::new().unwrap();
    assert!(session.render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(name='outer')); pushViewport(viewport(name='left')); upViewport(); pushViewport(viewport(name='right')); upViewport(); downViewport('left'); upViewport(); downViewport('right'); upViewport()", 200, 100).is_ok());
}

#[test]
fn pop_removes_child_while_up_keeps_it() {
    let mut session = RSession::new().unwrap();
    assert!(session.render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(name='outer')); pushViewport(viewport(name='gone')); popViewport(); pushViewport(viewport(name='kept')); upViewport(); downViewport('kept'); upViewport()", 200, 100).is_ok());
    assert!(session.render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(name='outer')); pushViewport(viewport(name='gone')); popViewport(); downViewport('gone')", 200, 100).is_err());
}

#[test]
fn seek_preserves_nested_branch_for_down_navigation() {
    let mut session = RSession::new().unwrap();
    assert!(session.render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(name='outer')); pushViewport(viewport(name='inner')); pushViewport(viewport(name='leaf')); seekViewport('outer'); downViewport('inner'); downViewport('leaf'); upViewport(2); seekViewport('outer'); upViewport()", 200, 100).is_ok());
}

#[test]
fn seek_finds_sibling_and_repeated_names_in_persisted_tree() {
    let mut session = RSession::new().unwrap();
    assert!(session.render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(name='outer')); pushViewport(viewport(name='left')); pushViewport(viewport(name='same')); upViewport(2); pushViewport(viewport(name='right')); pushViewport(viewport(name='same')); seekViewport('left'); seekViewport('same'); upViewport(); seekViewport('right'); downViewport('same'); upViewport()", 200, 100).is_ok());
}

#[test]
fn repeated_child_names_restore_the_correct_parent_coordinates() {
    let mut session = RSession::new().unwrap();
    session
        .render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(name='outer')); pushViewport(viewport(name='left',width=.2)); left<-convertWidth(unit(1,'npc'),'inches',TRUE); upViewport(); pushViewport(viewport(name='right',width=.7)); right<-convertWidth(unit(1,'npc'),'inches',TRUE); upViewport(); seekViewport('left'); left_again<-convertWidth(unit(1,'npc'),'inches',TRUE); upViewport(); seekViewport('right'); right_again<-convertWidth(unit(1,'npc'),'inches',TRUE); upViewport()", 200, 100)
        .unwrap();
    let values = session
        .eval("paste(left, right, left_again, right_again, sep=',')")
        .unwrap();
    let values = values
        .trim_start_matches("[1] \"")
        .trim_end_matches('"')
        .split(',')
        .map(|v| v.parse::<f64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 4);
    assert!((values[0] - 0.2 * 200. / 96.).abs() < 1e-6);
    assert!((values[1] - 0.7 * 200. / 96.).abs() < 1e-6);
    assert!((values[0] - values[2]).abs() < 1e-12);
    assert!((values[1] - values[3]).abs() < 1e-12);
}

#[test]
fn vp_path_selects_repeated_nested_names() {
    let mut session = RSession::new().unwrap();
    assert!(session.render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(name='outer')); pushViewport(viewport(name='left')); pushViewport(viewport(name='same',width=.2)); upViewport(2); pushViewport(viewport(name='right')); pushViewport(viewport(name='same',width=.7)); seekViewport(vpPath('outer','left','same')); a<-convertWidth(unit(1,'npc'),'inches',TRUE); seekViewport(vpPath('outer','right','same')); b<-convertWidth(unit(1,'npc'),'inches',TRUE); upViewport(3)", 200, 100).is_ok());
    let values = session.eval("paste(a,b,sep=',')").unwrap();
    assert!(values.contains("0.416") && values.contains("1.458"));
}

#[test]
fn replay_restores_grid_tree_for_down_and_conversion() {
    let mut session = RSession::new().unwrap();
    session
        .render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(name='outer')); pushViewport(viewport(name='inner',width=.4)); p<-recordPlot(); n<-length(attr(p,'rport.grid.state')); grid.newpage(); replayPlot(p); upViewport(); downViewport('inner'); restored<-convertWidth(unit(1,'npc'),'inches',TRUE); upViewport()", 200, 100)
        .unwrap();
    let value = session.eval("restored").unwrap();
    assert!(value.contains("0.833"));
}

#[test]
fn resized_replay_restores_coordinates_and_invalid_metadata_keeps_current_grid() {
    let mut session = RSession::new().unwrap();
    session.render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(name='outer',width=.4)); p<-recordPlot(); popViewport(); p2<-recordPlot()", 200,100).unwrap();
    session.render_with_dimensions("replayPlot(p); resized<-convertWidth(unit(1,'npc'),'inches',TRUE); bad<-p; attr(bad,'rport.grid.state')<-charToRaw('{}'); try(replayPlot(bad),silent=TRUE); after_bad<-convertWidth(unit(1,'npc'),'inches',TRUE); replayPlot(p2); after_pop<-convertWidth(unit(1,'npc'),'inches',TRUE)", 400,200).unwrap();
    assert_eq!(
        session
            .eval("all(abs(c(resized,after_bad,after_pop)-c(.4,.4,1)*400/96)<1e-12)")
            .unwrap(),
        "[1] TRUE"
    );
}
