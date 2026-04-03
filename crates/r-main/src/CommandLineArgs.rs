/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997-2023   The R Core Team
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 */

use libc::{c_int, c_char, size_t, calloc, strdup, strtol, strncpy};
use crate::defn::*;
use crate::r_ext::RStartup::*;

/* Remove and process common command-line arguments
 *  Formally part of ../unix/sys-common.c.
 */

/*
  This copies the command line arguments to the Rstart
  structure. The memory is obtained from calloc, etc.
  since these are permanent and it is not intended that
  they be modified. This is why they are copied before
  being processed and removed from the list.

  We might store these as a SEXP. I have no strong opinion
  about this.
 */

/* Permanent copy of the command line arguments and the number
   of them passed to the application.
   These are populated via the routine R_set_command_line_arguments().
 */
static mut NumCommandLineArgs: c_int = 0;
static mut CommandLineArgs: *mut *mut c_char = std::ptr::null_mut(); // this does not get freed


#[no_mangle]
pub unsafe extern "C" fn R_set_command_line_arguments(argc: c_int, argv: *mut *mut c_char) {
    // nothing here is ever freed.
    NumCommandLineArgs = argc;
    CommandLineArgs = calloc(argc as size_t, std::mem::size_of::<*mut c_char>() as size_t) as *mut *mut c_char;
    if CommandLineArgs.is_null() {
        R_Suicide(b"allocation failure in R_set_command_line_arguments\0" as *const u8 as *const c_char);
    }

    for i in 0..argc as isize {
        *CommandLineArgs.offset(i) = strdup(*argv.offset(i));
        if (*CommandLineArgs.offset(i)).is_null() {
            R_Suicide(b"allocation failure in R_set_command_line_arguments\0" as *const u8 as *const c_char);
        }
    }
}


/*
  The .Internal which returns the command line arguments that are stored
  in global variables.
 */
#[no_mangle]
pub unsafe extern "C" fn do_commandArgs(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let vals: SEXP;

    checkArity(op, args);
    /* need protection as mkChar allocates */
    vals = PROTECT(allocVector(STRSXP, NumCommandLineArgs));
    for i in 0..NumCommandLineArgs as isize {
        SET_STRING_ELT(vals, i, mkChar(*CommandLineArgs.offset(i)));
    }
    UNPROTECT(1);
    return vals;
}

#[cfg(windows)]
extern "C" {
    static mut R_LoadRconsole: Rboolean;
}

#[no_mangle]
pub unsafe extern "C" fn R_common_command_line(pac: *mut c_int, argv: *mut *mut c_char, Rp: *mut Rstart) {
    let mut ac = *pac;
    let mut newac = 1;	/* argv[0] is process name */
    let mut lval: libc::c_long; /* this is used for ppval, so 32-bit long is fine */
    let mut p: *mut c_char;
    let mut av = argv;
    let mut msg: [c_char; 1024] = [0; 1024];
    let mut processing = TRUE;

    R_RestoreHistory = 1;
    while { ac -= 1; ac } != 0 {
        av = av.offset(1);
        if processing && **av == b'-' as c_char {
            if !strcmp(*av, b"--version\0" as *const u8 as *const c_char) {
                PrintVersion(msg.as_mut_ptr(), 1024);
                R_ShowMessage(msg.as_ptr());
                libc::exit(0);
            }
            else if !strcmp(*av, b"--args\0" as *const u8 as *const c_char) {
                /* copy this through for further processing */
                *argv.offset(newac as isize) = *av;
                newac += 1;
                processing = FALSE;
            }
            else if !strcmp(*av, b"--save\0" as *const u8 as *const c_char) {
                (*Rp).SaveAction = SA_SAVE;
            }
            else if !strcmp(*av, b"--no-save\0" as *const u8 as *const c_char) {
                (*Rp).SaveAction = SA_NOSAVE;
            }
            else if !strcmp(*av, b"--restore\0" as *const u8 as *const c_char) {
                (*Rp).RestoreAction = SA_RESTORE;
            }
            else if !strcmp(*av, b"--no-restore\0" as *const u8 as *const c_char) {
                (*Rp).RestoreAction = SA_NORESTORE;
                R_RestoreHistory = 0;
            }
            else if !strcmp(*av, b"--no-restore-data\0" as *const u8 as *const c_char) {
                (*Rp).RestoreAction = SA_NORESTORE;
            }
            else if !strcmp(*av, b"--no-restore-history\0" as *const u8 as *const c_char) {
                R_RestoreHistory = 0;
            }
            else if (!strcmp(*av, b"--silent\0" as *const u8 as *const c_char) ||
                     !strcmp(*av, b"--quiet\0" as *const u8 as *const c_char) ||
                     !strcmp(*av, b"-q\0" as *const u8 as *const c_char)) {
                (*Rp).R_Quiet = TRUE;
            }
            else if !strcmp(*av, b"--vanilla\0" as *const u8 as *const c_char) {
                (*Rp).SaveAction = SA_NOSAVE; /* --no-save */
                (*Rp).RestoreAction = SA_NORESTORE; /* --no-restore */
                R_RestoreHistory = 0;     // --no-restore-history (= part of --no-restore)
                (*Rp).LoadSiteFile = FALSE; /* --no-site-file */
                (*Rp).LoadInitFile = FALSE; /* --no-init-file */
                (*Rp).NoRenviron = TRUE;    // --no-environ
                #[cfg(windows)]
                {
                    R_LoadRconsole = FALSE;
                }
            }
            else if !strcmp(*av, b"--no-environ\0" as *const u8 as *const c_char) {
                (*Rp).NoRenviron = TRUE;
            }
            else if !strcmp(*av, b"--verbose\0" as *const u8 as *const c_char) {
                (*Rp).R_Verbose = TRUE;
            }
            else if (!strcmp(*av, b"--no-echo\0" as *const u8 as *const c_char) ||
                     !strcmp(*av, b"--slave\0" as *const u8 as *const c_char) || // "deprecated" from R 4.0.0 (spring 2020)
                     !strcmp(*av, b"-s\0" as *const u8 as *const c_char)) {
                (*Rp).R_Quiet = TRUE;
                (*Rp).R_NoEcho = TRUE;
                (*Rp).SaveAction = SA_NOSAVE;
            }
            else if !strcmp(*av, b"--no-site-file\0" as *const u8 as *const c_char) {
                (*Rp).LoadSiteFile = FALSE;
            }
            else if !strcmp(*av, b"--no-init-file\0" as *const u8 as *const c_char) {
                (*Rp).LoadInitFile = FALSE;
            }
            /* Undocumented and unused.
             else if (!strcmp(*av, "--debug-init")) {
            	Rp->DebugInitFile = TRUE;
             */
            else if strncmp(*av, b"--encoding\0" as *const u8 as *const c_char, 10) == 0 {
                if strlen(*av) < 12 {
                    if ac > 1 {
                        ac -= 1;
                        av = av.offset(1);
                        p = *av;
                    } else {
                        p = std::ptr::null_mut();
                    }
                } else {
                    p = (*av).offset(11);
                }
                if p.is_null() {
                    R_ShowMessage(_(b"WARNING: no value given for --encoding\0" as *const u8 as *const c_char));
                } else {
                    strncpy(R_StdinEnc.as_mut_ptr(), p, 30);
                    R_StdinEnc[30] = 0;
                }
            }
            #[cfg(windows)]
            else if !strcmp(*av, b"--no-Rconsole\0" as *const u8 as *const c_char) {
                R_LoadRconsole = 0;
            }
            else if (!strcmp(*av, b"-save\0" as *const u8 as *const c_char) ||
                     !strcmp(*av, b"-nosave\0" as *const u8 as *const c_char) ||
                     !strcmp(*av, b"-restore\0" as *const u8 as *const c_char) ||
                     !strcmp(*av, b"-norestore\0" as *const u8 as *const c_char) ||
                     !strcmp(*av, b"-noreadline\0" as *const u8 as *const c_char) ||
                     !strcmp(*av, b"-quiet\0" as *const u8 as *const c_char) ||
                     !strcmp(*av, b"-nsize\0" as *const u8 as *const c_char) ||
                     !strcmp(*av, b"-vsize\0" as *const u8 as *const c_char) ||
                     strncmp(*av, b"--max-nsize\0" as *const u8 as *const c_char, 11) == 0 ||
                     strncmp(*av, b"--max-vsize\0" as *const u8 as *const c_char, 11) == 0 ||
                     !strcmp(*av, b"-V\0" as *const u8 as *const c_char) ||
                     !strcmp(*av, b"-n\0" as *const u8 as *const c_char) ||
                     !strcmp(*av, b"-v\0" as *const u8 as *const c_char)) {
                snprintf(msg.as_mut_ptr(), 1024,
                         _(b"WARNING: option '%s' no longer supported\0" as *const u8 as *const c_char), *av);
                R_ShowMessage(msg.as_ptr());
            }
            /* mop up --min-[nv]size */
            else if strncmp(*av, b"--min-nsize\0" as *const u8 as *const c_char, 11) == 0 ||
                     strncmp(*av, b"--min-vsize\0" as *const u8 as *const c_char, 11) == 0 {
                if strlen(*av) < 13 {
                    if ac > 1 {
                        ac -= 1;
                        av = av.offset(1);
                        p = *av;
                    } else {
                        p = std::ptr::null_mut();
                    }
                } else {
                    p = (*av).offset(12);
                }
                if p.is_null() {
                    snprintf(msg.as_mut_ptr(), 1024,
                             _(b"WARNING: no value given for '%s'\0" as *const u8 as *const c_char), *av);
                    R_ShowMessage(msg.as_ptr());
                    break;
                }
                let ierr: c_int;
                let value: R_size_t;
                value = R_Decode2Long(p, &ierr);
                if ierr != 0 {
                    if ierr < 0 {
                        snprintf(msg.as_mut_ptr(), 1024,
                                 _(b"WARNING: '%s' value is invalid: ignored\0" as *const u8 as *const c_char),
                                 *av);
                    }
                    else {
                        snprintf(msg.as_mut_ptr(), 1024,
                                 _(b"WARNING: %s: too large and ignored\0" as *const u8 as *const c_char),
                                 *av);
                    }
                    R_ShowMessage(msg.as_ptr());

                } else {
                    if strncmp(*av, b"--min-nsize\0" as *const u8 as *const c_char, 11) == 0 {
                        (*Rp).nsize = value;
                    }
                    if strncmp(*av, b"--min-vsize\0" as *const u8 as *const c_char, 11) == 0 {
                        (*Rp).vsize = value;
                    }
                }
            }
            else if strncmp(*av, b"--max-ppsize\0" as *const u8 as *const c_char, 12) == 0 {
                if strlen(*av) < 14 {
                    if ac > 1 {
                        ac -= 1;
                        av = av.offset(1);
                        p = *av;
                    } else {
                        p = std::ptr::null_mut();
                    }
                } else {
                    p = (*av).offset(13);
                }
                if p.is_null() {
                    R_ShowMessage(_(b"WARNING: no value given for '--max-ppsize'\0" as *const u8 as *const c_char));
                    break;
                }
                lval = strtol(p, &mut p, 10);
                if lval < 0 {
                    R_ShowMessage(_(b"WARNING: '--max-ppsize' value is negative: ignored\0" as *const u8 as *const c_char));
                }
                else if lval < 10000 {
                    R_ShowMessage(_(b"WARNING: '--max-ppsize' value is too small: ignored\0" as *const u8 as *const c_char));
                }
                else if lval > 500000 {
                    R_ShowMessage(_(b"WARNING: '--max-ppsize' value is too large: ignored\0" as *const u8 as *const c_char));
                }
                else {
                    (*Rp).ppsize = lval as size_t;
                }
            }
            else if strncmp(*av, b"--max-connections\0" as *const u8 as *const c_char, 17) == 0 {
                if strlen(*av) < 19 {
                    if ac > 1 {
                        ac -= 1;
                        av = av.offset(1);
                        p = *av;
                    } else {
                        p = std::ptr::null_mut();
                    }
                } else {
                    p = (*av).offset(18);
                }
                if p.is_null() {
                    R_ShowMessage(_(b"WARNING: no value given for '--max-connections'\0" as *const u8 as *const c_char));
                    break;
                }
                lval = strtol(p, &mut p, 10);
                if lval < 0 {
                    R_ShowMessage(_(b"WARNING: '--max-connections' value is negative: ignored\0" as *const u8 as *const c_char));
                }
                else if lval < 128 {
                    R_ShowMessage(_(b"WARNING: '--max-connections' value is too small: ignored\0" as *const u8 as *const c_char));
                }
                else if lval > 4096 {
                    R_ShowMessage(_(b"WARNING: '--max-connections' value is too large: ignored\0" as *const u8 as *const c_char));
                }
                else {
                    (*Rp).nconnections = lval as c_int;
                }
            }
            else { /* unknown -option */
                *argv.offset(newac as isize) = *av;
                newac += 1;
            }
        }
        else {
            *argv.offset(newac as isize) = *av;
            newac += 1;
        }
    }
    *pac = newac;
    return;
}
