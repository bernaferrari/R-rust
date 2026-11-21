//! bytecode tests (part 2/2) — extracted verbatim from eval/bytecode.rs

use super::*;

#[test]
fn gnu_dup_validator_accepts_balanced_stacks_above_legacy_cutoff() {
    // A valid straight-line stream has a statically bounded depth, even
    // when it exceeds the former arbitrary 64-entry adapter limit.
    let mut words = vec![GNU_BC_MAX_VERSION, GNU_OP_LDNULL];
    words.extend(std::iter::repeat_n(GNU_OP_DUP, 256));
    words.extend(std::iter::repeat_n(GNU_OP_POP, 256));
    words.push(GNU_OP_RETURN);
    assert!(validate_gnu_adapter_stream(&words, 0).unwrap());
}

#[test]
fn gnu_short_circuit_validator_accepts_and_or_cfg() {
    // compiler:::disassemble(cmpfun(function(x, y) x && y))
    let and_stream = [12, 20, 1, 88, 0, 10, 20, 2, 89, 0, 1];
    assert_eq!(validate_gnu_adapter_stream(&and_stream, 3), Ok(true));
    // compiler:::disassemble(cmpfun(function(x, y) x || y))
    let or_stream = [12, 20, 1, 90, 0, 10, 20, 2, 91, 0, 1];
    assert_eq!(validate_gnu_adapter_stream(&or_stream, 3), Ok(true));
    let and_const = [12, 20, 1, 88, 0, 9, 18, 89, 0, 1];
    assert_eq!(validate_gnu_adapter_stream(&and_const, 2), Ok(true));

    let mut non_boundary = and_stream;
    non_boundary[5] = 9;
    assert!(
        validate_gnu_adapter_stream(&non_boundary, 3)
            .unwrap_err()
            .contains("instruction boundary")
    );
    assert!(validate_gnu_adapter_stream(&[12, 88, 0, 4, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 18, 89, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 1, 88, 3, 10, 20, 2, 89, 0, 1], 3).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 1, 90, 0, 99, 20, 2, 91, 0, 1], 3).is_err());
}

#[test]
fn gnu_vector_logic_validator_accepts_and_or_not() {
    // compiler:::disassemble(cmpfun(function(x, y) x & y))
    let and_stream = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_GETVAR,
        2,
        GNU_OP_AND,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&and_stream, 3), Ok(true));
    // compiler:::disassemble(cmpfun(function(x, y) x | y))
    let or_stream = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_GETVAR,
        2,
        GNU_OP_OR,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&or_stream, 3), Ok(true));
    // compiler:::disassemble(cmpfun(function(x) !x))
    let not_stream = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_NOT,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&not_stream, 2), Ok(true));

    let underflow = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_AND,
        0,
        GNU_OP_RETURN,
    ];
    assert!(validate_gnu_adapter_stream(&underflow, 2).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 59, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 1, 20, 2, 57, 3, 1], 3).is_err());
}

#[test]
fn gnu_istype_validator_accepts_isnull_family() {
    // compiler:::disassemble(cmpfun(function(x) is.null(x), options=list(optimize=3)))
    let null_stream = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_ISNULL,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&null_stream, 2), Ok(true));
    let integer_stream = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_ISINTEGER,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&integer_stream, 2), Ok(true));
    let object_stream = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_ISOBJECT,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&object_stream, 2), Ok(true));
    // ISNUMERIC is emitted only by older/hand-built streams; still accept it.
    let numeric_stream = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_ISNUMERIC,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&numeric_stream, 2), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 75, 1], 1).is_err());
}

#[test]
fn gnu_dollar_validator_accepts_get_and_assign_streams() {
    // compiler:::disassemble(cmpfun(function(x) x$a, options=list(optimize=3)))
    let dollar = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_DOLLAR,
        0,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&dollar, 3), Ok(true));
    // compiler:::disassemble(cmpfun(function(x,v){x$a<-v;x}, options=list(optimize=3)))
    let dollargets = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_DOLLARGETS,
        4,
        5,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&dollargets, 7), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 73, 0, 1, 1], 2).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 73, 2, 1, 1], 2).is_err());
    // GNU mkCode executes GETVAR; STARTASSIGN; RETURN and returns the
    // original RHS. No ENDASSIGN is required when returning immediately.
    assert_eq!(
        validate_gnu_adapter_stream(&[12, 20, 0, 61, 1, 1], 2),
        Ok(true)
    );
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 74, 0, 1, 1], 2).is_err());
}

#[test]
fn gnu_subset_n_validator_accepts_vector_get_and_assign_streams() {
    // compiler:::disassemble(cmpfun(function(x,i) x[i], options=list(optimize=3)))
    let subset = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTSUBSET_N,
        0,
        10,
        GNU_OP_GETVAR_MISSOK,
        2,
        GNU_OP_VECSUBSET,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&subset, 3), Ok(true));
    // compiler:::disassemble(cmpfun(function(x) x[1L], options=list(optimize=3)))
    let const_index = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTSUBSET_N,
        0,
        10,
        GNU_OP_LDCONST,
        2,
        GNU_OP_VECSUBSET,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&const_index, 3), Ok(true));
    // compiler:::disassemble(cmpfun(function(x,i) x[[i]], options=list(optimize=3)))
    let subset2 = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTSUBSET2_N,
        0,
        10,
        GNU_OP_GETVAR_MISSOK,
        2,
        GNU_OP_VECSUBSET2,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&subset2, 3), Ok(true));
    // compiler:::disassemble(cmpfun(function(x,i,v){x[i]<-v;x}, options=list(optimize=3)))
    let assign = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_STARTSUBASSIGN_N,
        4,
        12,
        GNU_OP_GETVAR_MISSOK,
        6,
        GNU_OP_VECSUBASSIGN,
        4,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&assign, 7), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 104, 0, 1, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 84, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 104, 2, 6, 1], 1).is_err());
}

#[test]
fn gnu_dfltsubset_validator_accepts_empty_index_stream() {
    // compiler:::disassemble(cmpfun(function(x) x[], options=list(optimize=3)))
    let empty = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTSUBSET,
        0,
        8,
        GNU_OP_DOMISSING,
        GNU_OP_DFLTSUBSET,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&empty, 2), Ok(true));
    // compiler:::disassemble(cmpfun(function(x) x[,], options=list(optimize=3)))
    let matrix_missing = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTSUBSET,
        0,
        9,
        GNU_OP_DOMISSING,
        GNU_OP_DOMISSING,
        GNU_OP_DFLTSUBSET,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&matrix_missing, 2), Ok(true));
    // compiler:::disassemble(cmpfun(function(x,i) x[i,], options=list(optimize=3)))
    let row_missing = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTSUBSET,
        0,
        11,
        GNU_OP_GETVAR_MISSOK,
        2,
        GNU_OP_PUSHARG,
        GNU_OP_DOMISSING,
        GNU_OP_DFLTSUBSET,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&row_missing, 3), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 63, 0, 4, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 30, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 64, 1], 1).is_err());
}

#[test]
fn gnu_dfltsubassign_validator_accepts_missing_index_streams() {
    // compiler:::disassemble(cmpfun(function(x,v){x[]<-v;x}, options=list(optimize=3)))
    let empty = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_STARTSUBASSIGN,
        4,
        10,
        GNU_OP_DOMISSING,
        GNU_OP_DFLTSUBASSIGN,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&empty, 7), Ok(true));
    // compiler:::disassemble(cmpfun(function(x,v){x[,]<-v;x}, options=list(optimize=3)))
    let matrix_missing = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_STARTSUBASSIGN,
        4,
        11,
        GNU_OP_DOMISSING,
        GNU_OP_DOMISSING,
        GNU_OP_DFLTSUBASSIGN,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&matrix_missing, 7), Ok(true));
    // compiler:::disassemble(cmpfun(function(x,i,v){x[i,]<-v;x}, options=list(optimize=3)))
    let row_missing = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_STARTSUBASSIGN,
        4,
        13,
        GNU_OP_GETVAR_MISSOK,
        6,
        GNU_OP_PUSHARG,
        GNU_OP_DOMISSING,
        GNU_OP_DFLTSUBASSIGN,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&row_missing, 7), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 65, 0, 4, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 61, 1, 66, 1], 2).is_err());
}

#[test]
fn gnu_dfltsubassign2_validator_accepts_missing_index_stream() {
    // compiler:::disassemble(cmpfun(function(x,v){x[[]]<-v;x}, options=list(optimize=3)))
    let empty = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_STARTSUBASSIGN2,
        4,
        10,
        GNU_OP_DOMISSING,
        GNU_OP_DFLTSUBASSIGN2,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&empty, 7), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 71, 0, 4, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 61, 1, 72, 1], 2).is_err());
}

#[test]
fn gnu_setter_call_validator_accepts_names_and_attr_streams() {
    // compiler:::disassemble(cmpfun(function(x,v){names(x)<-v;x}, options=list(optimize=3)))
    let names = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_GETFUN,
        4,
        GNU_OP_PUSHNULLARG,
        GNU_OP_SETTER_CALL,
        6,
        1,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&names, 7), Ok(true));
    // compiler:::disassemble(cmpfun(function(x,v){attr(x,"a")<-v;x}, options=list(optimize=3)))
    let attr = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_GETFUN,
        4,
        GNU_OP_PUSHNULLARG,
        GNU_OP_PUSHCONSTARG,
        6,
        GNU_OP_SETTER_CALL,
        7,
        1,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&attr, 8), Ok(true));
    assert!(
        validate_gnu_adapter_stream(&[12, 98, 0, 1, 1], 2)
            .unwrap_err()
            .contains("no active GETFUN call")
    );
    assert!(
        validate_gnu_adapter_stream(&[12, 23, 0, 98, 0, 1, 1], 2)
            .unwrap_err()
            .contains("empty stack")
    );
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 61, 1, 23, 2, 35, 98, 3, 1, 1], 3).is_err());
}

#[test]
fn gnu_getter_call_validator_accepts_names_subset_assign_stream() {
    // compiler:::disassemble(cmpfun(function(x,v){names(x)[1]<-v;x}, options=list(optimize=3)))
    let names = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_GETFUN,
        4,
        GNU_OP_PUSHNULLARG,
        GNU_OP_GETTER_CALL,
        6,
        GNU_OP_SWAP,
        GNU_OP_STARTSUBASSIGN_N,
        7,
        18,
        GNU_OP_LDCONST,
        9,
        GNU_OP_VECSUBASSIGN,
        7,
        GNU_OP_GETFUN,
        10,
        GNU_OP_PUSHNULLARG,
        GNU_OP_SETTER_CALL,
        12,
        13,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&names, 15), Ok(true));
    // compiler:::disassemble(cmpfun(function(x,v){attr(x,"a")[1]<-v;x}, options=list(optimize=3)))
    let attr = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_GETFUN,
        4,
        GNU_OP_PUSHNULLARG,
        GNU_OP_PUSHCONSTARG,
        6,
        GNU_OP_GETTER_CALL,
        7,
        GNU_OP_SWAP,
        GNU_OP_STARTSUBASSIGN_N,
        8,
        20,
        GNU_OP_LDCONST,
        10,
        GNU_OP_VECSUBASSIGN,
        8,
        GNU_OP_GETFUN,
        11,
        GNU_OP_PUSHNULLARG,
        GNU_OP_PUSHCONSTARG,
        6,
        GNU_OP_SETTER_CALL,
        13,
        14,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&attr, 16), Ok(true));
    assert!(
        validate_gnu_adapter_stream(&[12, 99, 0, 1], 2)
            .unwrap_err()
            .contains("no active GETFUN call")
    );
    assert!(
        validate_gnu_adapter_stream(&[12, 23, 0, 99, 0, 1], 2)
            .unwrap_err()
            .contains("empty stack")
    );
    assert!(
        validate_gnu_adapter_stream(&[12, 100, 1], 1)
            .unwrap_err()
            .contains("SWAP")
    );
}

#[test]
fn gnu_dfltsubset2_validator_accepts_empty_index_stream() {
    // compiler:::disassemble(cmpfun(function(x) x[[]], options=list(optimize=3)))
    let empty = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTSUBSET2,
        0,
        8,
        GNU_OP_DOMISSING,
        GNU_OP_DFLTSUBSET2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&empty, 2), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 69, 0, 4, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 70, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 70, 1], 1).is_err());
}

#[test]
fn gnu_matsubset_validator_accepts_matrix_and_array_streams() {
    // compiler:::disassemble(cmpfun(function(x) x[1L,2L], options=list(optimize=3)))
    let mat_const = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTSUBSET_N,
        0,
        12,
        GNU_OP_LDCONST,
        2,
        GNU_OP_LDCONST,
        3,
        GNU_OP_MATSUBSET,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&mat_const, 4), Ok(true));
    // compiler:::disassemble(cmpfun(function(x,i,j) x[i,j], options=list(optimize=3)))
    let mat_vars = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTSUBSET_N,
        0,
        12,
        GNU_OP_GETVAR_MISSOK,
        2,
        GNU_OP_GETVAR_MISSOK,
        3,
        GNU_OP_MATSUBSET,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&mat_vars, 4), Ok(true));
    // compiler:::disassemble(cmpfun(function(x) x[1L,2L,3L], options=list(optimize=3)))
    let subset_n = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTSUBSET_N,
        0,
        15,
        GNU_OP_LDCONST,
        2,
        GNU_OP_LDCONST,
        3,
        GNU_OP_LDCONST,
        4,
        GNU_OP_SUBSET_N,
        0,
        3,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&subset_n, 5), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 85, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 85, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 16, 1, 16, 1, 85, 2, 1], 2).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 112, 0, 3, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 112, 0, -1, 1], 1).is_err());
}

#[test]
fn gnu_matsubset2_validator_accepts_double_index_stream() {
    // compiler:::disassemble(cmpfun(function(x) x[[1L,1L]], options=list(optimize=3)))
    let mat2 = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTSUBSET2_N,
        0,
        12,
        GNU_OP_LDCONST,
        2,
        GNU_OP_LDCONST,
        2,
        GNU_OP_MATSUBSET2,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&mat2, 4), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 107, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 107, 0, 1], 1).is_err());
}

#[test]
fn gnu_subset2_n_validator_accepts_triple_index_stream() {
    // compiler:::disassemble(cmpfun(function(x) x[[1L,2L,3L]], options=list(optimize=3)))
    let subset2_n = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTSUBSET2_N,
        0,
        15,
        GNU_OP_LDCONST,
        2,
        GNU_OP_LDCONST,
        3,
        GNU_OP_LDCONST,
        4,
        GNU_OP_SUBSET2_N,
        0,
        3,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&subset2_n, 5), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 113, 0, 3, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 113, 0, -1, 1], 1).is_err());
}

#[test]
fn gnu_matsubassign_validator_accepts_matrix_streams() {
    // compiler:::disassemble(cmpfun(function(x,v){x[1L,2L]<-v;x}, options=list(optimize=3)))
    let mat_const = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_STARTSUBASSIGN_N,
        4,
        14,
        GNU_OP_LDCONST,
        6,
        GNU_OP_LDCONST,
        7,
        GNU_OP_MATSUBASSIGN,
        4,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&mat_const, 8), Ok(true));
    // compiler:::disassemble(cmpfun(function(x,i,j,v){x[i,j]<-v;x}, options=list(optimize=3)))
    let mat_vars = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_STARTSUBASSIGN_N,
        4,
        14,
        GNU_OP_GETVAR_MISSOK,
        6,
        GNU_OP_GETVAR_MISSOK,
        7,
        GNU_OP_MATSUBASSIGN,
        4,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&mat_vars, 8), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 87, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 87, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 16, 1, 16, 1, 87, 2, 1], 2).is_err());
}

#[test]
fn gnu_subassign_n_validator_accepts_array_streams() {
    // compiler:::disassemble(cmpfun(function(x,v){x[1L,2L,3L]<-v;x}, options=list(optimize=3)))
    let const_idx = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_STARTSUBASSIGN_N,
        4,
        17,
        GNU_OP_LDCONST,
        6,
        GNU_OP_LDCONST,
        7,
        GNU_OP_LDCONST,
        8,
        GNU_OP_SUBASSIGN_N,
        4,
        3,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&const_idx, 9), Ok(true));
    // compiler:::disassemble(cmpfun(function(x,i,j,k,v){x[i,j,k]<-v;x}, options=list(optimize=3)))
    let vars = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_STARTSUBASSIGN_N,
        4,
        17,
        GNU_OP_GETVAR_MISSOK,
        6,
        GNU_OP_GETVAR_MISSOK,
        7,
        GNU_OP_GETVAR_MISSOK,
        8,
        GNU_OP_SUBASSIGN_N,
        4,
        3,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&vars, 9), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 114, 0, 3, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 114, 0, 3, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 16, 1, 114, 0, -1, 1], 2).is_err());
}

#[test]
fn gnu_matsubassign2_validator_accepts_compiled_stream() {
    // compiler:::disassemble(cmpfun(function(x,v){x[[1L,2L]]<-v;x}, options=list(optimize=3)))
    let mat2 = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_STARTSUBASSIGN2_N,
        4,
        14,
        GNU_OP_LDCONST,
        6,
        GNU_OP_LDCONST,
        7,
        GNU_OP_MATSUBASSIGN2,
        4,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&mat2, 8), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 109, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 109, 0, 1], 1).is_err());
}

#[test]
fn gnu_subassign2_n_validator_accepts_array_stream() {
    // compiler:::disassemble(cmpfun(function(x,v){x[[1L,2L,3L]]<-v;x}, options=list(optimize=3)))
    let const_idx = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_STARTSUBASSIGN2_N,
        4,
        17,
        GNU_OP_LDCONST,
        6,
        GNU_OP_LDCONST,
        7,
        GNU_OP_LDCONST,
        8,
        GNU_OP_SUBASSIGN2_N,
        4,
        3,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&const_idx, 9), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 115, 0, 3, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 115, 0, 3, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 16, 1, 115, 0, -1, 1], 2).is_err());
}

#[test]
fn gnu_dup2nd_validator_accepts_dollar_subset_assign_stream() {
    // compiler:::disassemble(cmpfun(function(x,v){x$a[1L]<-v;x}, options=list(optimize=3)))
    let dollar = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_DUP2ND,
        GNU_OP_DOLLAR,
        4,
        5,
        GNU_OP_SWAP,
        GNU_OP_STARTSUBASSIGN_N,
        7,
        17,
        GNU_OP_LDCONST,
        9,
        GNU_OP_VECSUBASSIGN,
        7,
        GNU_OP_DOLLARGETS,
        10,
        5,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&dollar, 13), Ok(true));
    assert!(
        validate_gnu_adapter_stream(&[12, 101, 1], 1)
            .unwrap_err()
            .contains("DUP2ND")
    );
}

#[test]
fn gnu_colon_validator_accepts_compiled_streams() {
    // compiler:::disassemble(cmpfun(function(n) 1:n, options=list(optimize=3)))
    let one_to_n = [
        GNU_BC_MAX_VERSION,
        GNU_OP_LDCONST,
        1,
        GNU_OP_GETVAR,
        2,
        GNU_OP_COLON,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&one_to_n, 4), Ok(true));
    // compiler:::disassemble(cmpfun(function(a,b) a:b, options=list(optimize=3)))
    let range = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_GETVAR,
        2,
        GNU_OP_COLON,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&range, 4), Ok(true));
    assert!(
        validate_gnu_adapter_stream(&[12, 120, 0, 1], 1)
            .unwrap_err()
            .contains("requires 2")
    );
    assert!(validate_gnu_adapter_stream(&[12, 16, 0, 120, 2, 1], 2).is_err());
}

#[test]
fn gnu_seqalong_validator_accepts_compiled_stream() {
    // compiler:::disassemble(cmpfun(function(x) seq_along(x), options=list(optimize=3)))
    let seqalong = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_SEQALONG,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&seqalong, 3), Ok(true));
    assert!(
        validate_gnu_adapter_stream(&[12, 121, 0, 1], 1)
            .unwrap_err()
            .contains("empty stack")
    );
}

#[test]
fn gnu_seqlen_validator_accepts_compiled_stream() {
    // compiler:::disassemble(cmpfun(function(n) seq_len(n), options=list(optimize=3)))
    let seqlen = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_SEQLEN,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&seqlen, 3), Ok(true));
    assert!(
        validate_gnu_adapter_stream(&[12, 122, 0, 1], 1)
            .unwrap_err()
            .contains("empty stack")
    );
}

#[test]
fn gnu_log_validator_accepts_compiled_stream() {
    assert_eq!(GNU_BC_OPERAND_WIDTHS[GNU_OP_LOG as usize], 1);
    // compiler:::disassemble(cmpfun(function(x) log(x), options=list(optimize=3)))
    let log = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_LOG,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&log, 3), Ok(true));
    assert!(
        validate_gnu_adapter_stream(&[12, 116, 0, 1], 1)
            .unwrap_err()
            .contains("empty stack")
    );
}

#[test]
fn gnu_logbase_validator_accepts_compiled_stream() {
    assert_eq!(GNU_BC_OPERAND_WIDTHS[GNU_OP_LOGBASE as usize], 1);
    // compiler:::disassemble(cmpfun(function(x) log(x, 10), options=list(optimize=3)))
    let logbase = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_LDCONST,
        2,
        GNU_OP_LOGBASE,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&logbase, 4), Ok(true));
    // compiler:::disassemble(cmpfun(function(x,b) log(x,b), options=list(optimize=3)))
    let logbase_var = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_GETVAR,
        2,
        GNU_OP_LOGBASE,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&logbase_var, 4), Ok(true));
    assert!(
        validate_gnu_adapter_stream(&[12, 117, 0, 1], 1)
            .unwrap_err()
            .contains("requires 2")
    );
}

#[test]
fn gnu_math1_validator_accepts_compiled_streams() {
    assert_eq!(GNU_BC_OPERAND_WIDTHS[GNU_OP_MATH1 as usize], 2);
    // compiler:::disassemble(cmpfun(function(x) sin(x), options=list(optimize=3)))
    let sin = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_MATH1,
        0,
        6,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&sin, 3), Ok(true));
    // compiler:::disassemble(cmpfun(function(x) expm1(x), options=list(optimize=3)))
    let expm1 = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_MATH1,
        0,
        3,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&expm1, 3), Ok(true));
    assert!(
        validate_gnu_adapter_stream(&[12, 118, 0, 6, 1], 1)
            .unwrap_err()
            .contains("empty stack")
    );
    assert!(
        validate_gnu_adapter_stream(&[12, 20, 1, 118, 0, 24, 1], 3)
            .unwrap_err()
            .contains("function index")
    );
    assert!(validate_gnu_adapter_stream(&[12, 20, 1, 118, 9, 6, 1], 3).is_err());
}

#[test]
fn gnu_vecsubassign2_validator_accepts_compiled_stream() {
    // compiler:::disassemble(cmpfun(function(x,i,v){x[[i]]<-v;x}, options=list(optimize=3)))
    let vec2 = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETVAR,
        1,
        GNU_OP_STARTASSIGN,
        2,
        GNU_OP_STARTSUBASSIGN2_N,
        4,
        12,
        GNU_OP_GETVAR_MISSOK,
        6,
        GNU_OP_VECSUBASSIGN2,
        4,
        GNU_OP_ENDASSIGN,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&vec2, 8), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 108, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 108, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 111, 0, 4, 1], 1).is_err());
}

#[test]
fn gnu_assign2_validator_accepts_dollar_and_subset_streams() {
    // compiler:::disassemble(cmpfun(function() { x$a <<- 1L; x }, options=list(optimize=3)))
    let dollar = [
        GNU_BC_MAX_VERSION,
        GNU_OP_LDCONST,
        1,
        GNU_OP_STARTASSIGN2,
        2,
        GNU_OP_DOLLARGETS,
        4,
        5,
        GNU_OP_ENDASSIGN2,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&dollar, 7), Ok(true));
    // compiler:::disassemble(cmpfun(function(i) { x[i] <<- 8L; x }, options=list(optimize=3)))
    let subset = [
        GNU_BC_MAX_VERSION,
        GNU_OP_LDCONST,
        1,
        GNU_OP_STARTASSIGN2,
        2,
        GNU_OP_STARTSUBASSIGN_N,
        4,
        12,
        GNU_OP_GETVAR_MISSOK,
        6,
        GNU_OP_VECSUBASSIGN,
        4,
        GNU_OP_ENDASSIGN2,
        2,
        GNU_OP_POP,
        GNU_OP_GETVAR,
        2,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&subset, 7), Ok(true));
    assert!(validate_gnu_adapter_stream(&[12, 96, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 16, 0, 97, 1, 1], 2).is_err());
}

#[test]
fn gnu_printvalue_setloopval_dotserr_validator_accepts_streams() {
    assert_eq!(GNU_BC_OPERAND_WIDTHS[GNU_OP_PRINTVALUE as usize], 0);
    assert_eq!(GNU_BC_OPERAND_WIDTHS[GNU_OP_SETLOOPVAL as usize], 0);
    assert_eq!(GNU_BC_OPERAND_WIDTHS[GNU_OP_DOTSERR as usize], 0);
    let printvalue = [
        GNU_BC_MAX_VERSION,
        GNU_OP_LDCONST,
        1,
        GNU_OP_PRINTVALUE,
        GNU_OP_LDNULL,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&printvalue, 3), Ok(true));
    let setloopval = [
        GNU_BC_MAX_VERSION,
        GNU_OP_LDNULL,
        GNU_OP_LDCONST,
        1,
        GNU_OP_SETLOOPVAL,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&setloopval, 3), Ok(true));
    let dotserr = [GNU_BC_MAX_VERSION, GNU_OP_DOTSERR];
    assert_eq!(validate_gnu_adapter_stream(&dotserr, 1), Ok(true));
    assert!(
        validate_gnu_adapter_stream(&[12, 6, 1], 1)
            .unwrap_err()
            .contains("stack depth")
    );
    assert!(
        validate_gnu_adapter_stream(&[12, 14, 1], 1)
            .unwrap_err()
            .contains("stack depth")
    );
}

#[test]
fn gnu_getglobfun_getsymfun_dotcall_validator_accepts_streams() {
    assert_eq!(GNU_BC_OPERAND_WIDTHS[GNU_OP_GETGLOBFUN as usize], 1);
    assert_eq!(GNU_BC_OPERAND_WIDTHS[GNU_OP_GETSYMFUN as usize], 1);
    assert_eq!(GNU_BC_OPERAND_WIDTHS[GNU_OP_DOTCALL as usize], 2);
    let glob = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETGLOBFUN,
        1,
        GNU_OP_PUSHCONSTARG,
        2,
        GNU_OP_CALL,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&glob, 4), Ok(true));
    let sym = [
        GNU_BC_MAX_VERSION,
        GNU_OP_GETSYMFUN,
        1,
        GNU_OP_CALL,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&sym, 3), Ok(true));
    let dotcall = [
        GNU_BC_MAX_VERSION,
        GNU_OP_LDCONST,
        1,
        GNU_OP_DOTCALL,
        0,
        0,
        GNU_OP_RETURN,
    ];
    assert_eq!(validate_gnu_adapter_stream(&dotcall, 3), Ok(true));
    assert!(
        validate_gnu_adapter_stream(&[12, 119, 0, 1, 1], 2)
            .unwrap_err()
            .contains("requires 2")
    );
}

#[test]
fn gnu_cfg_proves_unequal_depths_and_checks_every_alternative() {
    // The first branch reaches PC9 at depth0, the second at depth1.
    // LDNULL/RETURN is valid for both: GNU discards lower operands.
    let branch = [12, 18, 3, 0, 8, 18, 2, 9, 15, 17, 1];
    assert_eq!(validate_gnu_adapter_stream(&branch, 1), Ok(true));
    // A POP at the join is invalid for the depth0 branch, even though the
    // depth1 branch is visited first by the worklist.
    let underflow = [12, 18, 3, 0, 8, 18, 2, 9, 15, 4, 17, 1];
    assert!(
        validate_gnu_adapter_stream(&underflow, 1)
            .unwrap_err()
            .contains("stack depth 0")
    );
    // Each trip around the backedge adds an operand. Stop proof exploration
    // at the state budget instead of accepting the already-seen PC.
    let growing = [12, 18, 18, 3, 0, 8, 2, 1, 1];
    assert_eq!(validate_gnu_adapter_stream(&growing, 1), Ok(false));
}

#[test]
fn gnu_cfg_executes_shadowed_stop_switch_without_source_fallback() {
    use crate::sexp::session::RSession;
    // Pinned GNU r90451 compiler::cmpfun(function(x) {
    //   v <- switch(x, a=, b=1L); v
    // }), in an environment whose stop returns99L. GNU's generated numeric
    // empty-alternative handler calls the shadowable stop and falls through
    // to LDNULL; source switch would report an error instead.
    let fixture = include_bytes!("fixtures/gnu-switch-unequal-stack.rds");
    let raw = fixture
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut session = RSession::new_without_default_packages();
    let script = format!(
        r#"
        f <- unserialize(as.raw(c({raw})))
        g <- unserialize(serialize(f,NULL))
        is.null(f(1L)) && is.null(f(99L)) && identical(f(2L),1L) &&
            identical(f("a"),1L) && is.null(g(1L))
    "#
    );
    let (result, out, err) = session.eval_code_with_output_capture(&script);
    assert_eq!(
        result
            .unwrap_or_else(|error| panic!("{error}; {out:?}; {err:?}"))
            .logical_elt(0),
        Some(1)
    );
    session.with_active(|| unsafe {
        let function = crate::sexp::envir::R_findVarInFrame(
            crate::sexp::globals::R_GlobalEnv(),
            crate::sexp::symbol::install_str("f"),
        );
        assert!(
            crate::eval::bc_eval::BCODE_IS_GNU(crate::sexp::accessors::BODY(function)),
            "the pinned fixture must retain and execute its GNU instruction stream"
        );
    });
    let (result, out, err) = session.eval_code_with_output_capture(
        r#"
        environment(f)$stop <- get("stop",baseenv())
        caught <- tryCatch(f(1L), error=function(e) conditionMessage(e))
        environment(f)$stop <- function(...) 99L
        identical(caught,"empty alternative in numeric switch") && is.null(f(1L))
    "#,
    );
    assert_eq!(
        result
            .unwrap_or_else(|error| panic!("{error}; {out:?}; {err:?}"))
            .logical_elt(0),
        Some(1)
    );
}

fn cfg_integers(session: &crate::sexp::session::RSession, words: &[i32]) -> Sexp<'static> {
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let raw = owner
            .with_arena(|arena| {
                let raw = arena.alloc_vector(SEXPTYPE::INTSXP, words.len() as i64);
                let node = arena.node_token(raw).unwrap();
                let payload = arena.heap_identity().payload_lease(&node).unwrap();
                for (index, word) in words.iter().enumerate() {
                    payload.set_integer_elt(index, *word).unwrap();
                }
                raw
            })
            .unwrap();
        owner.sexp(raw).unwrap().into_owned().unwrap()
    })
}

#[test]
fn gnu_cfg_owned_empty_and_sentinel_pools_charge_original_owner_and_reject_revocation() {
    use crate::sexp::session::RSession;
    for length in [0, 1, 2] {
        let session = RSession::new_for_gc_tests();
        let pool = session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let pointer = owner
                .with_arena(|arena| arena.alloc_vector(SEXPTYPE::VECSXP, length))
                .unwrap();
            owner.sexp(pointer).unwrap().into_owned().unwrap()
        });
        let entries = session.with_active(|| {
            if length == 2 {
                let integer = super::tests_util::fixture_integer(&session, 7);
                crate::sexp::object::SexpMut::try_from_checked(pool.clone())
                    .unwrap()
                    .try_set_vector_elt(1, integer)
                    .unwrap();
            }
            (0..length)
                .map(|i| pool.try_vector_elt(i).unwrap().into_owned().unwrap())
                .collect::<Vec<_>>()
        });
        let code: &[i32] = match length {
            0 => &[12, 17, 1],
            1 => &[12, 16, 0, 1],
            _ => &[12, 16, 1, 1],
        };
        let instruction_value = cfg_integers(&session, code);
        session.with_active(|| {
            assert!(
                validate_gnu_adapter_with_owned_constants(
                    code,
                    &entries,
                    &pool,
                    &instruction_value
                )
                .unwrap()
                .0
            );
            let owner = session.owner_token().unwrap();
            owner.full_gc().unwrap();
            owner
                .with_arena(|arena| {
                    let mut budget = arena.budget();
                    budget.max_bytes = arena.heap_identity().byte_ledger().allocated_bytes();
                    arena.set_budget(budget);
                })
                .unwrap();
            let error = validate_gnu_adapter_with_owned_constants(
                code,
                &entries,
                &pool,
                &instruction_value,
            )
            .err()
            .expect("proof buffers require original-owner admission");
            assert!(error.contains("workspace exceeds memory limit"), "{error}");
        });
        drop(session);
        assert!(
            validate_gnu_adapter_with_owned_constants(code, &entries, &pool, &instruction_value)
                .is_err()
        );
    }
}

#[test]
fn gnu_cfg_detached_work_ceiling_preserves_unproven_source_fallback() {
    // A valid nested-call stream has bounded stack effects, but canonical
    // marker lookup exceeds the detached API's explicit proof-work budget.
    let mut words = vec![GNU_BC_MAX_VERSION];
    for _ in 0..2048 {
        words.extend([GNU_OP_GETFUN, 0]);
    }
    for _ in 0..2048 {
        words.extend([GNU_OP_CALL, 0]);
    }
    words.push(GNU_OP_RETURN);
    assert_eq!(validate_gnu_adapter_stream(&words, 1), Ok(false));
    // Malformation established before any ceiling is reached is still an
    // error; budget fallback never approves an unproven stream.
    assert!(validate_gnu_adapter_stream(&[12, GNU_OP_CALL, 0, GNU_OP_RETURN], 1).is_err());
}

fn cfg_pool(session: &crate::sexp::session::RSession, length: i64) -> Sexp<'static> {
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let raw = owner
            .with_arena(|arena| arena.alloc_vector(SEXPTYPE::VECSXP, length))
            .unwrap();
        owner.sexp(raw).unwrap().into_owned().unwrap()
    })
}

fn cfg_force_collection_headroom(
    session: &crate::sexp::session::RSession,
) -> crate::sexp::memory::MemoryLedger {
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        owner
            .with_arena(|arena| {
                // This unrooted payload makes collection, rather than a raised
                // memory limit, supply room for admitted proof scratch.
                let _garbage = arena.alloc_vector(SEXPTYPE::REALSXP, 8192);
                let ledger = arena.heap_identity().byte_ledger();
                let mut budget = arena.budget();
                budget.max_bytes = ledger.allocated_bytes();
                arena.set_budget(budget);
                ledger
            })
            .unwrap()
    })
}

#[test]
fn gnu_cfg_gc_retry_reclaims_and_revalidates_instruction_pool_and_switch_snapshots() {
    use crate::sexp::object::SexpMut;
    use crate::sexp::session::RSession;
    use std::{cell::Cell, rc::Rc};
    for mode in 0..4 {
        let session = RSession::new_for_gc_tests();
        let pool = cfg_pool(&session, if mode == 3 { 4 } else { 1 });
        let offsets = cfg_integers(&session, &[7, 9, 9]);
        if mode == 3 {
            session.with_active(|| {
                SexpMut::try_from_checked(pool.clone())
                    .unwrap()
                    .try_set_vector_elt(3, offsets.clone())
                    .unwrap()
            });
        }
        let words: &[i32] = if mode == 3 {
            &[12, 17, 102, 0, 1, 2, 3, 17, 1, 17, 1]
        } else {
            &[12, 16, 0, 1]
        };
        let instruction_value = cfg_integers(&session, words);
        let replacement = super::tests_util::fixture_integer(&session, 42);
        let observed = Rc::new(Cell::new(false));
        let called = observed.clone();
        let callback_code = instruction_value.clone();
        let callback_pool = pool.clone();
        let original = crate::sexp::owner::StoredOwner::from_value(&pool)
            .unwrap()
            .managed()
            .unwrap();
        session.with_active(|| {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if called.replace(true) {
                    return;
                }
                match mode {
                    0 => {
                        // Temporarily mutate and restore the original R vector.
                        // The raw boundary must hold an owned Rust snapshot, not
                        // a protected shared payload slice, across this callback.
                        SexpMut::try_from_checked(callback_code.clone())
                            .unwrap()
                            .try_set_integer_elt(1, GNU_OP_POP)
                            .unwrap();
                        SexpMut::try_from_checked(callback_code.clone())
                            .unwrap()
                            .try_set_integer_elt(1, GNU_OP_LDCONST)
                            .unwrap();
                        crate::sexp::owner::with_runtime(&original, |access| {
                            access.with_native(|owner| owner.full_gc())
                        })
                        .unwrap()
                        .unwrap();
                    }
                    1 => SexpMut::try_from_checked(callback_code.clone())
                        .unwrap()
                        .try_set_integer_elt(1, GNU_OP_POP)
                        .unwrap(),
                    2 => SexpMut::try_from_checked(callback_pool.clone())
                        .unwrap()
                        .try_set_vector_elt(0, replacement.clone())
                        .unwrap(),
                    _ => SexpMut::try_from_checked(offsets.clone())
                        .unwrap()
                        .try_set_integer_elt(0, 9)
                        .unwrap(),
                }
            }))
        });
        let ledger = cfg_force_collection_headroom(&session);
        let before = ledger.allocated_bytes();
        let result = session.with_active(|| {
            validate_gnu_adapter_with_constants(instruction_value.as_raw(), pool.as_raw())
        });
        assert!(
            observed.get(),
            "mode={mode}: admission must trigger collection"
        );
        assert!(
            ledger.allocated_bytes() < before,
            "collection must reclaim payload backing"
        );
        match mode {
            0 => assert_eq!(result, Ok(true)),
            1 => assert!(result.unwrap_err().contains("instructions changed")),
            2 => assert!(result.unwrap_err().contains("constant pool changed")),
            _ => assert!(result.unwrap_err().contains("SWITCH tables changed")),
        }
    }
}

#[test]
fn gnu_cfg_gc_retry_rejects_owner_revocation_and_keeps_cleanup_storage_pinned() {
    use crate::sexp::session::RSession;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let session = RSession::new_for_gc_tests();
    let pool = cfg_pool(&session, 0);
    let instruction_value = cfg_integers(&session, &[12, 17, 1]);
    let original = session.owner_token().unwrap().weak_owner().unwrap();
    let ledger = cfg_force_collection_headroom(&session);
    let sessions = Rc::new(RefCell::new(Some(session)));
    let callback_sessions = Rc::downgrade(&sessions);
    let observed = Rc::new(Cell::new(false));
    let called = observed.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        called.set(true);
        callback_sessions
            .upgrade()
            .unwrap()
            .borrow_mut()
            .as_mut()
            .unwrap()
            .close();
    }));
    let before = ledger.allocated_bytes();
    let result = crate::sexp::owner::with_runtime(&original, |_| {
        validate_gnu_adapter_with_constants(instruction_value.as_raw(), pool.as_raw())
    });
    assert!(observed.get());
    assert!(result.is_err() || result.as_ref().unwrap().is_err());
    assert!(ledger.allocated_bytes() < before);
    drop(sessions);
}
