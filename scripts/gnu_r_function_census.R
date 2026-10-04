#!/usr/bin/env Rscript
# GNU R namespace/native inventory STARTER.
# Run under the pinned GNU R oracle, not under R-rust:
#   GNU_R_SOURCE_COMMIT=... Rscript --vanilla scripts/gnu_r_function_census.R /tmp/gnu-r-census

options(warn = 1, stringsAsFactors = FALSE)
cli <- commandArgs(trailingOnly = TRUE)
if (length(cli) < 1L) {
  stop('Usage: Rscript --vanilla gnu_r_function_census.R OUTPUT_DIR [PACKAGE ...]',
       call. = FALSE)
}
out <- cli[[1L]]
if (dir.exists(out) && length(list.files(out, all.files = TRUE, no.. = TRUE))) {
  stop('OUTPUT_DIR must be new or empty; refusing to overwrite a census.', call. = FALSE)
}
if (!dir.exists(out) && !dir.create(out, recursive = TRUE)) {
  stop('Could not create OUTPUT_DIR.', call. = FALSE)
}

issues <- list()
record_issue <- function(stage, item, severity, message) {
  issues[[length(issues) + 1L]] <<- data.frame(
    stage = stage, item = item, severity = severity, message = message,
    stringsAsFactors = FALSE
  )
}
checked <- function(expr, stage, item) {
  tryCatch(
    withCallingHandlers(
      list(ok = TRUE, value = force(expr)),
      warning = function(w) {
        record_issue(stage, item, 'warning', conditionMessage(w))
        invokeRestart('muffleWarning')
      }
    ),
    error = function(e) {
      record_issue(stage, item, 'error', conditionMessage(e))
      list(ok = FALSE, value = NULL)
    }
  )
}
cell <- function(x) {
  if (is.null(x) || length(x) == 0L) '' else paste(as.character(x), collapse = ' | ')
}
write_rows <- function(rows, columns, filename) {
  if (length(rows)) {
    result <- do.call(rbind, rows)
  } else {
    result <- as.data.frame(setNames(rep(list(character()), length(columns)), columns),
                            stringsAsFactors = FALSE)
  }
  utils::write.csv(result, file.path(out, filename), row.names = FALSE,
                   na = '', fileEncoding = 'UTF-8')
  invisible(result)
}

installed <- utils::installed.packages(lib.loc = .Library, noCache = TRUE)
if (length(cli) > 1L) {
  packages <- sort(unique(cli[-1L]))
} else {
  packages <- sort(unique(installed[
    !is.na(installed[, 'Priority']) &
      installed[, 'Priority'] %in% c('base', 'recommended'), 'Package'
  ]))
}
if (!length(packages)) stop('No packages selected from the oracle installation.')

bindings <- list()
package_rows <- list()
namespace_dlls <- list()
s3_tables <- list()
function_records <- list()

for (pkg in packages) {
  message('Inspecting ', pkg)
  idx <- match(pkg, installed[, 'Package'])
  version <- if (is.na(idx)) '' else installed[idx, 'Version']
  priority <- if (is.na(idx)) '' else cell(installed[idx, 'Priority'])
  ns_result <- checked(loadNamespace(pkg, lib.loc = .Library), 'namespace_load', pkg)
  if (!ns_result$ok) {
    package_rows[[length(package_rows) + 1L]] <- data.frame(
      package = pkg, version = version, priority = priority, status = 'load_failed',
      bindings_seen = 0L, functions_seen = 0L, stringsAsFactors = FALSE
    )
    next
  }
  ns <- ns_result$value
  locals_result <- checked(ls(envir = ns, all.names = TRUE, sorted = TRUE),
                           'namespace_names', pkg)
  exports_result <- checked(getNamespaceExports(ns), 'namespace_exports', pkg)
  locals <- if (locals_result$ok) locals_result$value else character()
  exports <- if (exports_result$ok) exports_result$value else character()
  names_to_inspect <- sort(unique(c(locals, exports)))
  n_functions <- 0L

  for (name in names_to_inspect) {
    id <- paste0(pkg, ':::', name)
    local <- name %in% locals
    exported <- name %in% exports
    kind <- 'unresolved'
    function_type <- ''
    formal_text <- ''
    args_text <- ''
    defining_environment <- ''
    is_fun <- FALSE
    status <- 'inspected'

    active <- if (local) checked(bindingIsActive(name, ns), 'active_binding', id) else
      list(ok = TRUE, value = FALSE)
    if (!active$ok) {
      status <- 'inspection_failed'
    } else if (isTRUE(active$value)) {
      status <- 'active_binding_not_forced'
      record_issue('binding', id, 'incomplete', 'Active binding was not invoked.')
    } else {
      value_result <- checked(
        if (local) get(name, envir = ns, inherits = FALSE) else getExportedValue(pkg, name),
        'binding_read', id
      )
      if (!value_result$ok) {
        status <- 'inspection_failed'
      } else {
        object <- value_result$value
        kind <- typeof(object)
        is_fun <- is.function(object)
        if (is_fun) {
          n_functions <- n_functions + 1L
          function_type <- if (is.primitive(object)) kind else 'closure'
          formal_result <- checked(
            paste(utils::capture.output(dput(formals(object))), collapse = '\n'),
            'function_formals', id
          )
          formal_text <- if (formal_result$ok) formal_result$value else ''
          args_result <- checked(
            paste(utils::capture.output(dput(args(object))), collapse = '\n'),
            'function_documented_args', id
          )
          args_text <- if (args_result$ok) args_result$value else ''
          env_result <- checked(
            if (is.primitive(object)) '' else environmentName(environment(object)),
            'function_environment', id
          )
          defining_environment <- if (env_result$ok) env_result$value else ''
          syntax_result <- checked(
            list(package = pkg, name = name, type = function_type,
                 formals = formals(object),
                 body = if (is.primitive(object)) NULL else body(object)),
            'function_syntax', id
          )
          if (syntax_result$ok) function_records[[id]] <- syntax_result$value
          if (!formal_result$ok || !args_result$ok || !env_result$ok || !syntax_result$ok)
            status <- 'partially_inspected'
        }
      }
    }
    bindings[[length(bindings) + 1L]] <- data.frame(
      package = pkg, package_version = version, name = name,
      exported = exported, local_binding = local, type = kind,
      is_function = is_fun, function_type = function_type,
      defining_environment = defining_environment, formals = formal_text,
      documented_args = args_text, inventory_status = status,
      port_implementation_status = 'unclassified',
      port_behavior_status = 'not_tested', port_safety_status = 'not_assessed',
      stringsAsFactors = FALSE
    )
  }

  if (pkg != 'base') {
    s3_result <- checked(getNamespaceInfo(ns, 'S3methods'), 's3_registrations', pkg)
    if (s3_result$ok) s3_tables[pkg] <- list(s3_result$value)
    # Pure R namespaces legitimately omit the DLLs field. Its absence is not
    # a failed native inventory; actual metadata/read failures remain issues.
    dll_result <- checked({
      info <- get('.__NAMESPACE__.', envir = ns, inherits = FALSE)
      if (exists('DLLs', envir = info, inherits = FALSE)) {
        getNamespaceInfo(ns, 'DLLs')
      } else {
        list()
      }
    }, 'namespace_dlls', pkg)
    if (dll_result$ok && length(dll_result$value)) {
      for (dll in dll_result$value) {
        namespace_dlls[[length(namespace_dlls) + 1L]] <- data.frame(
          package = pkg, dll = cell(dll[['name']]), dll_path = cell(dll[['path']]),
          stringsAsFactors = FALSE
        )
      }
    }
  }
  package_rows[[length(package_rows) + 1L]] <- data.frame(
    package = pkg, version = version, priority = priority, status = 'loaded',
    bindings_seen = length(names_to_inspect), functions_seen = n_functions,
    stringsAsFactors = FALSE
  )
}

native <- list()
loaded_dlls <- getLoadedDLLs()
for (dll in loaded_dlls) {
  dll_name <- cell(dll[['name']])
  routines <- checked(getDLLRegisteredRoutines(dll), 'native_registrations', dll_name)
  if (!routines$ok) next
  for (interface in names(routines$value)) {
    entries <- routines$value[[interface]]
    for (entry in entries) {
      native[[length(native) + 1L]] <- data.frame(
        dll = dll_name, dll_path = cell(dll[['path']]), interface = interface,
        name = cell(entry[['name']]), num_parameters = cell(entry[['numParameters']]),
        port_implementation_status = 'unclassified',
        port_behavior_status = 'not_tested', port_safety_status = 'not_assessed',
        stringsAsFactors = FALSE
      )
    }
  }
}

binding_df <- write_rows(bindings,
  c('package', 'package_version', 'name', 'exported', 'local_binding', 'type',
    'is_function', 'function_type', 'defining_environment', 'formals',
    'documented_args', 'inventory_status', 'port_implementation_status',
    'port_behavior_status', 'port_safety_status'), 'namespace_bindings.csv')
utils::write.csv(binding_df[binding_df$is_function, , drop = FALSE],
                 file.path(out, 'functions.csv'), row.names = FALSE, na = '',
                 fileEncoding = 'UTF-8')
write_rows(package_rows, c('package', 'version', 'priority', 'status',
                          'bindings_seen', 'functions_seen'), 'packages.csv')
write_rows(native, c('dll', 'dll_path', 'interface', 'name', 'num_parameters',
                     'port_implementation_status', 'port_behavior_status',
                     'port_safety_status'), 'native_routines.csv')
write_rows(namespace_dlls, c('package', 'dll', 'dll_path'), 'namespace_dlls.csv')
issue_df <- write_rows(issues, c('stage', 'item', 'severity', 'message'), 'issues.csv')
saveRDS(s3_tables, file.path(out, 's3_registrations.rds'), version = 2)
saveRDS(function_records, file.path(out, 'function_syntax.rds'), version = 2)
saveRDS(list(R_version = R.version, capabilities = capabilities(),
             platform = .Platform, sys_info = Sys.info(), locale = Sys.getlocale(),
             session_info = utils::sessionInfo(), selected_packages = packages,
             library = .Library,
             declared_source_commit = Sys.getenv('GNU_R_SOURCE_COMMIT', unset = NA_character_),
             source_commit_independently_verified = FALSE),
        file.path(out, 'oracle_metadata.rds'), version = 2)
writeLines(c(
  'This is an oracle census, NOT a statement of R-rust compatibility.',
  'The selected package list and all loading/inspection failures are recorded.',
  'Reconcile packages against the pinned distribution manifest; this script does not discover absent packages.',
  'Reconcile .Internal/.Primitive entries against R_FunTab in the pinned source.',
  'Reconcile native registration source and public headers, including conditional/platform-only APIs.',
  'Separately inventory generated S4 method tables/classes and package test/data/help assets.',
  'Do not compare documented primitive args() as if they fully specified primitive matching behavior.',
  'Run independent behavior tests before changing port_behavior_status from not_tested.',
  'The GNU_R_SOURCE_COMMIT environment variable is recorded, not authenticated.'
), file.path(out, 'REMAINING_INVENTORY_OBLIGATIONS.txt'))
message('Recorded ', sum(as.logical(binding_df$is_function), na.rm = TRUE), ' function bindings and ',
        length(native), ' native registrations in ', normalizePath(out), '.')
if (nrow(issue_df) && any(issue_df$severity %in% c('error', 'incomplete'))) {
  message('Census has unresolved entries; inspect issues.csv. It is not complete.')
  quit(status = 1L, save = 'no')
}
message('Selected-namespace scan completed. Source/platform/S4 reconciliation is still required.')
