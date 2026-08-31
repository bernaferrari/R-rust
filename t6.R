e <- new.env(); e$`?`  # check primitive
cat(typeof(get("?")),"\n")
local({ detach("package:utils", unload=FALSE); tryCatch(eval(parse(text="?x")), error=function(e) cat("ERR:",conditionMessage(e),"\n")) })
