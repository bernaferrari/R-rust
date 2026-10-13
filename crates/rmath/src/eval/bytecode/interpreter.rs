//! Private bytecode interpreter (eval_bytecode) and its helpers — extracted verbatim from eval/bytecode.rs

use super::*;

pub(crate) fn read_operand(
    bytecode: &[c_int],
    pc: &mut usize,
    opname: &str,
) -> Result<c_int, String> {
    let value = bytecode
        .get(*pc)
        .copied()
        .ok_or_else(|| format!("{opname} bytecode operand is truncated"))?;
    *pc += 1;
    Ok(value)
}

pub(crate) fn read_operand_index(
    bytecode: &[c_int],
    pc: &mut usize,
    opname: &str,
) -> Result<usize, String> {
    let value = read_operand(bytecode, pc, opname)?;
    if value < 0 {
        return Err(format!(
            "{opname} bytecode operand index {value} is negative"
        ));
    }
    Ok(value as usize)
}

pub(crate) fn read_jump_target(
    bytecode: &[c_int],
    pc: &mut usize,
    opname: &str,
) -> Result<usize, String> {
    let target = read_operand_index(bytecode, pc, opname)?;
    if target > bytecode.len() {
        return Err(format!(
            "{opname} bytecode jump target {target} is outside instruction stream length {}",
            bytecode.len()
        ));
    }
    Ok(target)
}

/// Passive node provenance and owning values stay separate from execution.
/// The scoped access and allocator cannot escape into retained callback data.
pub(crate) struct PrivateExecution<'execution> {
    pub(crate) access: &'execution RuntimeAccess,
    pub(crate) domain: NodeDomain<'static>,
    pub(crate) allocator: NodeAllocator<'execution, 'static>,
    pub(crate) nil: Sexp<'static>,
    pub(crate) true_value: Sexp<'static>,
    pub(crate) false_value: Sexp<'static>,
}
impl PrivateExecution<'_> {
    pub(crate) fn native<T>(
        &self,
        operation: impl for<'operation> FnOnce(crate::sexp::owner::OwnerToken<'operation>) -> T,
    ) -> Result<T, String> {
        self.access
            .with_native(|owner| Ok(operation(owner)))
            .map_err(|e| sexp_err("private bytecode native adapter", e))
    }
    pub(crate) fn require_live(&self) -> Result<(), String> {
        self.access
            .require_active()
            .map_err(|e| sexp_err("private bytecode owner", e))
    }
}

pub(crate) fn initialized_scalar(
    execution: &PrivateExecution<'_>,
    kind: SEXPTYPE,
    initialize: impl FnOnce(&crate::sexp::payload::PayloadLease) -> Option<()>,
) -> Result<Sexp<'static>, String> {
    execution.require_live()?;
    let result = execution
        .allocator
        .allocate(|arena| {
            let pointer = arena.alloc_vector(kind, 1);
            let heap = arena.heap_identity();
            let node = arena.node_token(pointer)?;
            initialize(&heap.payload_lease(&node)?)?;
            let mut header = heap.node_snapshot(&node)?;
            header.sxpinfo.set_scalar(true);
            heap.replace_node(&node, header)?;
            Some(pointer)
        })
        .map_err(|e| sexp_err("private bytecode scalar", e))?;
    execution.require_live()?;
    Ok(result)
}
pub(crate) fn make_lgl(
    execution: &PrivateExecution<'_>,
    val: c_int,
) -> Result<Sexp<'static>, String> {
    initialized_scalar(execution, SEXPTYPE::LGLSXP, |lease| {
        lease.set_integer_elt(0, val)
    })
}
pub(crate) fn make_real(
    execution: &PrivateExecution<'_>,
    val: c_double,
) -> Result<Sexp<'static>, String> {
    initialized_scalar(execution, SEXPTYPE::REALSXP, |lease| {
        lease.set_real_elt(0, val)
    })
}
pub(crate) fn make_int(
    execution: &PrivateExecution<'_>,
    val: c_int,
) -> Result<Sexp<'static>, String> {
    initialized_scalar(execution, SEXPTYPE::INTSXP, |lease| {
        lease.set_integer_elt(0, val)
    })
}
pub(crate) fn push_operand(
    stack: &mut Vec<Sexp<'static>>,
    value: Sexp<'static>,
) -> Result<(), String> {
    stack
        .try_reserve(1)
        .map_err(|_| "cannot grow private bytecode stack")?;
    stack.push(value);
    Ok(())
}
pub(crate) fn pop_arguments(
    stack: &mut Vec<Sexp<'static>>,
    count: usize,
) -> Result<Vec<Sexp<'static>>, String> {
    let start = stack
        .len()
        .checked_sub(count)
        .ok_or("private bytecode call argument stack underflow")?;
    let mut args = Vec::new();
    args.try_reserve_exact(count)
        .map_err(|_| "cannot reserve private bytecode arguments")?;
    args.extend(stack.drain(start..));
    Ok(args)
}
pub(crate) fn argument_list(
    execution: &PrivateExecution<'_>,
    args: &[Sexp<'static>],
) -> Result<Sexp<'static>, String> {
    let mut list = execution.nil.clone();
    for arg in args.iter().rev() {
        list = execution
            .allocator
            .pairlist_cell(arg, &list, &execution.nil)
            .map_err(|e| sexp_err("private bytecode argument cell", e))?;
        execution.require_live()?;
    }
    Ok(list)
}
pub(crate) fn language_call(
    execution: &PrivateExecution<'_>,
    fun: &Sexp<'static>,
    args: &Sexp<'static>,
) -> Result<Sexp<'static>, String> {
    let body = crate::sexp::ffi::NodeBody::List(crate::sexp::ffi::Listsxp {
        carval: execution.domain.link(fun).map_err(|e| e.to_string())?,
        cdrval: execution.domain.link(args).map_err(|e| e.to_string())?,
        tagval: execution
            .domain
            .link(&execution.nil)
            .map_err(|e| e.to_string())?,
    });
    let call = execution
        .allocator
        .allocate(|arena| {
            let pointer = arena.alloc_node(SEXPTYPE::LANGSXP);
            let heap = arena.heap_identity();
            let node = arena.node_token(pointer)?;
            let mut header = heap.node_snapshot(&node)?;
            header.data = body;
            heap.replace_node(&node, header)?;
            Some(pointer)
        })
        .map_err(|e| sexp_err("private bytecode call", e))?;
    execution.require_live()?;
    Ok(call)
}

pub(crate) fn scalar_int(value: &Sexp<'_>, context: &str) -> Result<c_int, String> {
    value
        .try_integer_elt(0)
        .map_err(|err| sexp_err(context, err))
}

pub(crate) fn scalar_real(value: &Sexp<'_>, context: &str) -> Result<c_double, String> {
    value.try_real_elt(0).map_err(|err| sexp_err(context, err))
}

pub(crate) fn scalar_f64_or_zero(value: &Sexp<'_>, context: &str) -> Result<c_double, String> {
    match value.clone().typeof_() {
        SEXPTYPE::REALSXP => scalar_real(value, context),
        SEXPTYPE::INTSXP => scalar_int(value, context).map(c_double::from),
        SEXPTYPE::LGLSXP => value
            .try_logical_elt(0)
            .map(c_double::from)
            .map_err(|err| sexp_err(context, err)),
        _ => Ok(0.0),
    }
}

pub(crate) fn scalar_bool_or_false(value: &Sexp<'_>, context: &str) -> Result<bool, String> {
    match value.clone().typeof_() {
        SEXPTYPE::LGLSXP => value
            .try_logical_elt(0)
            .map(|value| value != 0)
            .map_err(|err| sexp_err(context, err)),
        SEXPTYPE::INTSXP => scalar_int(value, context).map(|value| value != 0),
        SEXPTYPE::REALSXP => scalar_real(value, context).map(|value| value != 0.0),
        _ => Ok(false),
    }
}

pub(crate) fn apply_binary_op<FR, FI>(
    execution: &PrivateExecution<'_>,
    a: Sexp<'static>,
    b: Sexp<'static>,
    real_op: FR,
    int_op: FI,
) -> Result<Sexp<'static>, String>
where
    FR: Fn(c_double, c_double) -> c_double,
    FI: Fn(c_int, c_int) -> c_int,
{
    if a.clone().typeof_() == SEXPTYPE::REALSXP && b.clone().typeof_() == SEXPTYPE::REALSXP {
        let av = scalar_real(&a, "left real operand")?;
        let bv = scalar_real(&b, "right real operand")?;
        make_real(execution, real_op(av, bv))
    } else if a.clone().typeof_() == SEXPTYPE::INTSXP && b.clone().typeof_() == SEXPTYPE::INTSXP {
        let av = scalar_int(&a, "left integer operand")?;
        let bv = scalar_int(&b, "right integer operand")?;
        make_int(
            execution,
            if av == c_int::MIN || bv == c_int::MIN {
                c_int::MIN
            } else {
                int_op(av, bv)
            },
        )
    } else {
        let av = scalar_f64_or_zero(&a, "left numeric operand")?;
        let bv = scalar_f64_or_zero(&b, "right numeric operand")?;
        make_real(execution, real_op(av, bv))
    }
}

pub(crate) fn apply_comparison<F>(
    execution: &PrivateExecution<'_>,
    a: Sexp<'static>,
    b: Sexp<'static>,
    cmp: F,
) -> Result<Sexp<'static>, String>
where
    F: Fn(c_double, c_double) -> bool,
{
    let result = if a.clone().typeof_() == SEXPTYPE::REALSXP
        && b.clone().typeof_() == SEXPTYPE::REALSXP
    {
        let av = scalar_real(&a, "left real comparison operand")?;
        let bv = scalar_real(&b, "right real comparison operand")?;
        if cmp(av, bv) { 1 } else { 0 }
    } else if a.clone().typeof_() == SEXPTYPE::INTSXP && b.clone().typeof_() == SEXPTYPE::INTSXP {
        let av = scalar_int(&a, "left integer comparison operand")? as c_double;
        let bv = scalar_int(&b, "right integer comparison operand")? as c_double;
        if cmp(av, bv) { 1 } else { 0 }
    } else {
        let av = scalar_f64_or_zero(&a, "left comparison operand")?;
        let bv = scalar_f64_or_zero(&b, "right comparison operand")?;
        if cmp(av, bv) { 1 } else { 0 }
    };
    make_lgl(execution, result)
}

pub fn eval_bytecode<'a>(code: Sexp<'a>, env: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let code = code
        .into_owned()
        .map_err(|e| sexp_err("private bytecode code", e))?;
    let env = env
        .into_owned()
        .map_err(|e| sexp_err("private bytecode environment", e))?;
    let owner = code
        .runtime_owner
        .clone()
        .ok_or("private bytecode has no runtime owner")?;
    crate::sexp::owner::with_runtime(&owner, |access| eval_bytecode_in_scope(code, env, access))
        .map_err(|e| sexp_err("private bytecode owner", e))?
}

pub(crate) fn eval_bytecode_in_scope(
    code: Sexp<'static>,
    env: Sexp<'static>,
    access: &RuntimeAccess,
) -> Result<Sexp<'static>, String> {
    let domain = access.domain();
    let allocator = access.allocator(&domain).map_err(|e| e.to_string())?;
    domain
        .link(&env)
        .map_err(|e| sexp_err("private bytecode environment domain", e))?;
    if env.typeof_() != SEXPTYPE::ENVSXP {
        return Err("private bytecode requires an environment".into());
    }
    let execution = PrivateExecution {
        true_value: domain.logical(true),
        false_value: domain.logical(false),
        nil: domain.nil(),
        domain,
        allocator,
        access,
    };
    execution.require_live()?;
    if code.typeof_() != SEXPTYPE::INTSXP {
        return Err("invalid bytecode vector: expected integer vector".into());
    }
    // Snapshot actual pool entries before any lazy instruction reader callback.
    let constants = execution.native(|_| snapshot_constants(code.attrib()))??;
    let instruction_count = usize::try_from(code.len()).map_err(|_| "invalid bytecode length")?;
    let scratch_bytes = instruction_count
        .checked_mul(std::mem::size_of::<c_int>())
        .ok_or("bytecode scratch size overflow")?;
    let _scratch = access
        .with_arena(|arena| arena.try_reserve_transient(scratch_bytes))
        .map_err(|e| e.to_string())?
        .ok_or("bytecode scratch exceeds session memory limit")?;
    let mut bytecode = Vec::new();
    bytecode
        .try_reserve_exact(instruction_count)
        .map_err(|_| "failed to allocate bytecode scratch")?;
    bytecode.resize(instruction_count, 0);
    access
        .with_native(|_| code.copy_integer_into(&mut bytecode))
        .map_err(|e| sexp_err("invalid bytecode vector", e))?;
    execution.require_live()?;
    let mut pc = 0;
    let mut stack = Vec::new();
    let (result, _) =
        eval_bytecode_loop(&bytecode, &mut pc, &mut stack, &constants, env, &execution)?;
    execution.require_live()?;
    Ok(result)
}

pub(crate) enum PrivateStep {
    Complete(Sexp<'static>, ControlFlow),
    Loop(PrivateLoop),
}
pub(crate) struct PrivateLoop {
    pub(crate) origin: usize,
    pub(crate) end: usize,
    pub(crate) kind: PrivateLoopKind,
}
pub(crate) struct PrivateFor {
    pub(crate) variable: Sexp<'static>,
    pub(crate) sequence: Sexp<'static>,
    pub(crate) body: usize,
    pub(crate) length: usize,
    pub(crate) next: usize,
}
pub(crate) enum PrivateLoopKind {
    For(Box<PrivateFor>),
    While {
        condition: usize,
        body: usize,
        testing: bool,
    },
    Repeat {
        body: usize,
    },
}
/// Own the continuations and keep their active opcode origins indexed together.
/// The index is passive metadata; all runtime values live in the owning frames.
#[derive(Default)]
pub(crate) struct PrivateLoopStack {
    pub(crate) frames: Vec<PrivateLoop>,
    pub(crate) origins: HashSet<usize>,
}
impl PrivateLoopStack {
    fn check_entry(&self, origin: usize) -> Result<(), String> {
        if self.origins.contains(&origin) {
            return Err(format!(
                "private bytecode recursive loop target cycle at {origin}"
            ));
        }
        Ok(())
    }

    fn push(&mut self, frame: PrivateLoop) -> Result<(), String> {
        self.check_entry(frame.origin)?;
        // Both stores grow fallibly before either publishes an entry. A failed
        // reserve changes capacities only and leaves the index synchronized.
        self.frames
            .try_reserve(1)
            .map_err(|_| "cannot grow private bytecode loop continuations")?;
        self.origins
            .try_reserve(1)
            .map_err(|_| "cannot grow private bytecode loop origin index")?;
        self.origins.insert(frame.origin);
        self.frames.push(frame);
        Ok(())
    }

    fn pop(&mut self) -> Result<Option<PrivateLoop>, String> {
        let Some(frame) = self.frames.pop() else {
            return Ok(None);
        };
        if !self.origins.remove(&frame.origin) {
            return Err("invalid private bytecode loop origin index".into());
        }
        Ok(Some(frame))
    }
}

pub(crate) fn next_for_iteration(
    frame: &mut PrivateLoop,
    stack: &mut Vec<Sexp<'static>>,
    env: &Sexp<'static>,
    execution: &PrivateExecution<'_>,
) -> Result<Option<usize>, String> {
    let PrivateLoopKind::For(state) = &mut frame.kind else {
        return Err("invalid private for-loop continuation".into());
    };
    let PrivateFor {
        variable,
        sequence,
        body,
        length,
        next,
    } = state.as_mut();
    if *next >= *length {
        return Ok(None);
    }
    execution.require_live()?;
    let index = i64::try_from(*next).map_err(|_| "private for-loop index overflow")?;
    let value = match sequence.typeof_() {
        SEXPTYPE::INTSXP => make_int(
            execution,
            sequence.try_integer_elt(index).map_err(|e| e.to_string())?,
        )?,
        SEXPTYPE::REALSXP => make_real(
            execution,
            sequence.try_real_elt(index).map_err(|e| e.to_string())?,
        )?,
        _ => make_int(
            execution,
            c_int::try_from(*next).map_err(|_| "private for-loop scalar index overflow")?,
        )?,
    };
    execution.require_live()?;
    execution.native(|_| unsafe {
        crate::sexp::envir::defineVar(variable.as_raw(), value.as_raw(), env.as_raw());
    })?;
    execution.require_live()?;
    push_operand(stack, value)?;
    *next += 1;
    Ok(Some(*body))
}

/// Execute nested private loops with owning Rust continuations. The segment
/// interpreter returns before another segment runs, keeping host stack usage
/// constant even for deeply nested valid instruction graphs.
pub(crate) fn eval_bytecode_loop(
    bytecode: &[c_int],
    pc: &mut usize,
    stack: &mut Vec<Sexp<'static>>,
    constants: &[Sexp<'static>],
    env: Sexp<'static>,
    execution: &PrivateExecution<'_>,
) -> Result<(Sexp<'static>, ControlFlow), String> {
    let mut frames = PrivateLoopStack::default();
    let mut current_pc = *pc;
    let mut completed: Option<(Sexp<'static>, ControlFlow)> = None;
    loop {
        execution.require_live()?;
        if let Some((value, control)) = completed.take() {
            let Some(mut frame) = frames.pop()? else {
                *pc = current_pc;
                execution.require_live()?;
                return Ok((value, control));
            };
            match &mut frame.kind {
                PrivateLoopKind::For(_) => {
                    if control == ControlFlow::Break {
                        current_pc = frame.end;
                        completed = Some((execution.false_value.clone(), ControlFlow::Normal));
                    } else if let Some(body) =
                        next_for_iteration(&mut frame, stack, &env, execution)?
                    {
                        current_pc = body;
                        frames.push(frame)?;
                    } else {
                        current_pc = frame.end;
                    }
                }
                PrivateLoopKind::While {
                    condition,
                    body,
                    testing,
                } => {
                    if *testing {
                        if control != ControlFlow::Normal {
                            completed = Some((value, control));
                        } else if !scalar_bool_or_false(&value, "while condition")? {
                            current_pc = frame.end;
                            completed = Some((execution.false_value.clone(), ControlFlow::Normal));
                        } else {
                            *testing = false;
                            current_pc = *body;
                            frames.push(frame)?;
                        }
                    } else if control == ControlFlow::Break {
                        current_pc = frame.end;
                        completed = Some((value, ControlFlow::Normal));
                    } else {
                        *testing = true;
                        current_pc = *condition;
                        frames.push(frame)?;
                    }
                }
                PrivateLoopKind::Repeat { body } => {
                    if control == ControlFlow::Break {
                        current_pc = frame.end;
                        completed = Some((value, ControlFlow::Normal));
                    } else {
                        current_pc = *body;
                        frames.push(frame)?;
                    }
                }
            }
            continue;
        }
        match eval_bytecode_segment(
            bytecode,
            &mut current_pc,
            stack,
            constants,
            env.clone(),
            execution,
        )? {
            PrivateStep::Complete(value, control) => completed = Some((value, control)),
            PrivateStep::Loop(mut frame) => {
                // Reject recursive targets before iteration creates an operand
                // or enters any allocation callback.
                frames.check_entry(frame.origin)?;
                let entry = match &frame.kind {
                    PrivateLoopKind::For(_) => {
                        next_for_iteration(&mut frame, stack, &env, execution)?
                    }
                    PrivateLoopKind::While { condition, .. } => Some(*condition),
                    PrivateLoopKind::Repeat { body } => Some(*body),
                };
                if let Some(entry) = entry {
                    current_pc = entry;
                    frames.push(frame)?;
                } else {
                    current_pc = frame.end;
                }
            }
        }
    }
}

pub(crate) fn eval_bytecode_segment(
    bytecode: &[c_int],
    pc: &mut usize,
    stack: &mut Vec<Sexp<'static>>,
    constants: &[Sexp<'static>],
    env: Sexp<'static>,
    execution: &PrivateExecution<'_>,
) -> Result<PrivateStep, String> {
    while *pc < bytecode.len() {
        execution.require_live()?;
        let opcode_pc = *pc;
        let opcode = bytecode[*pc] as c_int;
        *pc += 1;

        match opcode {
            BCreturn => {
                let val = stack
                    .pop()
                    .ok_or_else(|| "empty stack on return".to_string())?;
                execution.require_live()?;
                return Ok(PrivateStep::Complete(val, ControlFlow::Normal));
            }
            BCgvar | BCsvar => {
                let idx = read_operand_index(bytecode, pc, "variable")?;
                let sym = get_constant(constants, idx)?;
                if sym.typeof_() != SEXPTYPE::SYMSXP {
                    return Err("private variable constant is not a symbol".into());
                }
                let val = execution
                    .native(|_| unsafe { crate::eval::eval::find_var_result(sym, env.clone()) })??
                    .ok_or_else(|| "variable not found".to_string())?;
                execution.require_live()?;
                push_operand(stack, val.into_owned().map_err(|e| e.to_string())?)?;
            }
            BCint | BCreal | BCstring => {
                let idx = read_operand_index(bytecode, pc, "constant")?;
                let val = get_constant(constants, idx)?;
                push_operand(stack, val.into_owned().map_err(|e| e.to_string())?)?;
            }
            BCtrue => {
                push_operand(stack, execution.true_value.clone())?;
            }
            BCfalse => {
                push_operand(stack, execution.false_value.clone())?;
            }
            BCnil => {
                push_operand(stack, execution.nil.clone())?;
            }
            BCdot => {
                return Err("'...' used in incorrect context".to_string());
            }
            BCnot => {
                let val = stack
                    .pop()
                    .ok_or_else(|| "empty stack on not".to_string())?;
                let v = if scalar_bool_or_false(&val, "bytecode not operand")? {
                    1
                } else {
                    0
                };
                push_operand(stack, make_lgl(execution, if v != 0 { 0 } else { 1 })?)?;
            }
            BCadd => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on add".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on add".to_string())?;
                push_operand(
                    stack,
                    apply_binary_op(
                        execution,
                        a,
                        b,
                        |x, y| x + y,
                        |x, y| x.checked_add(y).unwrap_or(c_int::MIN),
                    )?,
                )?;
            }
            BCsub => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on sub".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on sub".to_string())?;
                push_operand(
                    stack,
                    apply_binary_op(
                        execution,
                        a,
                        b,
                        |x, y| x - y,
                        |x, y| x.checked_sub(y).unwrap_or(c_int::MIN),
                    )?,
                )?;
            }
            BCmul => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mul".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mul".to_string())?;
                push_operand(
                    stack,
                    apply_binary_op(
                        execution,
                        a,
                        b,
                        |x, y| x * y,
                        |x, y| x.checked_mul(y).unwrap_or(c_int::MIN),
                    )?,
                )?;
            }
            BCdiv => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on div".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on div".to_string())?;
                push_operand(
                    stack,
                    make_real(
                        execution,
                        scalar_f64_or_zero(&a, "left division operand")?
                            / scalar_f64_or_zero(&b, "right division operand")?,
                    )?,
                )?;
            }
            BCmod => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mod".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mod".to_string())?;
                if a.clone().typeof_() == SEXPTYPE::REALSXP
                    && b.clone().typeof_() == SEXPTYPE::REALSXP
                {
                    push_operand(
                        stack,
                        make_real(
                            execution,
                            scalar_real(&a, "left real modulo operand")?
                                % scalar_real(&b, "right real modulo operand")?,
                        )?,
                    )?;
                } else if a.clone().typeof_() == SEXPTYPE::INTSXP
                    && b.clone().typeof_() == SEXPTYPE::INTSXP
                {
                    let bv = scalar_int(&b, "right integer modulo operand")?;
                    if bv != 0 {
                        push_operand(
                            stack,
                            make_int(
                                execution,
                                scalar_int(&a, "left integer modulo operand")?
                                    .checked_rem(bv)
                                    .unwrap_or(c_int::MIN),
                            )?,
                        )?;
                    } else {
                        push_operand(stack, make_real(execution, f64::NAN)?)?;
                    }
                } else {
                    push_operand(
                        stack,
                        make_real(
                            execution,
                            scalar_f64_or_zero(&a, "left modulo operand")?
                                % scalar_f64_or_zero(&b, "right modulo operand")?,
                        )?,
                    )?;
                }
            }
            BCpow => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on pow".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on pow".to_string())?;
                push_operand(
                    stack,
                    make_real(
                        execution,
                        scalar_f64_or_zero(&a, "left power operand")?
                            .powf(scalar_f64_or_zero(&b, "right power operand")?),
                    )?,
                )?;
            }
            BCeq => {
                let b = stack.pop().ok_or_else(|| "empty stack on eq".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on eq".to_string())?;
                push_operand(stack, apply_comparison(execution, a, b, |x, y| x == y)?)?;
            }
            BCne => {
                let b = stack.pop().ok_or_else(|| "empty stack on ne".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on ne".to_string())?;
                push_operand(stack, apply_comparison(execution, a, b, |x, y| x != y)?)?;
            }
            BClt => {
                let b = stack.pop().ok_or_else(|| "empty stack on lt".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on lt".to_string())?;
                push_operand(stack, apply_comparison(execution, a, b, |x, y| x < y)?)?;
            }
            BCle => {
                let b = stack.pop().ok_or_else(|| "empty stack on le".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on le".to_string())?;
                push_operand(stack, apply_comparison(execution, a, b, |x, y| x <= y)?)?;
            }
            BCgt => {
                let b = stack.pop().ok_or_else(|| "empty stack on gt".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on gt".to_string())?;
                push_operand(stack, apply_comparison(execution, a, b, |x, y| x > y)?)?;
            }
            BCge => {
                let b = stack.pop().ok_or_else(|| "empty stack on ge".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on ge".to_string())?;
                push_operand(stack, apply_comparison(execution, a, b, |x, y| x >= y)?)?;
            }
            BCand => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on and".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on and".to_string())?;
                push_operand(
                    stack,
                    make_lgl(
                        execution,
                        if scalar_bool_or_false(&a, "left and operand")?
                            && scalar_bool_or_false(&b, "right and operand")?
                        {
                            1
                        } else {
                            0
                        },
                    )?,
                )?;
            }
            BCor => {
                let b = stack.pop().ok_or_else(|| "empty stack on or".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on or".to_string())?;
                push_operand(
                    stack,
                    make_lgl(
                        execution,
                        if scalar_bool_or_false(&a, "left or operand")?
                            || scalar_bool_or_false(&b, "right or operand")?
                        {
                            1
                        } else {
                            0
                        },
                    )?,
                )?;
            }
            BCcall => {
                let idx = read_operand_index(bytecode, pc, "call function")?;
                let nargs = read_operand_index(bytecode, pc, "call argument count")?;

                let args_vec = pop_arguments(stack, nargs)?;
                let fun = get_constant(constants, idx)?;

                if fun.clone().typeof_() == SEXPTYPE::CLOSXP {
                    let arg_list = argument_list(execution, &args_vec)?;
                    let call = language_call(execution, &fun, &arg_list)?;
                    let result = execution
                        .native(|_| unsafe {
                            crate::eval::eval::eval_lang_safe(call.clone(), env.clone())
                        })?
                        .map_err(|e| format!("closure call failed: {e}"))?;
                    execution.require_live()?;
                    push_operand(stack, result.into_owned().map_err(|e| e.to_string())?)?;
                } else {
                    push_operand(stack, fun)?;
                }
            }
            BCpush => {
                let idx = read_operand_index(bytecode, pc, "push")?;
                push_operand(stack, get_constant(constants, idx)?)?;
            }
            BCpop => {
                stack.pop().ok_or("empty stack on pop")?;
            }
            BCdup => {
                let top = stack.last().ok_or("empty stack on dup")?.clone();
                push_operand(stack, top)?;
            }
            BCprint => {
                if let Some(top) = stack.last() {
                    let output = if top.clone().typeof_() == SEXPTYPE::EXPRSXP {
                        format_expression_vector(top, execution)?
                    } else {
                        let type_name = match top.clone().typeof_() {
                            SEXPTYPE::NILSXP => "NULL",
                            SEXPTYPE::INTSXP => "integer",
                            SEXPTYPE::REALSXP => "double",
                            SEXPTYPE::LGLSXP => "logical",
                            SEXPTYPE::STRSXP => "character",
                            SEXPTYPE::VECSXP => "list",
                            SEXPTYPE::RAWSXP => "raw",
                            SEXPTYPE::CPLXSXP => "complex",
                            SEXPTYPE::SYMSXP => "symbol",
                            SEXPTYPE::CLOSXP => "closure",
                            SEXPTYPE::ENVSXP => "environment",
                            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => "pairlist",
                            SEXPTYPE::CHARSXP => "charsxp",
                            SEXPTYPE::PROMSXP => "promise",
                            SEXPTYPE::DOTSXP => "...",
                            SEXPTYPE::SPECIALSXP => "special",
                            SEXPTYPE::BUILTINSXP => "builtin",
                            SEXPTYPE::EXTPTRSXP => "externalptr",
                            SEXPTYPE::WEAKREFSXP => "weakref",
                            SEXPTYPE::BCODESXP => "bytecode",
                            SEXPTYPE::OBJSXP => "object",
                            _ => "unknown",
                        };
                        format!("[{}; length={}]", type_name, top.clone().len())
                    };
                    if crate::sexp::output::is_capturing() {
                        crate::sexp::output::capture_stdout(&output);
                        crate::sexp::output::capture_stdout("\n");
                    } else {
                        println!("{}", output);
                    }
                }
            }
            BCbegin => {}
            BCif => {
                let cond = stack.pop().ok_or_else(|| "empty stack on if".to_string())?;
                let true_offset = read_jump_target(bytecode, pc, "if true")?;
                let false_offset = read_jump_target(bytecode, pc, "if false")?;
                if scalar_bool_or_false(&cond, "if condition")? {
                    *pc = true_offset;
                } else {
                    *pc = false_offset;
                }
            }
            BCjump => {
                let offset = read_jump_target(bytecode, pc, "jump")?;
                *pc = offset;
            }
            BCfjmp => {
                let cond = stack
                    .pop()
                    .ok_or_else(|| "empty stack on fjmp".to_string())?;
                let offset = read_jump_target(bytecode, pc, "false jump")?;
                if !scalar_bool_or_false(&cond, "false jump condition")? {
                    *pc = offset;
                }
            }
            BCtjmp => {
                let cond = stack
                    .pop()
                    .ok_or_else(|| "empty stack on tjmp".to_string())?;
                let offset = read_jump_target(bytecode, pc, "true jump")?;
                if scalar_bool_or_false(&cond, "true jump condition")? {
                    *pc = offset;
                }
            }
            BCfor => {
                let var_idx = read_operand_index(bytecode, pc, "for variable")?;
                let seq_idx = read_operand_index(bytecode, pc, "for sequence")?;
                let body = read_jump_target(bytecode, pc, "for body")?;
                let end = read_jump_target(bytecode, pc, "for end")?;
                let variable = get_constant(constants, var_idx)?;
                if variable.typeof_() != SEXPTYPE::SYMSXP {
                    return Err("private for-loop variable is not a symbol".into());
                }
                let sequence = get_constant(constants, seq_idx)?;
                let length = usize::try_from(sequence.len())
                    .map_err(|_| "invalid private for-loop length")?;
                return Ok(PrivateStep::Loop(PrivateLoop {
                    origin: opcode_pc,
                    end,
                    kind: PrivateLoopKind::For(Box::new(PrivateFor {
                        variable,
                        sequence,
                        body,
                        length,
                        next: 0,
                    })),
                }));
            }
            BCwhile => {
                let condition = read_jump_target(bytecode, pc, "while condition")?;
                let body = read_jump_target(bytecode, pc, "while body")?;
                let end = read_jump_target(bytecode, pc, "while end")?;
                return Ok(PrivateStep::Loop(PrivateLoop {
                    origin: opcode_pc,
                    end,
                    kind: PrivateLoopKind::While {
                        condition,
                        body,
                        testing: true,
                    },
                }));
            }
            BCrepeat => {
                let body = read_jump_target(bytecode, pc, "repeat body")?;
                let end = read_jump_target(bytecode, pc, "repeat end")?;
                return Ok(PrivateStep::Loop(PrivateLoop {
                    origin: opcode_pc,
                    end,
                    kind: PrivateLoopKind::Repeat { body },
                }));
            }
            BCbreak => {
                execution.require_live()?;
                return Ok(PrivateStep::Complete(
                    make_lgl(execution, 0)?,
                    ControlFlow::Break,
                ));
            }
            BCnext => {
                execution.require_live()?;
                return Ok(PrivateStep::Complete(
                    make_lgl(execution, 0)?,
                    ControlFlow::Next,
                ));
            }
            BCspecial => {
                let idx = read_operand_index(bytecode, pc, "special function")?;
                let nargs = read_operand_index(bytecode, pc, "special argument count")?;

                let args_vec = pop_arguments(stack, nargs)?;
                let fun = get_constant(constants, idx)?;

                if fun.clone().typeof_() == SEXPTYPE::SPECIALSXP
                    || fun.clone().typeof_() == SEXPTYPE::BUILTINSXP
                {
                    let arg_list = argument_list(execution, &args_vec)?;

                    let call_sexp = language_call(execution, &fun, &arg_list)?;

                    let result = execution
                        .native(|_| unsafe {
                            crate::eval::eval::eval_lang_safe(call_sexp.clone(), env.clone())
                        })?
                        .map_err(|e| format!("special call failed: {e}"))?;
                    execution.require_live()?;
                    push_operand(stack, result.into_owned().map_err(|e| e.to_string())?)?;
                } else {
                    push_operand(stack, fun)?;
                }
            }
            BCbuiltin => {
                let idx = read_operand_index(bytecode, pc, "builtin function")?;
                let nargs = read_operand_index(bytecode, pc, "builtin argument count")?;

                let args_vec = pop_arguments(stack, nargs)?;
                let fun = get_constant(constants, idx)?;

                if fun.clone().typeof_() == SEXPTYPE::BUILTINSXP
                    || fun.clone().typeof_() == SEXPTYPE::SPECIALSXP
                {
                    let arg_list = argument_list(execution, &args_vec)?;

                    let call_sexp = language_call(execution, &fun, &arg_list)?;

                    let result = execution
                        .native(|_| unsafe {
                            crate::eval::eval::eval_lang_safe(call_sexp.clone(), env.clone())
                        })?
                        .map_err(|e| format!("builtin call failed: {e}"))?;
                    execution.require_live()?;
                    push_operand(stack, result.into_owned().map_err(|e| e.to_string())?)?;
                } else {
                    push_operand(stack, fun)?;
                }
            }
            BCneg => {
                let val = stack
                    .pop()
                    .ok_or_else(|| "empty stack on neg".to_string())?;
                if val.clone().typeof_() == SEXPTYPE::REALSXP {
                    push_operand(
                        stack,
                        make_real(execution, -scalar_real(&val, "real negation operand")?)?,
                    )?;
                } else if val.clone().typeof_() == SEXPTYPE::INTSXP {
                    push_operand(
                        stack,
                        make_int(
                            execution,
                            scalar_int(&val, "integer negation operand")?
                                .checked_neg()
                                .unwrap_or(c_int::MIN),
                        )?,
                    )?;
                } else {
                    push_operand(stack, val.into_owned().map_err(|e| e.to_string())?)?;
                }
            }
            BCclosure => {
                let idx = read_operand_index(bytecode, pc, "closure")?;
                push_operand(stack, get_constant(constants, idx)?)?;
            }
            _ => {
                return Err(format!("unknown bytecode opcode: {opcode}"));
            }
        }
    }

    let val = stack
        .pop()
        .ok_or_else(|| "empty stack at end of bytecode".to_string())?;
    execution.require_live()?;
    Ok(PrivateStep::Complete(val, ControlFlow::Normal))
}

pub(crate) fn snapshot_constants(
    pool: Option<Sexp<'static>>,
) -> Result<Vec<Sexp<'static>>, String> {
    let Some(pool) = pool else {
        return Ok(Vec::new());
    };
    if pool.typeof_() == SEXPTYPE::NILSXP {
        return Ok(Vec::new());
    }
    if !matches!(pool.typeof_(), SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP) {
        return Err("private bytecode constant pool is not a vector".into());
    }
    let count = usize::try_from(pool.len()).map_err(|_| "invalid private constant pool length")?;
    let mut constants = Vec::new();
    constants
        .try_reserve_exact(count)
        .map_err(|_| "cannot reserve private bytecode constants")?;
    for index in 0..count {
        constants.push(
            pool.try_vector_elt(index as i64)
                .and_then(Sexp::into_owned)
                .map_err(|e| sexp_err("private bytecode constant", e))?,
        );
    }
    Ok(constants)
}
pub(crate) fn get_constant(
    constants: &[Sexp<'static>],
    idx: usize,
) -> Result<Sexp<'static>, String> {
    constants
        .get(idx)
        .cloned()
        .ok_or_else(|| format!("constant index {idx} is out of bounds"))
}
