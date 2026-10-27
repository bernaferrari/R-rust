use r_embed::{RSession, RuntimePathPolicy};

fn sessions() -> [RSession; 2] {
    [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap(),
    ]
}

#[test]
fn callback_public_positions_names_and_duplicate_removal_match_gnu() {
    for mut session in sessions() {
        let output=session.eval(r#"local({
            f<-function(...)TRUE
            a<-addTaskCallback(f,name='left');b<-addTaskCallback(f,name='right')
            removeTaskCallback(1L);c<-addTaskCallback(f,name='after')
            cat(unname(a),unname(b),unname(c),paste(c(names(a),names(b),names(c)),collapse=','),paste(getTaskCallbackNames(),collapse=','),'\n')
            removeTaskCallback(1L);removeTaskCallback(1L)
            addTaskCallback(f,name='same');addTaskCallback(f,name='same');removeTaskCallback('same')
            cat(paste(getTaskCallbackNames(),collapse=','),length(getTaskCallbackNames()),'\n')
            removeTaskCallback('same')
        })"#).unwrap();
        assert_eq!(
            output,
            "1 2 2 left,right,after right,after \nsame 1 \n[1] TRUE\n"
        );
    }
}

#[test]
fn callback_hidden_typed_registration_checks_arity_and_rejects_forgery() {
    for mut session in sessions() {
        let output=session.eval(r#"local({
            cat(exists('.C_R_getTaskCallbackNames',baseenv(),inherits=FALSE),bindingIsLocked('.C_R_getTaskCallbackNames',baseenv()),'\n')
            info<-.C_R_getTaskCallbackNames
            cat(.C_R_addTaskCallback$numParameters,.C_R_removeTaskCallback$numParameters,info$numParameters,inherits(info,'CallRoutine'),inherits(info$address,'RegisteredNativeSymbol'),'\n')
            wrong<-inherits(try(.Call(info,1L),silent=TRUE),'try-error')
            hidden<-inherits(try(getNativeSymbolInfo('R_addTaskCallback'),silent=TRUE),'try-error')
            string_lookup<-inherits(try(.Call('R_getTaskCallbackNames'),silent=TRUE),'try-error')
            forged<-inherits(try(.Call(list(name='R_getTaskCallbackNames',address=new('externalptr'),numParameters=0L)),silent=TRUE),'try-error')
            cat(wrong,hidden,string_lookup,forged,length(.Call(info)),'\n')
        })"#).unwrap();
        assert_eq!(
            output,
            "TRUE TRUE \n4 1 0 TRUE TRUE \nTRUE TRUE TRUE TRUE 0 \n"
        );
    }
}

#[test]
fn callback_wrappers_match_gnu_formals_bodies_environments_and_visibility() {
    for mut session in sessions() {
        let output=session.eval(r#"local({
          expected<-list(
            function(f,data=NULL,name=character()) {if(!is.function(f))stop('handler must be a function');val<-.Call(.C_R_addTaskCallback,f,data,!missing(data),as.character(name));val+1L},
            function(id) {if(!is.character(id))id<-as.integer(id);.Call(.C_R_removeTaskCallback,id)},
            function() .Call(.C_R_getTaskCallbackNames))
          actual<-list(addTaskCallback,removeTaskCallback,getTaskCallbackNames)
          same<-vapply(seq_along(actual),function(i)identical(formals(actual[[i]]),formals(expected[[i]]))&&identical(body(actual[[i]]),body(expected[[i]]))&&identical(environment(actual[[i]]),baseenv()),logical(1))
          add<-withVisible(addTaskCallback(function(...)TRUE,name='visible'))
          remove<-withVisible(removeTaskCallback('visible'))
          query<-withVisible(getTaskCallbackNames())
          cat(all(same),add$visible,remove$visible,query$visible,add$value,remove$value,length(query$value),'\n')
        })"#).unwrap();
        assert_eq!(output, "TRUE TRUE TRUE TRUE 1 TRUE 0 \n");
    }
}
