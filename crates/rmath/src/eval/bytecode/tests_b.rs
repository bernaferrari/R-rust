//! bytecode tests (part 2/2) — extracted verbatim from eval/bytecode.rs

use super::tests_util::*;
use super::*;
use crate::sexp::session::RSession;

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
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 61, 1, 1], 2).is_err());
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
