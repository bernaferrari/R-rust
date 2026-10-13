//! I/O, printing, and connection builtin entries.

use super::EvaluatedBuiltin;

pub(super) const BASE_IO: &[EvaluatedBuiltin] = &[
    EvaluatedBuiltin {
        name: "cat",
        handler: crate::mainutils::essentials::do_cat,
    },
    EvaluatedBuiltin {
        name: "print",
        handler: crate::mainutils::essentials::do_print,
    },
    EvaluatedBuiltin {
        name: "file_ext",
        handler: crate::mainutils::essentials::do_file_ext,
    },
    EvaluatedBuiltin {
        name: "file_path_sans_ext",
        handler: crate::mainutils::essentials::do_file_path_sans_ext,
    },
    EvaluatedBuiltin {
        name: "loadings",
        handler: crate::mainutils::essentials::do_loadings,
    },
    EvaluatedBuiltin {
        name: "ls.print",
        handler: crate::mainutils::essentials::do_ls_print,
    },
    EvaluatedBuiltin {
        name: "naprint",
        handler: crate::mainutils::essentials::do_naprint,
    },
    EvaluatedBuiltin {
        name: "deparse",
        handler: crate::mainutils::essentials::do_deparse,
    },
    EvaluatedBuiltin {
        name: "file.path",
        handler: crate::mainutils::essentials::do_file_path,
    },
    EvaluatedBuiltin {
        name: "file.show",
        handler: crate::mainutils::platform::do_fileshow,
    },
    EvaluatedBuiltin {
        name: "file.create",
        handler: crate::mainutils::essentials::do_file_create,
    },
    EvaluatedBuiltin {
        name: "file.append",
        handler: crate::mainutils::platform::do_fileappend,
    },
    EvaluatedBuiltin {
        name: "file.link",
        handler: crate::mainutils::platform::do_filelink,
    },
    EvaluatedBuiltin {
        name: "file.symlink",
        handler: crate::mainutils::platform::do_filesymlink,
    },
    EvaluatedBuiltin {
        name: "file.remove",
        handler: crate::mainutils::platform::do_fileremove,
    },
    EvaluatedBuiltin {
        name: "file.rename",
        handler: crate::mainutils::platform::do_filerename,
    },
    EvaluatedBuiltin {
        name: "file.copy",
        handler: crate::mainutils::platform::do_filecopy,
    },
    EvaluatedBuiltin {
        name: "file.access",
        handler: crate::mainutils::platform::do_fileaccess,
    },
    EvaluatedBuiltin {
        name: "file.choose",
        handler: crate::mainutils::platform::do_filechoose,
    },
    EvaluatedBuiltin {
        name: "file",
        handler: crate::mainutils::connections::do_file,
    },
    EvaluatedBuiltin {
        name: "url",
        handler: crate::mainutils::essentials::do_url,
    },
    EvaluatedBuiltin {
        name: "rawConnection",
        handler: crate::mainutils::connections::do_rawConnection,
    },
    EvaluatedBuiltin {
        name: "open",
        handler: crate::mainutils::connections::do_open,
    },
    EvaluatedBuiltin {
        name: "summary.connection",
        handler: crate::mainutils::connections::do_sumConnection,
    },
    EvaluatedBuiltin {
        name: "getAllConnections",
        handler: crate::mainutils::connections::do_getAllConnections,
    },
    EvaluatedBuiltin {
        name: "getConnection",
        handler: crate::mainutils::connections::do_getConnection,
    },
    EvaluatedBuiltin {
        name: "flush",
        handler: crate::mainutils::essentials::do_flush,
    },
    EvaluatedBuiltin {
        name: "flush.console",
        handler: crate::mainutils::essentials::do_flush_console,
    },
    EvaluatedBuiltin {
        name: "gzfile",
        handler: crate::mainutils::essentials::do_gzfile,
    },
    EvaluatedBuiltin {
        name: "gzcon",
        handler: crate::mainutils::connections::do_gzcon,
    },
    EvaluatedBuiltin {
        name: "bzfile",
        handler: crate::mainutils::connections::do_bzfile,
    },
    EvaluatedBuiltin {
        name: "xzfile",
        handler: crate::mainutils::connections::do_xzfile,
    },
    EvaluatedBuiltin {
        name: "memCompress",
        handler: crate::mainutils::connections::do_memCompress,
    },
    EvaluatedBuiltin {
        name: "memDecompress",
        handler: crate::mainutils::connections::do_memDecompress,
    },
    EvaluatedBuiltin {
        name: "pipe",
        handler: crate::mainutils::essentials::do_pipe,
    },
    EvaluatedBuiltin {
        name: "socketConnection",
        handler: crate::mainutils::essentials::do_socketConnection,
    },
    EvaluatedBuiltin {
        name: "seek",
        handler: crate::mainutils::essentials::do_seek,
    },
    EvaluatedBuiltin {
        name: "pushBack",
        handler: crate::mainutils::essentials::do_pushBack,
    },
    EvaluatedBuiltin {
        name: "pushBackLength",
        handler: crate::mainutils::essentials::do_pushBackLength,
    },
    EvaluatedBuiltin {
        name: "readBin",
        handler: crate::mainutils::essentials::do_readBin,
    },
    EvaluatedBuiltin {
        name: "writeBin",
        handler: crate::mainutils::essentials::do_writeBin,
    },
    EvaluatedBuiltin {
        name: "printCoefmat",
        handler: crate::mainutils::essentials::do_print_coefmat,
    },
    EvaluatedBuiltin {
        name: "print.default",
        handler: crate::mainutils::essentials::do_print_default,
    },
    EvaluatedBuiltin {
        name: "print.data.frame",
        handler: crate::mainutils::essentials::do_print_data_frame,
    },
    EvaluatedBuiltin {
        name: "print.table",
        handler: crate::mainutils::essentials::do_print_table,
    },
    EvaluatedBuiltin {
        name: "print.factor",
        handler: crate::mainutils::essentials::do_print_factor,
    },
    EvaluatedBuiltin {
        name: "print.eigen",
        handler: crate::mainutils::essentials::do_print_eigen,
    },
    EvaluatedBuiltin {
        name: "print.rle",
        handler: crate::mainutils::essentials::do_print_rle,
    },
    EvaluatedBuiltin {
        name: "print.proc_time",
        handler: crate::mainutils::essentials::do_print_proc_time,
    },
    EvaluatedBuiltin {
        name: "env.profile",
        handler: crate::mainutils::essentials::do_envprofile,
    },
    EvaluatedBuiltin {
        name: "removeSource",
        handler: crate::mainutils::srcref::do_remove_source,
    },
    EvaluatedBuiltin {
        name: "scan",
        handler: crate::mainutils::essentials::do_scan,
    },
    EvaluatedBuiltin {
        name: "write.table",
        handler: crate::mainutils::essentials::do_write_table,
    },
    EvaluatedBuiltin {
        name: "stdin",
        handler: crate::mainutils::connections::do_stdin,
    },
    EvaluatedBuiltin {
        name: "stdout",
        handler: crate::mainutils::connections::do_stdout,
    },
    EvaluatedBuiltin {
        name: "stderr",
        handler: crate::mainutils::connections::do_stderr,
    },
    EvaluatedBuiltin {
        name: "isatty",
        handler: crate::mainutils::connections::do_isatty,
    },
    EvaluatedBuiltin {
        name: "readLines",
        handler: crate::mainutils::connections::do_readLines,
    },
    EvaluatedBuiltin {
        name: "writeLines",
        handler: crate::mainutils::essentials::do_writeLines,
    },
    EvaluatedBuiltin {
        name: "sink",
        handler: crate::mainutils::essentials::do_sink,
    },
    EvaluatedBuiltin {
        name: "sink.number",
        handler: crate::mainutils::essentials::do_sink_number,
    },
    EvaluatedBuiltin {
        name: "system.file",
        handler: crate::mainutils::essentials::do_system_file,
    },
    EvaluatedBuiltin {
        name: "deparse1",
        handler: crate::mainutils::essentials::do_deparse1,
    },
    EvaluatedBuiltin {
        name: "dput",
        handler: crate::mainutils::essentials::do_dput,
    },
    EvaluatedBuiltin {
        name: "dump",
        handler: crate::mainutils::essentials::do_dump,
    },
    EvaluatedBuiltin {
        name: "print.function",
        handler: crate::mainutils::essentials::do_print_function,
    },
    EvaluatedBuiltin {
        name: "print.integer",
        handler: crate::mainutils::essentials::do_print_integer,
    },
    EvaluatedBuiltin {
        name: "print.numeric",
        handler: crate::mainutils::essentials::do_print_numeric,
    },
    EvaluatedBuiltin {
        name: "print.logical",
        handler: crate::mainutils::essentials::do_print_logical,
    },
    EvaluatedBuiltin {
        name: "print.complex",
        handler: crate::mainutils::essentials::do_print_complex,
    },
    EvaluatedBuiltin {
        name: "print.environment",
        handler: crate::mainutils::essentials::do_print_environment,
    },
    EvaluatedBuiltin {
        name: "print.formula",
        handler: crate::mainutils::essentials::do_print_formula,
    },
    EvaluatedBuiltin {
        name: "print.raw",
        handler: crate::mainutils::essentials::do_print_raw,
    },
    EvaluatedBuiltin {
        name: "read.ftable",
        handler: crate::mainutils::essentials::do_read_ftable,
    },
    EvaluatedBuiltin {
        name: "write.ftable",
        handler: crate::mainutils::essentials::do_write_ftable,
    },
    EvaluatedBuiltin {
        name: "Sys.readlink",
        handler: crate::mainutils::platform::do_readlink,
    },
    EvaluatedBuiltin {
        name: "read.csv",
        handler: crate::mainutils::essentials::do_read_csv,
    },
    EvaluatedBuiltin {
        name: "write.csv",
        handler: crate::mainutils::essentials::do_write_csv,
    },
    EvaluatedBuiltin {
        name: "read.table",
        handler: crate::mainutils::essentials::do_read_table,
    },
    EvaluatedBuiltin {
        name: "memory.profile",
        handler: crate::mainutils::essentials::do_memory_profile,
    },
    EvaluatedBuiltin {
        name: "read.csv2",
        handler: crate::mainutils::essentials::do_read_csv2,
    },
    EvaluatedBuiltin {
        name: "write.csv2",
        handler: crate::mainutils::essentials::do_write_csv2,
    },
    EvaluatedBuiltin {
        name: "read.delim",
        handler: crate::mainutils::essentials::do_read_delim,
    },
    EvaluatedBuiltin {
        name: "readChar",
        handler: crate::mainutils::essentials::do_readChar,
    },
    EvaluatedBuiltin {
        name: "writeChar",
        handler: crate::mainutils::essentials::do_writeChar,
    },
    EvaluatedBuiltin {
        name: "readRDS",
        handler: crate::mainutils::essentials::do_loadRDS,
    },
    EvaluatedBuiltin {
        name: "saveRDS",
        handler: crate::mainutils::essentials::do_saveRDS,
    },
    EvaluatedBuiltin {
        name: "lazyLoadDBfetch",
        handler: crate::mainutils::serialize::do_lazyLoadDBfetch,
    },
    EvaluatedBuiltin {
        name: "serialize",
        handler: crate::mainutils::serialize::do_serialize,
    },
    EvaluatedBuiltin {
        name: "unserialize",
        handler: crate::mainutils::serialize::do_unserializeFromConn,
    },
    EvaluatedBuiltin {
        name: "load",
        handler: crate::mainutils::saveload::do_load_user,
    },
    EvaluatedBuiltin {
        name: "source",
        handler: crate::mainutils::essentials::do_source,
    },
    EvaluatedBuiltin {
        name: "sys.source",
        handler: crate::mainutils::essentials::do_sys_source,
    },
    EvaluatedBuiltin {
        name: "tempfile",
        handler: crate::mainutils::essentials::do_tempfile,
    },
    EvaluatedBuiltin {
        name: "file.exists",
        handler: crate::mainutils::platform::do_fileexists,
    },
    EvaluatedBuiltin {
        name: "file.info",
        handler: crate::mainutils::platform::do_fileinfo,
    },
    EvaluatedBuiltin {
        name: "file.size",
        handler: crate::mainutils::platform::do_filesize,
    },
    EvaluatedBuiltin {
        name: "file.mtime",
        handler: crate::mainutils::platform::do_filemtime,
    },
    EvaluatedBuiltin {
        name: "setFileTime",
        handler: crate::mainutils::platform::do_setfiletime,
    },
    EvaluatedBuiltin {
        name: "list.files",
        handler: crate::mainutils::platform::do_listfiles,
    },
];
