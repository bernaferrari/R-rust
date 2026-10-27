//! The three fixed GNU wrapper trees use canonical R storage directly.
//! Stack arrays keep lazy materialization free of native parser buffers.
use crate::sexp::{
    ffi::{Closxp, NodeBody, SEXPTYPE},
    object::{SessionNodeFactory, Sexp, SexpResult},
    owner::OwnerToken,
};
use std::ffi::CStr;

fn symbol(factory: &SessionNodeFactory<'_>, name: &CStr) -> SexpResult<Sexp<'static>> {
    factory.require_active()?;
    factory
        .wrap(unsafe { crate::sexp::symbol::Rf_install(name.as_ptr()) })?
        .into_owned()
}

fn call(
    factory: &SessionNodeFactory<'_>,
    head: &CStr,
    args: &[Sexp<'_>],
) -> SexpResult<Sexp<'static>> {
    let head = symbol(factory, head)?;
    let mut tail = factory.nil();
    for arg in args.iter().rev() {
        tail = factory.pairlist_cell(arg, &tail, &factory.nil())?;
    }
    factory.call(&head, &tail)?.into_owned()
}

fn closure(
    factory: &SessionNodeFactory<'_>,
    environment: &Sexp<'_>,
    args: &[(&CStr, Sexp<'_>)],
    body: Sexp<'_>,
) -> SexpResult<Sexp<'static>> {
    let mut formals = factory.nil();
    for (name, default) in args.iter().rev() {
        let name = symbol(factory, name)?;
        formals = factory.pairlist_cell(default, &formals, &name)?;
    }
    let graph = NodeBody::Closure(Closxp {
        formals: factory.link(&formals)?,
        body: factory.link(&body)?,
        env: factory.link(environment)?,
    });
    factory
        .allocate(|arena| {
            let pointer = arena.alloc_node(SEXPTYPE::CLOSXP);
            let node = arena.node_token(pointer)?;
            let heap = arena.heap_identity();
            let mut header = heap.node_snapshot(&node)?;
            header.data = graph;
            heap.replace_node(&node, header)?;
            Some(pointer)
        })?
        .into_owned()
}

pub(super) fn materialize(owner: OwnerToken<'_>, index: i32) -> SexpResult<Sexp<'static>> {
    let factory = owner.node_factory();
    let environment = factory.wrap(unsafe { crate::sexp::globals::R_BaseEnv() })?;
    match index {
        3 => {
            let f = symbol(&factory, c"f")?;
            let data = symbol(&factory, c"data")?;
            let name = symbol(&factory, c"name")?;
            let val = symbol(&factory, c"val")?;
            let check = call(
                &factory,
                c"if",
                &[
                    call(
                        &factory,
                        c"!",
                        &[call(&factory, c"is.function", &[f.clone()])?],
                    )?,
                    call(
                        &factory,
                        c"stop",
                        &[factory.strings(&["handler must be a function"])?],
                    )?,
                ],
            )?;
            let invoke = call(
                &factory,
                c".Call",
                &[
                    symbol(&factory, c".C_R_addTaskCallback")?,
                    f,
                    data.clone(),
                    call(&factory, c"!", &[call(&factory, c"missing", &[data])?])?,
                    call(&factory, c"as.character", &[name])?,
                ],
            )?;
            let assign = call(&factory, c"<-", &[val.clone(), invoke])?;
            let one = factory
                .allocate(|a| crate::sexp::builder::scalar_integer_in(a, 1).map(|v| v.as_raw()))?;
            let output = call(&factory, c"+", &[val, one])?;
            let body = call(&factory, c"{", &[check, assign, output])?;
            closure(
                &factory,
                &environment,
                &[
                    (c"f", factory.missing()),
                    (c"data", factory.nil()),
                    (c"name", call(&factory, c"character", &[])?),
                ],
                body,
            )
        }
        4 => {
            let id = symbol(&factory, c"id")?;
            let check = call(
                &factory,
                c"if",
                &[
                    call(
                        &factory,
                        c"!",
                        &[call(&factory, c"is.character", &[id.clone()])?],
                    )?,
                    call(
                        &factory,
                        c"<-",
                        &[id.clone(), call(&factory, c"as.integer", &[id.clone()])?],
                    )?,
                ],
            )?;
            let invoke = call(
                &factory,
                c".Call",
                &[symbol(&factory, c".C_R_removeTaskCallback")?, id],
            )?;
            let body = call(&factory, c"{", &[check, invoke])?;
            closure(&factory, &environment, &[(c"id", factory.missing())], body)
        }
        5 => {
            let body = call(
                &factory,
                c".Call",
                &[symbol(&factory, c".C_R_getTaskCallbackNames")?],
            )?;
            closure(&factory, &environment, &[], body)
        }
        _ => unreachable!("only fixed wrapper selectors reach this constructor"),
    }
}
